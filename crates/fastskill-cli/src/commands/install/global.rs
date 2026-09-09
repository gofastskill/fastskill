use super::{print_install_json, InstallArgs, InstallJsonResult, InstallJsonTarget};
use crate::config::{create_service_config, inject_edge_services};
use crate::error::{CliError, CliResult};
use fastskill_core::core::lifecycle_transaction::LifecycleTransaction;
use fastskill_core::core::lock::{global_lock_path, GlobalLockedSkillEntry, GlobalSkillsLock};
use fastskill_core::core::origin::Origin;
use fastskill_core::core::resolution::{
    prepare_locked_resolution_entries, prepare_locked_resolution_entries_preview,
    ResolutionLockEntry, ResolutionRoot,
};
use fastskill_core::core::skill_manager::SkillDefinition;
use fastskill_core::core::state_guard::StateMutationGuard;
use fastskill_core::core::AddMode;
use fastskill_core::{FastSkillService, SkillId};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[cfg(test)]
static FAIL_AFTER_FIRST_GLOBAL_APPLY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) async fn execute_global_install(args: InstallArgs) -> CliResult<()> {
    validate_options(&args)?;
    let lock_path = global_lock_path().map_err(|error| {
        CliError::Config(format!("Failed to resolve global lock path: {error}"))
    })?;
    if !lock_path.exists() {
        return Err(CliError::Config(
            "global-skills.lock not found. Run 'fastskill --global add <skill>' first.".to_string(),
        ));
    }
    let original_lock = std::fs::read(&lock_path).map_err(CliError::Io)?;
    let lock = GlobalSkillsLock::load_from_file(&lock_path)
        .map_err(|error| CliError::Config(format!("Failed to load global lock: {error}")))?;
    let roots = selected_roots(&lock, &args)?;
    if roots.is_empty() {
        if args.json {
            print_install_json(InstallJsonResult {
                scope: "global",
                outcome: "unchanged".to_string(),
                dry_run: args.dry_run,
                targets: Vec::new(),
                diagnostics: vec!["No global skills matched the selected groups".to_string()],
            })?;
        } else {
            crate::outln!("No global skills matched the selected groups");
        }
        return Ok(());
    }

    let config = create_service_config(true, None)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;
    let service = inject_edge_services(service)?;
    let locked = lock
        .skills
        .iter()
        .map(|entry| {
            (
                entry.id.clone(),
                ResolutionLockEntry {
                    origin: entry.origin.clone(),
                    resolved: entry.resolved.clone(),
                    dependencies: entry.dependencies.clone(),
                    groups: entry.groups.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let max_levels = args
        .depth
        .map(u32::try_from)
        .transpose()
        .map_err(|_| CliError::InvalidDepth("Depth must be at least 1".to_string()))?
        .unwrap_or(u32::MAX);
    let mut plan = if args.dry_run && !args.offline {
        prepare_locked_resolution_entries_preview(&service, roots, &locked, max_levels).await
    } else {
        prepare_locked_resolution_entries(&service, roots, &locked, max_levels, args.offline).await
    }
    .map_err(CliError::Service)?;
    let mut targets = Vec::new();
    let mut changed_ids = BTreeSet::new();
    for candidate in &plan.candidates {
        let id = candidate.prepared.id().to_string();
        let installed = service.config().skill_storage_path.join(&id);
        let locked_entry = lock.skills.iter().find(|entry| entry.id == id);
        let unchanged = locked_entry.is_some_and(|entry| installed_matches(&installed, entry));
        if !unchanged {
            changed_ids.insert(id.clone());
        }
        targets.push(InstallJsonTarget {
            id,
            outcome: if unchanged { "unchanged" } else { "changed" }.to_string(),
            current_revision: installed
                .exists()
                .then(|| locked_entry.map(|entry| entry.resolved.version.clone()))
                .flatten(),
            target_revision: Some(candidate.prepared.resolved().version.clone()),
            changes: if unchanged {
                Vec::new()
            } else {
                vec!["restore verified content from global lock".to_string()]
            },
            retained: if unchanged {
                vec!["verified content already installed".to_string()]
            } else {
                Vec::new()
            },
        });
    }

    if !args.dry_run && !changed_ids.is_empty() {
        let state_root = lock_path.parent().ok_or_else(|| {
            CliError::Config("global-skills.lock has no parent directory".to_string())
        })?;
        let state_guard = StateMutationGuard::acquire_for(
            state_root,
            Some(&service.config().skill_storage_path),
            "install global skills",
        )
        .map_err(CliError::Service)?;
        if std::fs::read(&lock_path).map_err(CliError::Io)? != original_lock {
            state_guard.recovered().map_err(CliError::Service)?;
            return Err(CliError::Config(
                "global-skills.lock changed after planning; retry the command".to_string(),
            ));
        }
        if let Err(error) =
            validate_unmodified_installed(&lock, &changed_ids, &service.config().skill_storage_path)
        {
            state_guard.recovered().map_err(CliError::Service)?;
            return Err(error);
        }
        let ids = changed_ids.iter().cloned().collect::<Vec<_>>();
        let lifecycle = match LifecycleTransaction::capture_with_files(
            state_root,
            &service.config().skill_storage_path,
            &ids,
            std::slice::from_ref(&lock_path),
        ) {
            Ok(lifecycle) => lifecycle,
            Err(error) => {
                state_guard.recovered().map_err(CliError::Service)?;
                return Err(CliError::Service(error));
            }
        };
        let previous = match capture_registry(&service, &ids).await {
            Ok(previous) => previous,
            Err(error) => {
                lifecycle.commit();
                state_guard.recovered().map_err(CliError::Service)?;
                return Err(error);
            }
        };
        plan.candidates.sort_by(|left, right| {
            right
                .depth
                .cmp(&left.depth)
                .then_with(|| left.prepared.id().cmp(right.prepared.id()))
        });
        #[cfg(test)]
        let mut applied = 0_usize;
        for candidate in plan.candidates {
            if !changed_ids.contains(candidate.prepared.id()) {
                continue;
            }
            let result = service
                .apply_prepared_content(candidate.prepared, AddMode::Update, candidate.groups)
                .await;
            if let Err(error) = result {
                return recover_global(
                    &service,
                    lifecycle,
                    state_guard,
                    &previous,
                    error.to_string(),
                )
                .await;
            }
            #[cfg(test)]
            {
                applied += 1;
                if applied == 1
                    && FAIL_AFTER_FIRST_GLOBAL_APPLY
                        .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    return recover_global(
                        &service,
                        lifecycle,
                        state_guard,
                        &previous,
                        "injected failure after first global apply".to_string(),
                    )
                    .await;
                }
            }
        }
        lifecycle.commit();
        state_guard.commit().map_err(CliError::Service)?;
    }
    let changed = targets.iter().any(|target| target.outcome == "changed");
    if args.json {
        print_install_json(InstallJsonResult {
            scope: "global",
            outcome: if changed { "changed" } else { "unchanged" }.to_string(),
            dry_run: args.dry_run,
            targets,
            diagnostics: Vec::new(),
        })?;
    } else if !changed {
        crate::outln!("Global lock is already satisfied; no changes");
    } else {
        let changed_count = targets
            .iter()
            .filter(|target| target.outcome == "changed")
            .count();
        crate::outln!(
            "{} global skill(s) {} from global-skills.lock",
            changed_count,
            if args.dry_run {
                "would be restored"
            } else {
                "restored"
            }
        );
    }
    Ok(())
}

fn validate_options(args: &InstallArgs) -> CliResult<()> {
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    if args.offline && args.reindex {
        return Err(CliError::Validation(
            "--offline and --reindex cannot be used together".to_string(),
        ));
    }
    if args.only.is_some() && args.without.is_some() {
        return Err(CliError::Validation(
            "--only and --without cannot be used together".to_string(),
        ));
    }
    if args.depth.is_some_and(|depth| depth < 1) {
        return Err(CliError::InvalidDepth(
            "Depth must be at least 1".to_string(),
        ));
    }
    Ok(())
}

fn selected_roots(lock: &GlobalSkillsLock, args: &InstallArgs) -> CliResult<Vec<ResolutionRoot>> {
    let mut known_groups = BTreeSet::from(["default".to_string()]);
    known_groups.extend(
        lock.skills
            .iter()
            .flat_map(|entry| entry.groups.iter().cloned()),
    );
    for requested in args
        .only
        .iter()
        .flatten()
        .chain(args.without.iter().flatten())
    {
        if !known_groups.contains(requested) {
            return Err(CliError::Validation(format!(
                "Unknown dependency group '{requested}'"
            )));
        }
    }
    Ok(lock
        .skills
        .iter()
        .filter(|entry| lock.covered_roots.contains(&entry.id))
        .filter(|entry| {
            let groups = effective_groups(&entry.groups);
            args.without
                .as_ref()
                .is_none_or(|without| !groups.iter().any(|group| without.contains(group)))
        })
        .filter(|entry| {
            let groups = effective_groups(&entry.groups);
            args.only.as_ref().is_none_or(|only| {
                !only.is_empty() && groups.iter().any(|group| only.contains(group))
            })
        })
        .map(|entry| ResolutionRoot {
            origin: entry.origin.clone(),
            expected_id: Some(entry.id.clone()),
            groups: entry.groups.clone(),
            locked: Some(entry.resolved.clone()),
        })
        .collect())
}

fn effective_groups(groups: &[String]) -> Vec<String> {
    if groups.is_empty() {
        vec!["default".to_string()]
    } else {
        groups.to_vec()
    }
}

fn installed_matches(path: &Path, entry: &GlobalLockedSkillEntry) -> bool {
    match &entry.origin {
        Origin::Local {
            path: source,
            editable: true,
        } => path
            .canonicalize()
            .ok()
            .zip(source.canonicalize().ok())
            .is_some_and(|(installed, source)| installed == source),
        _ => entry.resolved.checksum.as_ref().is_some_and(|expected| {
            fastskill_core::core::install::content_digest(path)
                .is_ok_and(|actual| &actual == expected)
        }),
    }
}

fn validate_unmodified_installed(
    lock: &GlobalSkillsLock,
    changed: &BTreeSet<String>,
    storage: &Path,
) -> CliResult<()> {
    for entry in lock
        .skills
        .iter()
        .filter(|entry| changed.contains(&entry.id))
    {
        let installed = storage.join(&entry.id);
        if !installed.exists() && !installed.is_symlink() {
            continue;
        }
        if matches!(entry.origin, Origin::Local { editable: true, .. }) {
            continue;
        }
        if entry.resolved.checksum.is_some() && !installed_matches(&installed, entry) {
            return Err(CliError::Config(format!(
                "installed global skill '{}' was modified; restore or remove local edits before changing managed state",
                entry.id
            )));
        }
    }
    Ok(())
}

async fn capture_registry(
    service: &FastSkillService,
    ids: &[String],
) -> CliResult<Vec<(SkillId, Option<SkillDefinition>)>> {
    let mut previous = Vec::with_capacity(ids.len());
    for id in ids {
        let skill_id = SkillId::new(id.clone()).map_err(CliError::Service)?;
        let definition = service
            .skill_manager()
            .get_skill(&skill_id)
            .await
            .map_err(CliError::Service)?;
        previous.push((skill_id, definition));
    }
    Ok(previous)
}

async fn restore_registry(
    service: &FastSkillService,
    previous: &[(SkillId, Option<SkillDefinition>)],
) -> Result<(), String> {
    let mut failures = Vec::new();
    for (id, definition) in previous {
        match service.skill_manager().get_skill(id).await {
            Ok(Some(_)) => {
                if let Err(error) = service.skill_manager().unregister_skill(id).await {
                    failures.push(format!("unregister {id}: {error}"));
                }
            }
            Ok(None) => {}
            Err(error) => failures.push(format!("inspect registry {id}: {error}")),
        }
        if let Some(definition) = definition {
            if let Err(error) = service
                .skill_manager()
                .force_register_skill(definition.clone())
                .await
            {
                failures.push(format!("restore registry {id}: {error}"));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

async fn recover_global(
    service: &FastSkillService,
    lifecycle: LifecycleTransaction,
    state_guard: StateMutationGuard,
    previous: &[(SkillId, Option<SkillDefinition>)],
    original_error: String,
) -> CliResult<()> {
    if let Err(error) = lifecycle.rollback() {
        drop(state_guard);
        return Err(CliError::Config(format!(
            "{original_error}; global install recovery failed: {error}"
        )));
    }
    if let Err(error) = restore_registry(service, previous).await {
        drop(state_guard);
        return Err(CliError::Config(format!(
            "{original_error}; global registry recovery failed: {error}"
        )));
    }
    state_guard.recovered().map_err(CliError::Service)?;
    Err(CliError::Config(original_error))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fastskill_core::core::lock::GlobalLockedSkillEntry;
    use fastskill_core::core::origin::{Origin, Resolved};
    use tempfile::TempDir;

    struct EnvGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(name: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn args() -> InstallArgs {
        InstallArgs {
            without: None,
            only: None,
            lock: false,
            depth: None,
            offline: false,
            dry_run: false,
            json: false,
            reindex: false,
            no_reindex: false,
        }
    }

    fn entry(id: &str, dependencies: &[&str], groups: &[&str]) -> GlobalLockedSkillEntry {
        GlobalLockedSkillEntry {
            id: id.to_string(),
            name: id.to_string(),
            origin: Origin::Local {
                path: id.into(),
                editable: false,
            },
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum: Some("digest".to_string()),
            },
            dependencies: dependencies
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            groups: groups.iter().map(|value| (*value).to_string()).collect(),
            installed_at: Utc::now(),
            last_checked_at: None,
            last_updated_at: None,
        }
    }

    #[test]
    fn selects_direct_roots_and_applies_group_filters() {
        let mut lock = GlobalSkillsLock::new_empty();
        lock.skills = vec![
            entry("root", &["child"], &["dev"]),
            entry("child", &[], &[]),
        ];
        lock.covered_roots = vec!["root".to_string(), "child".to_string()];
        let selected = selected_roots(&lock, &args()).unwrap();
        assert_eq!(
            selected.len(),
            2,
            "a directly selected dependency remains a root"
        );
        lock.covered_roots = vec!["root".to_string()];
        let selected = selected_roots(&lock, &args()).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].expected_id.as_deref(), Some("root"));

        let mut only = args();
        only.only = Some(vec!["dev".to_string()]);
        assert_eq!(selected_roots(&lock, &only).unwrap().len(), 1);
        only.only = Some(vec!["prod".to_string()]);
        assert!(selected_roots(&lock, &only).is_err());

        lock.covered_roots = vec!["child".to_string()];
        only.only = Some(vec!["default".to_string()]);
        assert_eq!(selected_roots(&lock, &only).unwrap().len(), 1);
    }

    #[test]
    fn rejects_conflicting_global_install_options() {
        let mut value = args();
        value.only = Some(vec!["dev".to_string()]);
        value.without = Some(vec!["test".to_string()]);
        assert!(validate_options(&value).is_err());
        value.without = None;
        value.depth = Some(0);
        assert!(validate_options(&value).is_err());

        let mut value = args();
        value.reindex = true;
        value.no_reindex = true;
        assert!(validate_options(&value).is_err());
        value.no_reindex = false;
        value.offline = true;
        assert!(validate_options(&value).is_err());
    }

    #[test]
    fn filters_without_empty_only_and_validates_installed_content() {
        let mut lock = GlobalSkillsLock::new_empty();
        lock.skills = vec![entry("dev", &[], &["dev"]), entry("plain", &[], &[])];
        lock.covered_roots = vec!["dev".to_string(), "plain".to_string()];
        let mut filtered = args();
        filtered.without = Some(vec!["dev".to_string()]);
        assert_eq!(selected_roots(&lock, &filtered).unwrap().len(), 1);
        filtered.without = None;
        filtered.only = Some(Vec::new());
        assert!(selected_roots(&lock, &filtered).unwrap().is_empty());

        let storage = TempDir::new().unwrap();
        let installed = storage.path().join("dev");
        std::fs::create_dir(&installed).unwrap();
        std::fs::write(installed.join("SKILL.md"), "edited").unwrap();
        let changed = BTreeSet::from(["dev".to_string()]);
        assert!(validate_unmodified_installed(&lock, &changed, storage.path()).is_err());
        lock.skills[0].origin = Origin::Local {
            path: storage.path().join("source"),
            editable: true,
        };
        validate_unmodified_installed(&lock, &changed, storage.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn editable_install_matches_only_the_recorded_source_symlink() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let other = temp.path().join("other");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let installed = temp.path().join("installed");
        symlink(&source, &installed).unwrap();

        let mut value = entry("editable", &[], &[]);
        value.origin = Origin::Local {
            path: source,
            editable: true,
        };
        assert!(installed_matches(&installed, &value));

        std::fs::remove_file(&installed).unwrap();
        symlink(other, &installed).unwrap();
        assert!(!installed_matches(&installed, &value));
    }

    #[tokio::test]
    async fn missing_malformed_and_empty_global_locks_are_reported() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("config");
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &config);
        let _cache = EnvGuard::set("FASTSKILL_CACHE_DIR", &temp.path().join("cache"));
        let global = config.join("fastskill");
        std::fs::create_dir_all(&global).unwrap();
        let lock_path = global.join("global-skills.lock");

        assert!(execute_global_install(args())
            .await
            .unwrap_err()
            .to_string()
            .contains("global-skills.lock not found"));
        std::fs::write(&lock_path, "invalid = [lock").unwrap();
        assert!(execute_global_install(args())
            .await
            .unwrap_err()
            .to_string()
            .contains("Failed to load global lock"));

        GlobalSkillsLock::new_empty()
            .save_to_file(&lock_path)
            .unwrap();
        let mut json = args();
        json.json = true;
        json.dry_run = true;
        let (result, output) = crate::output::capture(execute_global_install(json)).await;
        result.unwrap();
        let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(value["outcome"], "unchanged");
        assert_eq!(value["diagnostics"].as_array().unwrap().len(), 1);

        let (result, output) = crate::output::capture(execute_global_install(args())).await;
        result.unwrap();
        assert!(output.contains("No global skills matched"));
    }

    #[tokio::test]
    async fn failure_after_first_candidate_restores_global_state() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("config");
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &config);
        let _cache = EnvGuard::set("FASTSKILL_CACHE_DIR", &temp.path().join("cache"));
        let global = config.join("fastskill");
        std::fs::create_dir_all(&global).unwrap();
        let mut lock = GlobalSkillsLock::new_empty();
        for id in ["first", "second"] {
            let source = temp.path().join(id);
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("SKILL.md"),
                format!("---\nname: {id}\nversion: 1.0.0\ndescription: fixture\n---\n{id}\n"),
            )
            .unwrap();
            let mut value = entry(id, &[], &[]);
            value.origin = Origin::Local {
                path: source.clone(),
                editable: false,
            };
            value.resolved.checksum =
                Some(fastskill_core::core::install::content_digest(&source).unwrap());
            lock.skills.push(value);
            lock.covered_roots.push(id.to_string());
        }
        let lock_path = global.join("global-skills.lock");
        lock.save_to_file(&lock_path).unwrap();
        let before = std::fs::read(&lock_path).unwrap();
        FAIL_AFTER_FIRST_GLOBAL_APPLY.store(true, std::sync::atomic::Ordering::SeqCst);

        let error = execute_global_install(args()).await.unwrap_err();

        assert!(error.to_string().contains("injected failure"));
        assert_eq!(std::fs::read(&lock_path).unwrap(), before);
        assert!(!global.join("skills/first").exists());
        assert!(!global.join("skills/second").exists());
        assert!(!global.join(".fastskill/recovery-required").exists());
    }

    #[tokio::test]
    async fn global_preview_apply_and_noop_report_truthful_outcomes() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("config");
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &config);
        let _cache = EnvGuard::set("FASTSKILL_CACHE_DIR", &temp.path().join("cache"));
        let global = config.join("fastskill");
        std::fs::create_dir_all(&global).unwrap();
        let source = temp.path().join("demo");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\nversion: 1.0.0\ndescription: fixture\n---\nBody\n",
        )
        .unwrap();
        let mut lock = GlobalSkillsLock::new_empty();
        let mut demo = entry("demo", &[], &[]);
        demo.origin = Origin::Local {
            path: source.clone(),
            editable: false,
        };
        demo.resolved.checksum =
            Some(fastskill_core::core::install::content_digest(&source).unwrap());
        lock.skills.push(demo);
        lock.covered_roots.push("demo".to_string());
        lock.save_to_file(&global.join("global-skills.lock"))
            .unwrap();

        let mut preview = args();
        preview.dry_run = true;
        preview.json = true;
        let (result, output) = crate::output::capture(execute_global_install(preview)).await;
        result.unwrap();
        let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(value["outcome"], "changed");
        assert!(!global.join("skills/demo").exists());

        let (result, output) = crate::output::capture(execute_global_install(args())).await;
        result.unwrap();
        assert!(output.contains("restored"));
        assert!(global.join("skills/demo/SKILL.md").exists());

        let (result, output) = crate::output::capture(execute_global_install(args())).await;
        result.unwrap();
        assert!(output.contains("already satisfied"));
    }
}

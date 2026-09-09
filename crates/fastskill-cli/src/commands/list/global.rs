use super::{format_source_info, origin_location_label, origin_type_label, ListArgs};
use crate::error::{CliError, CliResult};
use fastskill_core::core::lock::{global_lock_path, GlobalSkillsLock};
use fastskill_core::core::project_removal::managed_tree_digest;
use fastskill_core::core::Origin;
use fastskill_core::{FastSkillService, OutputFormat};
use std::collections::{BTreeSet, HashMap, HashSet};

use super::output::ListRow;

pub(super) async fn execute_global_list(
    service: &FastSkillService,
    args: ListArgs,
    format: OutputFormat,
) -> CliResult<()> {
    let lock_path = global_lock_path()
        .map_err(|error| CliError::Config(format!("Failed to resolve global lock: {error}")))?;
    let lock = if lock_path.exists() {
        GlobalSkillsLock::load_from_file(&lock_path)
            .map_err(|error| CliError::Config(format!("Failed to load global lock: {error}")))?
    } else {
        GlobalSkillsLock::new_empty()
    };
    let known_groups = lock
        .covered_roots
        .iter()
        .filter_map(|id| lock.skills.iter().find(|entry| entry.id == *id))
        .flat_map(|entry| entry_groups(&entry.groups))
        .collect::<HashSet<_>>();
    for requested in args.only.iter().chain(args.without.iter()).flatten() {
        if !known_groups.contains(requested) {
            return Err(CliError::Validation(format!("Unknown group '{requested}'")));
        }
    }
    let installed = service
        .skill_manager()
        .list_skills()
        .await
        .map_err(CliError::Service)?
        .into_iter()
        .map(|skill| (skill.id.to_string(), skill))
        .collect::<HashMap<_, _>>();
    let ids = lock
        .skills
        .iter()
        .map(|entry| entry.id.clone())
        .chain(installed.keys().cloned())
        .collect::<HashSet<_>>();
    let selected_roots = lock
        .covered_roots
        .iter()
        .filter(|id| {
            lock.skills
                .iter()
                .find(|entry| entry.id == id.as_str())
                .is_some_and(|entry| {
                    let groups = entry_groups(&entry.groups);
                    args.only
                        .as_ref()
                        .is_none_or(|only| groups.iter().any(|group| only.contains(group)))
                        && args.without.as_ref().is_none_or(|without| {
                            !groups.iter().any(|group| without.contains(group))
                        })
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let selected_ids = closure(&lock, &selected_roots);
    let mut failures = Vec::new();
    let mut rows = Vec::new();
    for id in ids {
        let locked = lock.skills.iter().find(|entry| entry.id == id);
        let actual = installed.get(&id);
        let selected = selected_ids.contains(&id);
        let mutable = locked
            .is_some_and(|entry| matches!(entry.origin, Origin::Local { editable: true, .. }));
        let reconciliation = if locked.is_some() && !selected {
            "excluded"
        } else {
            reconcile(service, &id, locked, actual, mutable)
        };
        if args.check && selected && !matches!(reconciliation, "ok" | "extraneous") {
            failures.push(format!("{id}: {reconciliation}"));
        }
        let (source_path, source_type) = actual
            .map(|skill| {
                (
                    Some(skill.skill_file.display().to_string()),
                    Some(origin_type_label(&skill.origin).to_string()),
                )
            })
            .or_else(|| locked.map(|entry| format_source_info(&entry.origin)))
            .unwrap_or((None, None));
        rows.push(ListRow {
            id: id.clone(),
            name: actual
                .map(|skill| skill.name.clone())
                .or_else(|| locked.map(|entry| entry.name.clone()))
                .unwrap_or_else(|| id.clone()),
            description: actual
                .map(|skill| skill.description.clone())
                .unwrap_or_else(|| "-".to_string()),
            version: actual
                .map(|skill| skill.version.clone())
                .or_else(|| locked.map(|entry| entry.resolved.version.clone())),
            in_manifest: false,
            in_lock: locked.is_some(),
            installed: actual.is_some(),
            source_path,
            source_type,
            missing_from_folder: locked.is_some() && actual.is_none(),
            missing_from_lock: locked.is_none() && actual.is_some(),
            missing_from_manifest: false,
            desired_constraint: locked.map(|entry| origin_location_label(&entry.origin)),
            locked_version: locked.map(|entry| entry.resolved.version.clone()),
            actual_version: actual.map(|skill| skill.version.clone()),
            reconciliation: reconciliation.to_string(),
            owners: locked.map_or_else(Vec::new, |_| global_owners(&lock, &id)),
            groups: locked.map(|entry| entry.groups.clone()).unwrap_or_default(),
            mutable,
            override_active: false,
            extraneous: locked.is_none() && actual.is_some(),
        });
    }
    rows.sort_by(|left, right| left.id.cmp(&right.id));
    crate::outln!(
        "{}",
        super::output::format_list_results(&rows, format, args.details)
            .map_err(CliError::Config)?
    );
    if !failures.is_empty() {
        return Err(CliError::Config(format!(
            "Global state needs reconciliation: {}",
            failures.join("; ")
        )));
    }
    Ok(())
}

fn closure(lock: &GlobalSkillsLock, roots: &[String]) -> BTreeSet<String> {
    let mut selected: BTreeSet<_> = roots.iter().cloned().collect();
    loop {
        let before = selected.len();
        for entry in &lock.skills {
            if selected.contains(&entry.id) {
                selected.extend(entry.dependencies.iter().cloned());
            }
        }
        if selected.len() == before {
            return selected;
        }
    }
}

fn global_owners(lock: &GlobalSkillsLock, id: &str) -> Vec<String> {
    let mut owners = BTreeSet::new();
    if lock.covered_roots.iter().any(|root| root == id) {
        owners.insert("direct:global".to_string());
    }
    for root in &lock.covered_roots {
        if root != id && closure(lock, std::slice::from_ref(root)).contains(id) {
            owners.insert(format!("global-root:{root}"));
        }
    }
    for parent in &lock.skills {
        if parent
            .dependencies
            .iter()
            .any(|dependency| dependency == id)
        {
            owners.insert(format!("required-by:{}", parent.id));
        }
    }
    owners.into_iter().collect()
}

fn entry_groups(groups: &[String]) -> Vec<String> {
    if groups.is_empty() {
        vec!["default".to_string()]
    } else {
        groups.to_vec()
    }
}

fn reconcile(
    service: &FastSkillService,
    id: &str,
    locked: Option<&fastskill_core::core::lock::GlobalLockedSkillEntry>,
    actual: Option<&fastskill_core::SkillDefinition>,
    mutable: bool,
) -> &'static str {
    match (locked, actual) {
        (Some(_), None) => "missing-content",
        (Some(locked), Some(actual)) if locked.resolved.version != actual.version => {
            "revision-mismatch"
        }
        (Some(_), Some(_)) if mutable => "ok",
        (Some(locked), Some(_)) => match &locked.resolved.checksum {
            Some(expected) => {
                match managed_tree_digest(&service.config().skill_storage_path.join(id)) {
                    Ok(actual) if &actual == expected => "ok",
                    Ok(_) => "content-mismatch",
                    Err(_) => "integrity-error",
                }
            }
            None => "insufficient-integrity",
        },
        (None, Some(_)) => "extraneous",
        (None, None) => "ok",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fastskill_core::core::lock::GlobalLockedSkillEntry;
    use fastskill_core::core::origin::Resolved;
    use fastskill_core::{ServiceConfig, SkillDefinition, SkillId};
    use std::path::Path;
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

    fn entry(id: &str, version: &str, checksum: Option<String>) -> GlobalLockedSkillEntry {
        GlobalLockedSkillEntry {
            id: id.to_string(),
            name: id.to_string(),
            origin: Origin::Local {
                path: id.into(),
                editable: false,
            },
            resolved: Resolved {
                version: version.to_string(),
                commit_hash: None,
                checksum,
            },
            dependencies: Vec::new(),
            groups: Vec::new(),
            installed_at: Utc::now(),
            last_checked_at: None,
            last_updated_at: None,
        }
    }

    async fn service(storage: &Path) -> FastSkillService {
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: storage.to_path_buf(),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();
        service
    }

    #[tokio::test]
    async fn reconciliation_covers_integrity_revision_mutability_and_extraneous_state() {
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("skills");
        let installed = storage.join("demo");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(
            installed.join("SKILL.md"),
            "---\nname: demo\nversion: 1.0.0\n---\n# demo\n",
        )
        .unwrap();
        let service = service(&storage).await;
        let mut actual = SkillDefinition::new(
            SkillId::new("demo".to_string()).unwrap(),
            "demo".to_string(),
            String::new(),
            "1.0.0".to_string(),
            Origin::Local {
                path: installed.clone(),
                editable: false,
            },
        );
        actual.skill_file = installed.join("SKILL.md");

        assert_eq!(
            reconcile(
                &service,
                "demo",
                Some(&entry("demo", "1.0.0", None)),
                None,
                false
            ),
            "missing-content"
        );
        assert_eq!(
            reconcile(
                &service,
                "demo",
                Some(&entry("demo", "2.0.0", None)),
                Some(&actual),
                false
            ),
            "revision-mismatch"
        );
        assert_eq!(
            reconcile(
                &service,
                "demo",
                Some(&entry("demo", "1.0.0", None)),
                Some(&actual),
                true
            ),
            "ok"
        );
        let digest = managed_tree_digest(&installed).unwrap();
        assert_eq!(
            reconcile(
                &service,
                "demo",
                Some(&entry("demo", "1.0.0", Some(digest))),
                Some(&actual),
                false
            ),
            "ok"
        );
        assert_eq!(
            reconcile(
                &service,
                "demo",
                Some(&entry("demo", "1.0.0", Some("wrong".to_string()))),
                Some(&actual),
                false
            ),
            "content-mismatch"
        );
        assert_eq!(
            reconcile(
                &service,
                "absent",
                Some(&entry("absent", "1.0.0", Some("wrong".to_string()))),
                Some(&actual),
                false
            ),
            "integrity-error"
        );
        assert_eq!(
            reconcile(
                &service,
                "demo",
                Some(&entry("demo", "1.0.0", None)),
                Some(&actual),
                false
            ),
            "insufficient-integrity"
        );
        assert_eq!(
            reconcile(&service, "demo", None, Some(&actual), false),
            "extraneous"
        );
        assert_eq!(reconcile(&service, "demo", None, None, false), "ok");
    }

    #[tokio::test]
    async fn global_list_handles_empty_state_unknown_groups_and_check_failures() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &temp.path().join("config"));
        let storage = temp.path().join("skills");
        let service = service(&storage).await;
        let args = ListArgs {
            format: None,
            json: false,
            details: false,
            bundles: false,
            check: false,
            only: None,
            without: None,
            skills_dir: None,
        };
        execute_global_list(&service, args.clone(), OutputFormat::Json)
            .await
            .unwrap();

        let path = global_lock_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut lock = GlobalSkillsLock::new_empty();
        lock.covered_roots.push("demo".to_string());
        lock.skills
            .push(entry("demo", "1.0.0", Some("digest".to_string())));
        lock.save_to_file(&path).unwrap();
        let mut unknown = args.clone();
        unknown.only = Some(vec!["missing".to_string()]);
        assert!(matches!(
            execute_global_list(&service, unknown, OutputFormat::Table).await,
            Err(CliError::Validation(message)) if message.contains("Unknown group")
        ));
        let mut check = args;
        check.check = true;
        assert!(matches!(
            execute_global_list(&service, check, OutputFormat::Json).await,
            Err(CliError::Config(message)) if message.contains("missing-content")
        ));
    }
}

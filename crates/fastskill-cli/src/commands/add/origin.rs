use super::{sources, AddArgs};
use crate::error::{CliError, CliResult};
use crate::utils::SkillSource;
use fastskill_core::core::install::PreparedSkill;
use fastskill_core::core::lock::{global_lock_path, GlobalLockedSkillEntry, GlobalSkillsLock};
use fastskill_core::core::origin::{GitRef, Origin, Resolved};
use fastskill_core::core::repository::RepositoryManager;
use fastskill_core::core::resolution::{
    prepare_resolution, prepare_resolution_preview, ResolutionRoot,
};
use fastskill_core::core::state_guard::StateMutationGuard;
use fastskill_core::core::version::VersionConstraint;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

struct GlobalAddPlan {
    id: String,
    current_revision: Option<String>,
    target_revision: String,
    origin: Origin,
    resolved: Resolved,
    groups: Vec<String>,
    dependencies: Vec<String>,
    changes: Vec<String>,
    candidate: Option<PreparedSkill>,
}

pub(super) async fn add_global_skill(
    service: &FastSkillService,
    source: &SkillSource,
    args: &AddArgs,
) -> CliResult<()> {
    let origin = build_origin(service, source, args).await?;
    add_global_origins(service, vec![origin], args).await
}

pub(super) async fn add_global_origins(
    service: &FastSkillService,
    origins: Vec<Origin>,
    args: &AddArgs,
) -> CliResult<()> {
    let groups = args
        .group
        .clone()
        .map(|group| vec![group])
        .unwrap_or_default();
    let lock_path = global_lock_path().map_err(|error| {
        CliError::Config(format!("Failed to resolve global lock path: {error}"))
    })?;
    let original_lock = if lock_path.exists() {
        Some(fs::read(&lock_path).map_err(CliError::Io)?)
    } else {
        None
    };
    let mut lock = if lock_path.exists() {
        GlobalSkillsLock::load_from_file(&lock_path)
            .map_err(|error| CliError::Config(format!("Failed to load global lock: {error}")))?
    } else {
        GlobalSkillsLock::new_empty()
    };
    let locked = lock
        .skills
        .iter()
        .map(|entry| (entry.id.clone(), entry.resolved.clone()))
        .collect::<HashMap<_, _>>();
    let roots = origins
        .iter()
        .cloned()
        .map(|origin| ResolutionRoot {
            expected_id: match &origin {
                Origin::Repository { skill, .. } => Some(skill.clone()),
                _ => None,
            },
            origin,
            groups: groups.clone(),
            locked: None,
        })
        .collect();
    let resolution = if args.dry_run && !args.offline {
        prepare_resolution_preview(service, roots, &locked, u32::MAX).await
    } else {
        prepare_resolution(service, roots, &locked, u32::MAX, args.offline).await
    }
    .map_err(CliError::Service)?;
    let root_ids = resolution.root_ids.clone();
    if root_ids.is_empty() {
        return Err(CliError::Config(
            "Global add planning produced no root".to_string(),
        ));
    }
    for candidate in &resolution.candidates {
        crate::commands::update::global::validate_retained_global_candidate(
            &lock,
            &root_ids,
            candidate.prepared.id(),
            &candidate.origin,
            candidate.prepared.resolved(),
        )?;
    }
    for (root_id, origin) in root_ids.iter().zip(&origins) {
        if let Some(existing) = lock.skills.iter().find(|entry| entry.id == *root_id) {
            if !args.force && (!lock.covered_roots.contains(root_id) || existing.origin != *origin)
            {
                return Err(CliError::Validation(format!(
                    "Global skill '{root_id}' already has a different owner or origin; use --force to replace direct intent"
                )));
            }
        }
    }
    let refreshed_repositories = resolution.refreshed_repositories.clone();
    let mut plans = resolution
        .candidates
        .into_iter()
        .map(|candidate| {
            let id = candidate.prepared.id().to_string();
            let existing = lock.skills.iter().find(|entry| entry.id == id);
            let mut changes = Vec::new();
            if existing.is_none_or(|entry| entry.origin != candidate.origin) {
                changes.push("origin changed".to_string());
            }
            if existing.is_none_or(|entry| entry.resolved != *candidate.prepared.resolved()) {
                changes.push("resolved content changed".to_string());
            }
            if existing.is_none_or(|entry| entry.dependencies != candidate.dependencies) {
                changes.push("dependency ownership changed".to_string());
            }
            if existing.is_none_or(|entry| sorted(&entry.groups) != sorted(&candidate.groups)) {
                changes.push("groups changed".to_string());
            }
            GlobalAddPlan {
                id,
                current_revision: existing.map(|entry| entry.resolved.version.clone()),
                target_revision: candidate.prepared.resolved().version.clone(),
                origin: candidate.origin,
                resolved: candidate.prepared.resolved().clone(),
                groups: candidate.groups,
                dependencies: candidate.dependencies,
                changes,
                candidate: Some(candidate.prepared),
            }
        })
        .collect::<Vec<_>>();
    plans.sort_by(|left, right| left.id.cmp(&right.id));
    let content_changes = plans
        .iter()
        .filter(|plan| {
            plan.changes
                .iter()
                .any(|change| change.as_str() != "groups changed")
        })
        .map(|plan| plan.id.clone())
        .collect();
    crate::commands::update::global::validate_global_replacement_content(
        &lock,
        &content_changes,
        &service.config().skill_storage_path,
    )?;
    if args.dry_run {
        if args.json {
            let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
                service,
                "add",
                args.reindex,
                true,
                crate::config_file::load_auto_reindex_config(),
            )
            .await;
            emit_global_add(&plans, true, &refreshed_repositories, Some(&indexing))?;
        } else if plans.iter().all(|plan| plan.changes.is_empty()) {
            crate::outln!("Selected global skills are already satisfied; no changes were applied");
        } else {
            for plan in &plans {
                crate::outln!(
                    "  • {}: {} -> {} ({})",
                    plan.id,
                    plan.current_revision.as_deref().unwrap_or("not installed"),
                    plan.target_revision,
                    if plan.changes.is_empty() {
                        "unchanged"
                    } else {
                        "change"
                    }
                );
            }
            crate::outln!("No changes were applied");
        }
        return Ok(());
    }

    let state_root = lock_path
        .parent()
        .ok_or_else(|| CliError::Config("Global lock has no state directory".to_string()))?;
    let backup = TempDir::new().map_err(CliError::Io)?;
    let mut registry_snapshots = Vec::new();
    for plan in &plans {
        let id = fastskill_core::SkillId::new(plan.id.clone()).map_err(CliError::Service)?;
        let existing = service
            .skill_manager()
            .get_skill(&id)
            .await
            .map_err(CliError::Service)?;
        registry_snapshots.push((id, existing));
    }
    let guard = StateMutationGuard::acquire_for(
        state_root,
        Some(&service.config().skill_storage_path),
        "global add",
    )
    .map_err(CliError::Service)?;
    let current_lock = match fs::read(&lock_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            guard.recovered().map_err(CliError::Service)?;
            return Err(CliError::Io(error));
        }
    };
    if current_lock != original_lock {
        guard.recovered().map_err(CliError::Service)?;
        return Err(CliError::Config(
            "Global state changed while add was being planned; retry the command".to_string(),
        ));
    }
    if let Err(error) = crate::commands::update::global::validate_global_replacement_content(
        &lock,
        &content_changes,
        &service.config().skill_storage_path,
    ) {
        guard.recovered().map_err(CliError::Service)?;
        return Err(error);
    }
    let snapshots = match crate::commands::update::global::capture_directories(
        &service.config().skill_storage_path,
        plans.iter().map(|plan| plan.id.clone()),
        backup.path(),
    )
    .await
    {
        Ok(snapshots) => snapshots,
        Err(error) => {
            guard.recovered().map_err(CliError::Service)?;
            return Err(error);
        }
    };
    let apply_result = apply_global_plans(service, &mut lock, &mut plans).await;
    for root_id in &root_ids {
        if !lock.covered_roots.contains(root_id) {
            lock.covered_roots.push(root_id.clone());
        }
    }
    if let Err(error) = apply_result.and_then(|()| {
        lock.save_to_file(&lock_path)
            .map_err(|save| CliError::Config(format!("Failed to save global lock: {save}")))
    }) {
        restore_global_add(
            service,
            &lock_path,
            original_lock.as_deref(),
            &snapshots,
            registry_snapshots,
        )
        .await
        .map_err(|recovery| {
            CliError::Config(format!("{error}; global add recovery failed: {recovery}"))
        })?;
        guard.recovered().map_err(CliError::Service)?;
        return Err(error);
    }
    guard.commit().map_err(CliError::Service)?;

    if args.json {
        let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
            service,
            "add",
            args.reindex,
            args.no_reindex || args.offline,
            crate::config_file::load_auto_reindex_config(),
        )
        .await;
        emit_global_add(&plans, false, &refreshed_repositories, Some(&indexing))?;
    } else {
        if !args.offline {
            crate::utils::reindex_utils::maybe_auto_reindex(
                service,
                "add",
                args.reindex,
                args.no_reindex,
                crate::config_file::load_auto_reindex_config(),
                false,
            )
            .await?;
        }
        crate::outln!(
            "Successfully added global skill(s): {}",
            root_ids.join(", ")
        );
    }
    Ok(())
}

async fn apply_global_plans(
    service: &FastSkillService,
    lock: &mut GlobalSkillsLock,
    plans: &mut [GlobalAddPlan],
) -> CliResult<()> {
    for plan in plans {
        if plan.changes.is_empty() {
            continue;
        }
        if plan
            .changes
            .iter()
            .any(|change| change.as_str() != "groups changed")
        {
            let candidate = plan.candidate.take().ok_or_else(|| {
                CliError::Config(format!(
                    "Global add plan for '{}' was already applied",
                    plan.id
                ))
            })?;
            service
                .apply_prepared_content(
                    candidate,
                    fastskill_core::core::AddMode::Update,
                    plan.groups.clone(),
                )
                .await
                .map_err(CliError::Service)?;
        }
        let now = chrono::Utc::now();
        if let Some(entry) = lock.skills.iter_mut().find(|entry| entry.id == plan.id) {
            entry.origin = plan.origin.clone();
            entry.resolved = plan.resolved.clone();
            entry.dependencies.clone_from(&plan.dependencies);
            entry.groups.clone_from(&plan.groups);
            entry.last_updated_at = Some(now);
        } else {
            lock.skills.push(GlobalLockedSkillEntry {
                id: plan.id.clone(),
                name: plan.id.clone(),
                origin: plan.origin.clone(),
                resolved: plan.resolved.clone(),
                dependencies: plan.dependencies.clone(),
                groups: plan.groups.clone(),
                installed_at: now,
                last_checked_at: None,
                last_updated_at: Some(now),
            });
        }
    }
    Ok(())
}

fn sorted(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
}

async fn restore_global_add(
    service: &FastSkillService,
    lock_path: &std::path::Path,
    original_lock: Option<&[u8]>,
    snapshots: &[crate::commands::update::global::DirectorySnapshot],
    registry_snapshots: Vec<(
        fastskill_core::SkillId,
        Option<fastskill_core::SkillDefinition>,
    )>,
) -> CliResult<()> {
    crate::commands::update::global::restore_directories(snapshots).await?;
    match original_lock {
        Some(bytes) => fs::write(lock_path, bytes).map_err(CliError::Io)?,
        None => match fs::remove_file(lock_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        },
    }
    for (id, existing) in registry_snapshots {
        service
            .skill_manager()
            .unregister_skill(&id)
            .await
            .map_err(CliError::Service)?;
        if let Some(existing) = existing {
            service
                .skill_manager()
                .force_register_skill(existing)
                .await
                .map_err(CliError::Service)?;
        }
    }
    Ok(())
}

fn emit_global_add(
    plans: &[GlobalAddPlan],
    dry_run: bool,
    refreshed_repositories: &[String],
    indexing: Option<&crate::utils::reindex_utils::LifecycleIndexResult>,
) -> CliResult<()> {
    let changed = plans.iter().any(|plan| !plan.changes.is_empty());
    let targets = plans
        .iter()
        .map(|plan| {
            serde_json::json!({
                "id": plan.id,
                "outcome": if plan.changes.is_empty() { "unchanged" } else { "changed" },
                "current_revision": plan.current_revision,
                "target_revision": plan.target_revision,
                "changes": plan.changes,
                "retained": Vec::<String>::new()
            })
        })
        .collect::<Vec<_>>();
    crate::outln!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scope": "global",
            "outcome": if changed { "changed" } else { "unchanged" },
            "dry_run": dry_run,
            "targets": targets,
            "diagnostics": Vec::<String>::new(),
            "resolution": {
                "source": if refreshed_repositories.is_empty() { "cached" } else { "refreshed" },
                "refreshed_repositories": refreshed_repositories
            },
            "indexing": indexing
        }))
        .map_err(|error| CliError::Config(format!(
            "Failed to serialize global add result: {error}"
        )))?
    );
    Ok(())
}

/// Build install intent through the core inference seam, then apply explicit
/// command-line selectors that are outside the portable source reference.
pub(super) async fn build_origin(
    service: &FastSkillService,
    source: &SkillSource,
    args: &AddArgs,
) -> CliResult<Origin> {
    if args.source_type.is_some() {
        return build_explicit_origin(source, args);
    }
    let mut origin = service.infer_origin(&args.source).await?;
    if let Some(repository) = &args.repository {
        match &mut origin {
            Origin::Repository { repo, .. } => {
                let manager = service.repository_manager().ok_or_else(|| {
                    CliError::Config("No repositories are configured for this project".to_string())
                })?;
                if manager.get_repository(repository).is_none() {
                    return Err(CliError::Config(format!(
                        "Repository '{repository}' is not configured"
                    )));
                }
                *repo = repository.clone();
            }
            _ => {
                return Err(CliError::Validation(
                    "--repository is only valid when SOURCE is a skill ID".to_string(),
                ));
            }
        }
    }
    match &mut origin {
        Origin::Local { path, editable } => {
            *path = path.canonicalize().map_err(|error| {
                CliError::InvalidSource(format!(
                    "Failed to resolve absolute path for '{}': {error}",
                    path.display()
                ))
            })?;
            *editable = args.editable;
        }
        Origin::Git { r#ref, .. } => {
            if let Some(branch) = &args.branch {
                *r#ref = GitRef::Branch(branch.clone());
            } else if matches!(r#ref, GitRef::Default) {
                if let Some(tag) = &args.tag {
                    *r#ref = GitRef::Tag(tag.clone());
                }
            }
        }
        Origin::ZipUrl { .. } | Origin::Repository { .. } => {}
    }
    Ok(origin)
}

fn build_explicit_origin(source: &SkillSource, args: &AddArgs) -> CliResult<Origin> {
    match source {
        SkillSource::ZipFile(path) | SkillSource::Folder(path) => {
            let path = path.canonicalize().map_err(|error| {
                CliError::InvalidSource(format!(
                    "Failed to resolve absolute path for '{}': {error}",
                    path.display()
                ))
            })?;
            Ok(Origin::Local {
                path,
                editable: matches!(source, SkillSource::Folder(_)) && args.editable,
            })
        }
        SkillSource::GitUrl(url) => {
            let parsed = crate::utils::parse_git_url(url)?;
            let branch = args.branch.clone().or(parsed.branch);
            let r#ref = match (branch, &args.tag) {
                (Some(branch), _) => GitRef::Branch(branch),
                (None, Some(tag)) => GitRef::Tag(tag.clone()),
                (None, None) => GitRef::Default,
            };
            Ok(Origin::Git {
                url: url.clone(),
                r#ref,
                subdir: parsed.subdir,
            })
        }
        SkillSource::RemoteZipUrl(url) => Ok(Origin::ZipUrl { url: url.clone() }),
        SkillSource::SkillId(reference) => {
            let (skill, _, _, version) = sources::parse_registry_scope_id(reference)?;
            let version = version
                .as_deref()
                .map(VersionConstraint::parse)
                .transpose()
                .map_err(|error| {
                    CliError::Config(format!("Invalid version constraint: {error}"))
                })?;
            let manager = RepositoryManager::from_definitions(
                crate::config::load_repositories_from_project()?,
            );
            let repository = if let Some(repository) = &args.repository {
                manager.get_repository(repository).ok_or_else(|| {
                    CliError::Config(format!("Repository '{repository}' is not configured"))
                })?
            } else {
                manager.get_default_repository().ok_or_else(|| {
                    CliError::Config(
                        "No default repository configured. Use 'fastskill repo add' to add a repository."
                            .to_string(),
                    )
                })?
            };
            Ok(Origin::Repository {
                repo: repository.name.clone(),
                skill,
                version,
            })
        }
    }
}

#[cfg(test)]
#[path = "origin/coverage_tests.rs"]
mod coverage_tests;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use fastskill_core::ServiceConfig;
    use std::path::{Path, PathBuf};

    pub(super) struct EnvGuard {
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

    pub(super) fn write_skill(path: &Path, id: &str, version: &str, body: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {id}\nversion: {version}\ndescription: fixture\n---\n{body}\n"),
        )
        .unwrap();
    }

    pub(super) async fn global_fixture() -> (TempDir, EnvGuard, EnvGuard, FastSkillService, PathBuf)
    {
        let root = TempDir::new().unwrap();
        let config = root.path().join("config");
        let cache = root.path().join("cache");
        let xdg = EnvGuard::set("XDG_CONFIG_HOME", &config);
        let cache_guard = EnvGuard::set("FASTSKILL_CACHE_DIR", &cache);
        let storage = config.join("fastskill/skills");
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: storage.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();
        (root, xdg, cache_guard, service, storage)
    }

    pub(super) fn args() -> AddArgs {
        AddArgs {
            source: "fixture".to_string(),
            source_type: Some("local".to_string()),
            repository: None,
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: true,
            offline: false,
            dry_run: false,
            json: false,
        }
    }

    #[test]
    fn explicit_local_git_and_zip_sources_map_to_canonical_origins() {
        let temp = TempDir::new().unwrap();
        let folder = temp.path().join("skill");
        fs::create_dir_all(&folder).unwrap();
        let zip = temp.path().join("skill.zip");
        fs::write(&zip, "fixture").unwrap();

        let mut local_args = args();
        local_args.editable = true;
        assert!(matches!(
            build_explicit_origin(&SkillSource::Folder(folder.clone()), &local_args).unwrap(),
            Origin::Local { path, editable: true } if path == folder.canonicalize().unwrap()
        ));
        assert!(matches!(
            build_explicit_origin(&SkillSource::ZipFile(zip.clone()), &local_args).unwrap(),
            Origin::Local { path, editable: false } if path == zip.canonicalize().unwrap()
        ));
        assert!(build_explicit_origin(
            &SkillSource::Folder(temp.path().join("missing")),
            &local_args,
        )
        .is_err());

        let mut git_args = args();
        git_args.branch = Some("main".to_string());
        let git = build_explicit_origin(
            &SkillSource::GitUrl("https://example.com/org/repo.git".to_string()),
            &git_args,
        )
        .unwrap();
        assert!(
            matches!(git, Origin::Git { r#ref: GitRef::Branch(branch), .. } if branch == "main")
        );
        git_args.branch = None;
        git_args.tag = Some("v1".to_string());
        assert!(matches!(
            build_explicit_origin(
                &SkillSource::GitUrl("https://example.com/org/repo.git".to_string()),
                &git_args,
            )
            .unwrap(),
            Origin::Git { r#ref: GitRef::Tag(tag), .. } if tag == "v1"
        ));
        git_args.tag = None;
        assert!(matches!(
            build_explicit_origin(
                &SkillSource::GitUrl("https://example.com/org/repo.git".to_string()),
                &git_args,
            )
            .unwrap(),
            Origin::Git {
                r#ref: GitRef::Default,
                ..
            }
        ));
        assert_eq!(
            build_explicit_origin(
                &SkillSource::RemoteZipUrl("https://example.com/skill.zip".to_string()),
                &args(),
            )
            .unwrap(),
            Origin::ZipUrl {
                url: "https://example.com/skill.zip".to_string()
            }
        );
    }

    #[test]
    fn explicit_repository_reference_requires_a_configured_default() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let original = std::env::current_dir().unwrap();
        struct Restore(std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _restore = Restore(original);
        std::env::set_current_dir(temp.path()).unwrap();
        fs::write(temp.path().join("skill-project.toml"), "[dependencies]\n").unwrap();
        assert!(
            build_explicit_origin(&SkillSource::SkillId("demo@^1".to_string()), &args(),).is_err()
        );
        assert!(build_explicit_origin(
            &SkillSource::SkillId("demo@not-semver".to_string()),
            &args(),
        )
        .is_err());
    }

    #[tokio::test]
    async fn inferred_local_origin_honors_editability_and_rejects_repository_override() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("demo");
        write_skill(&source, "demo", "1.0.0", "demo");
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: temp.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();
        let mut inferred = args();
        inferred.source_type = None;
        inferred.source = source.display().to_string();
        inferred.editable = true;
        let source_kind = SkillSource::Folder(source.clone());
        assert!(matches!(
            build_origin(&service, &source_kind, &inferred).await.unwrap(),
            Origin::Local { path, editable: true } if path == source.canonicalize().unwrap()
        ));
        inferred.repository = Some("community".to_string());
        assert!(matches!(
            build_origin(&service, &source_kind, &inferred).await,
            Err(CliError::Validation(message)) if message.contains("only valid when SOURCE is a skill ID")
        ));
    }

    #[tokio::test]
    async fn global_add_human_preview_apply_unchanged_and_conflict_use_one_plan() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (root, _xdg, _cache, service, storage) = global_fixture().await;
        let source = root.path().join("source");
        write_skill(&source, "demo", "1.0.0", "first");
        let origin = Origin::Local {
            path: source.clone(),
            editable: false,
        };
        let mut preview = args();
        preview.source_type = None;
        preview.dry_run = true;
        add_global_origins(&service, Vec::new(), &preview)
            .await
            .unwrap_err();
        add_global_origins(&service, vec![origin.clone()], &preview)
            .await
            .unwrap();
        assert!(!global_lock_path().unwrap().exists());

        let mut apply = preview.clone();
        apply.dry_run = false;
        add_global_origins(&service, vec![origin.clone()], &apply)
            .await
            .unwrap();
        assert!(storage.join("demo/SKILL.md").exists());
        add_global_origins(&service, vec![origin], &preview)
            .await
            .unwrap();

        let replacement = root.path().join("replacement");
        write_skill(&replacement, "demo", "2.0.0", "second");
        let replacement = Origin::Local {
            path: replacement,
            editable: false,
        };
        assert!(matches!(
            add_global_origins(&service, vec![replacement.clone()], &preview).await,
            Err(CliError::Validation(message)) if message.contains("different owner or origin")
        ));
        let mut forced = preview;
        forced.force = true;
        forced.json = true;
        add_global_origins(&service, vec![replacement], &forced)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn global_add_restore_reinstates_lock_directory_and_registry_state() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (root, _xdg, _cache, service, storage) = global_fixture().await;
        write_skill(&storage.join("demo"), "demo", "1.0.0", "before");
        let id = fastskill_core::SkillId::new("demo".to_string()).unwrap();
        let mut definition = fastskill_core::SkillDefinition::new(
            id.clone(),
            "demo".to_string(),
            "fixture".to_string(),
            "1.0.0".to_string(),
            Origin::Local {
                path: storage.join("demo"),
                editable: false,
            },
        );
        definition.skill_file = storage.join("demo/SKILL.md");
        service
            .skill_manager()
            .force_register_skill(definition.clone())
            .await
            .unwrap();
        let registered = Some(definition);
        let backup = TempDir::new().unwrap();
        let snapshots = crate::commands::update::global::capture_directories(
            &storage,
            ["demo".to_string()].into_iter(),
            backup.path(),
        )
        .await
        .unwrap();
        fs::write(storage.join("demo/SKILL.md"), "changed").unwrap();
        let lock_path = root.path().join("config/fastskill/global-skills.lock");
        fs::write(&lock_path, "changed lock").unwrap();
        restore_global_add(
            &service,
            &lock_path,
            Some(b"original lock"),
            &snapshots,
            vec![(id.clone(), registered)],
        )
        .await
        .unwrap();
        assert_eq!(fs::read_to_string(&lock_path).unwrap(), "original lock");
        assert!(fs::read_to_string(storage.join("demo/SKILL.md"))
            .unwrap()
            .contains("before"));
        assert!(service
            .skill_manager()
            .get_skill(&id)
            .await
            .unwrap()
            .is_some());

        restore_global_add(&service, &lock_path, None, &[], Vec::new())
            .await
            .unwrap();
        assert!(!lock_path.exists());
    }

    #[test]
    fn global_add_json_distinguishes_changed_and_unchanged_targets() {
        let unchanged = GlobalAddPlan {
            id: "alpha".to_string(),
            current_revision: Some("1.0.0".to_string()),
            target_revision: "1.0.0".to_string(),
            origin: Origin::Local {
                path: "alpha".into(),
                editable: false,
            },
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum: None,
            },
            groups: Vec::new(),
            dependencies: Vec::new(),
            changes: Vec::new(),
            candidate: None,
        };
        let mut changed = GlobalAddPlan {
            id: "beta".to_string(),
            ..unchanged
        };
        changed.changes.push("origin changed".to_string());
        emit_global_add(&[changed], true, &["community".to_string()], None).unwrap();
    }
}

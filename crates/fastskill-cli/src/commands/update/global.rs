use super::{controlled_origin, UpdateArgs};
use crate::commands::add::copy_dir_recursive;
use crate::config::create_service_config;
use crate::error::{CliError, CliResult};
use crate::utils::messages;
use fastskill_core::core::install::PreparedSkill;
use fastskill_core::core::lock::{global_lock_path, GlobalLockedSkillEntry, GlobalSkillsLock};
use fastskill_core::core::project_removal::managed_tree_digest;
use fastskill_core::core::resolution::{
    prepare_resolution, prepare_resolution_preview, ResolutionRoot,
};
use fastskill_core::core::state_guard::StateMutationGuard;
use fastskill_core::core::{AddMode, Origin};
use fastskill_core::FastSkillService;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[cfg(test)]
static FAIL_APPLY_ID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
#[cfg(test)]
static FAIL_LOCK_SAVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static FAIL_ROLLBACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static CHANGE_LOCK_BEFORE_COMMIT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) struct DirectorySnapshot {
    installed: PathBuf,
    original: OriginalPath,
}

enum OriginalPath {
    Missing,
    Directory(PathBuf),
    Symlink(PathBuf),
}

struct PlannedGlobalUpdate {
    id: String,
    current_revision: Option<String>,
    target_revision: String,
    origin: Origin,
    groups: Vec<String>,
    dependencies: Vec<String>,
    candidate: Option<PreparedSkill>,
    changes: Vec<String>,
    remove: bool,
    blocked: bool,
}

struct GlobalApplySummary {
    updated_count: usize,
    pruned_count: usize,
    failures: Vec<String>,
}

fn direct_root_ids(lock: &GlobalSkillsLock) -> Vec<String> {
    lock.covered_roots.clone()
}

fn selected_closure(lock: &GlobalSkillsLock, roots: &[String]) -> BTreeSet<String> {
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

fn obsolete_dependencies(
    lock: &GlobalSkillsLock,
    selected_roots: &[String],
    candidate_ids: &BTreeSet<String>,
) -> Vec<String> {
    let previous_selected = selected_closure(lock, selected_roots);
    let other_roots = lock
        .covered_roots
        .iter()
        .filter(|root| !selected_roots.contains(root))
        .cloned()
        .collect::<Vec<_>>();
    let retained_by_other_roots = selected_closure(lock, &other_roots);
    previous_selected
        .difference(candidate_ids)
        .filter(|id| !retained_by_other_roots.contains(*id))
        .cloned()
        .collect()
}

fn emit_global_result(
    plans: &[PlannedGlobalUpdate],
    dry_run: bool,
    failures: &[String],
    indexing: Option<&crate::utils::reindex_utils::LifecycleIndexResult>,
) -> CliResult<()> {
    let changed = plans.iter().any(|plan| !plan.changes.is_empty());
    let outcome = if failures.is_empty() {
        if changed {
            "changed"
        } else {
            "unchanged"
        }
    } else if changed {
        "partial"
    } else {
        "failed"
    };
    let targets = plans
        .iter()
        .map(|plan| {
            let failed = failures
                .iter()
                .any(|failure| failure.starts_with(&format!("{}:", plan.id)));
            serde_json::json!({
                "id": plan.id,
                "outcome": if failed {
                    "failed"
                } else if plan.blocked {
                    "blocked"
                } else if plan.changes.is_empty() {
                    "unchanged"
                } else {
                    "changed"
                },
                "current_revision": plan.current_revision,
                "target_revision": plan.target_revision,
                "changes": plan.changes,
                "retained": if plan.blocked {
                    vec!["retained because another target failed"]
                } else {
                    Vec::<&str>::new()
                }
            })
        })
        .collect::<Vec<_>>();
    crate::outln!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "scope": "global",
            "outcome": outcome,
            "dry_run": dry_run,
            "targets": targets,
            "diagnostics": failures,
            "indexing": indexing
        }))
        .map_err(|error| CliError::Config(format!(
            "Failed to serialize global update result: {error}"
        )))?
    );
    Ok(())
}

pub(crate) async fn capture_directories(
    storage: &Path,
    ids: impl Iterator<Item = String>,
    backup_root: &Path,
) -> CliResult<Vec<DirectorySnapshot>> {
    let mut snapshots = Vec::new();
    for (index, id) in ids.enumerate() {
        let installed = storage.join(id);
        let original = if installed
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            OriginalPath::Symlink(fs::read_link(&installed).map_err(CliError::Io)?)
        } else if installed.exists() {
            let backup = backup_root.join(index.to_string());
            copy_dir_recursive(&installed, &backup).await?;
            OriginalPath::Directory(backup)
        } else {
            OriginalPath::Missing
        };
        snapshots.push(DirectorySnapshot {
            installed,
            original,
        });
    }
    Ok(snapshots)
}

pub(crate) async fn restore_directories(snapshots: &[DirectorySnapshot]) -> CliResult<()> {
    for snapshot in snapshots {
        if let Ok(metadata) = snapshot.installed.symlink_metadata() {
            if metadata.file_type().is_symlink() {
                fastskill_core::core::lifecycle_transaction::unlink_symlink(
                    &snapshot.installed,
                    &metadata,
                )
                .map_err(CliError::Service)?;
            } else if metadata.is_file() {
                fs::remove_file(&snapshot.installed).map_err(CliError::Io)?;
            } else {
                fs::remove_dir_all(&snapshot.installed).map_err(CliError::Io)?;
            }
        }
        match &snapshot.original {
            OriginalPath::Missing => {}
            OriginalPath::Directory(backup) => {
                copy_dir_recursive(backup, &snapshot.installed).await?;
            }
            OriginalPath::Symlink(target) => create_directory_symlink(target, &snapshot.installed)?,
        }
    }
    Ok(())
}

async fn rollback_global_update(
    service: &FastSkillService,
    snapshots: &[DirectorySnapshot],
    lock_path: &Path,
    original_lock: &[u8],
    registry_snapshots: &[(
        fastskill_core::SkillId,
        Option<fastskill_core::SkillDefinition>,
    )],
) -> CliResult<()> {
    #[cfg(test)]
    if FAIL_ROLLBACK.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CliError::Config(
            "injected global update rollback failure".to_string(),
        ));
    }
    restore_directories(snapshots).await?;
    fs::write(lock_path, original_lock).map_err(CliError::Io)?;
    for (id, existing) in registry_snapshots {
        match service.skill_manager().unregister_skill(id).await {
            Ok(()) | Err(fastskill_core::ServiceError::SkillNotFound(_)) => {}
            Err(error) => return Err(CliError::Service(error)),
        }
        if let Some(existing) = existing {
            service
                .skill_manager()
                .force_register_skill(existing.clone())
                .await
                .map_err(CliError::Service)?;
        }
    }
    Ok(())
}

fn remove_global_path(path: &Path) -> CliResult<()> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    if metadata.file_type().is_symlink() {
        fastskill_core::core::lifecycle_transaction::unlink_symlink(path, &metadata)
            .map_err(CliError::Service)
    } else if metadata.is_file() {
        fs::remove_file(path).map_err(CliError::Io)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(CliError::Io)
    } else {
        Err(CliError::Config(format!(
            "Unsupported global skill destination: {}",
            path.display()
        )))
    }
}

async fn apply_global_plan(
    service: &FastSkillService,
    lock_path: &Path,
    lock: &mut GlobalSkillsLock,
    prepared: &mut [PlannedGlobalUpdate],
    pruned_ids: &[String],
    json: bool,
) -> CliResult<GlobalApplySummary> {
    let mut updated_count = 0usize;
    let mut failures = Vec::new();
    for plan in prepared.iter_mut() {
        if plan.changes.is_empty() || plan.remove {
            continue;
        }
        #[cfg(test)]
        if FAIL_APPLY_ID
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_deref()
            == Some(&plan.id)
        {
            failures.push(format!("{}: injected apply failure", plan.id));
            continue;
        }
        let candidate = plan.candidate.take().ok_or_else(|| {
            CliError::Config(format!(
                "Global update plan for '{}' was already applied",
                plan.id
            ))
        })?;
        match service
            .apply_prepared_content(candidate, AddMode::Update, plan.groups.clone())
            .await
        {
            Ok(outcome) => {
                let now = chrono::Utc::now();
                if let Some(entry) = lock.skills.iter_mut().find(|entry| entry.id == plan.id) {
                    entry.origin = plan.origin.clone();
                    entry.resolved = outcome.resolved;
                    entry.dependencies.clone_from(&plan.dependencies);
                    entry.groups.clone_from(&plan.groups);
                    entry.last_updated_at = Some(now);
                } else {
                    lock.skills.push(GlobalLockedSkillEntry {
                        id: plan.id.clone(),
                        name: plan.id.clone(),
                        origin: plan.origin.clone(),
                        resolved: outcome.resolved,
                        dependencies: plan.dependencies.clone(),
                        groups: plan.groups.clone(),
                        installed_at: now,
                        last_checked_at: None,
                        last_updated_at: Some(now),
                    });
                }
                updated_count += 1;
                if !json {
                    crate::outln!("  {}", messages::ok(&format!("Updated {}", plan.id)));
                }
            }
            Err(error) => failures.push(format!("{}: {error}", plan.id)),
        }
    }

    let mut pruned_count = 0usize;
    if failures.is_empty() {
        for id in pruned_ids {
            remove_global_path(&service.config().skill_storage_path.join(id))?;
            let skill_id = fastskill_core::SkillId::new(id.clone()).map_err(CliError::Service)?;
            service
                .skill_manager()
                .unregister_skill(&skill_id)
                .await
                .map_err(CliError::Service)?;
            lock.skills.retain(|entry| entry.id != *id);
            pruned_count += 1;
            if !json {
                crate::outln!("  {}", messages::ok(&format!("Removed obsolete {id}")));
            }
        }
    } else {
        for plan in prepared.iter_mut().filter(|plan| plan.remove) {
            plan.blocked = true;
            plan.changes.clear();
            if !json {
                crate::outln!(
                    "  {}",
                    messages::warning(&format!(
                        "Retained {} because another target failed",
                        plan.id
                    ))
                );
            }
        }
    }

    if updated_count > 0 || pruned_count > 0 {
        #[cfg(test)]
        if FAIL_LOCK_SAVE.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(CliError::Config(
                "injected global lock save failure".to_string(),
            ));
        }
        lock.save_to_file(lock_path)
            .map_err(|error| CliError::Config(format!("Failed to save global lock: {error}")))?;
    }
    Ok(GlobalApplySummary {
        updated_count,
        pruned_count,
        failures,
    })
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) -> CliResult<()> {
    std::os::unix::fs::symlink(target, link).map_err(CliError::Io)
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) -> CliResult<()> {
    std::os::windows::fs::symlink_dir(target, link).map_err(CliError::Io)
}

pub(super) async fn execute_update_global(
    args: UpdateArgs,
    skills_dir_override: Option<PathBuf>,
) -> CliResult<()> {
    if !args.json {
        crate::outln!("Updating global skills...");
        crate::outln!();
    }

    let lock_path = global_lock_path().map_err(|error| {
        CliError::Config(format!("Failed to resolve global lock path: {error}"))
    })?;
    if !lock_path.exists() {
        if args.json {
            emit_global_result(&[], args.check || args.dry_run, &[], None)?;
        } else {
            crate::outln!(
                "{}",
                messages::info(
                    "No global-skills.lock found. Run 'fastskill add --global <skill>' first."
                )
            );
        }
        return Ok(());
    }
    let original_lock = fs::read(&lock_path).map_err(CliError::Io)?;
    let mut lock = GlobalSkillsLock::load_from_file(&lock_path)
        .map_err(|error| CliError::Config(format!("Failed to load global lock file: {error}")))?;
    let roots = direct_root_ids(&lock);
    let selected_roots = if let Some(id) = &args.skill_id {
        if !roots.contains(id) {
            return Err(CliError::Validation(format!(
                "Global skill '{id}' is not a directly selected root"
            )));
        }
        vec![id.clone()]
    } else {
        roots
    };
    if selected_roots.is_empty() {
        if args.json {
            emit_global_result(&[], args.check || args.dry_run, &[], None)?;
        } else {
            crate::outln!("{}", messages::info("No global skills to update"));
        }
        return Ok(());
    }

    let config = create_service_config(true, skills_dir_override)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;
    let service = crate::config::inject_edge_services(service)?;
    let mut failures = Vec::new();
    for id in selected_closure(&lock, &selected_roots) {
        let Some(entry) = lock.skills.iter().find(|entry| entry.id == id) else {
            failures.push(format!("{id}: dependency is absent from global lock"));
            continue;
        };
        let installed = service.config().skill_storage_path.join(&entry.id);
        let editable = matches!(entry.origin, Origin::Local { editable: true, .. });
        if !installed.exists() {
            failures.push(format!("{}: installed content is missing", entry.id));
            continue;
        }
        if !editable {
            let Some(expected) = &entry.resolved.checksum else {
                failures.push(format!(
                    "{}: lock has insufficient integrity evidence; restore it before updating",
                    entry.id
                ));
                continue;
            };
            match managed_tree_digest(&installed) {
                Ok(actual) if &actual == expected => {}
                Ok(_) => {
                    failures.push(format!(
                        "{}: installed content was modified; restore it before updating",
                        entry.id
                    ));
                    continue;
                }
                Err(error) => {
                    failures.push(format!(
                        "{}: cannot verify installed content: {error}",
                        entry.id
                    ));
                    continue;
                }
            }
        }
    }
    if !failures.is_empty() {
        if args.json {
            emit_global_result(&[], args.check || args.dry_run, &failures, None)?;
        }
        return Err(CliError::Config(format!(
            "Global update planning failed: {}",
            failures.join("; ")
        )));
    }

    let locked = lock
        .skills
        .iter()
        .map(|entry| (entry.id.clone(), entry.resolved.clone()))
        .collect::<HashMap<_, _>>();
    let resolution_roots = selected_roots
        .iter()
        .map(|id| {
            let entry = lock
                .skills
                .iter()
                .find(|entry| entry.id == *id)
                .ok_or_else(|| {
                    CliError::Config(format!("Selected global root '{id}' disappeared from Lock"))
                })?;
            let origin = controlled_origin(&entry.origin, &entry.resolved.version, &args).map_err(
                |error| CliError::Validation(format!("Cannot update '{}': {error}", entry.id)),
            )?;
            Ok(ResolutionRoot {
                origin,
                expected_id: Some(entry.id.clone()),
                groups: entry.groups.clone(),
                locked: Some(entry.resolved.clone()),
            })
        })
        .collect::<CliResult<Vec<_>>>()?;
    let resolution = if (args.check || args.dry_run) && !args.offline {
        prepare_resolution_preview(&service, resolution_roots, &locked, u32::MAX).await
    } else {
        prepare_resolution(&service, resolution_roots, &locked, u32::MAX, args.offline).await
    }
    .map_err(CliError::Service)?;
    let candidate_ids = resolution
        .candidates
        .iter()
        .map(|candidate| candidate.prepared.id().to_string())
        .collect::<BTreeSet<_>>();
    let pruned_ids = obsolete_dependencies(&lock, &selected_roots, &candidate_ids);
    let mut prepared = resolution
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
            PlannedGlobalUpdate {
                id,
                current_revision: existing.map(|entry| entry.resolved.version.clone()),
                target_revision: candidate.prepared.resolved().version.clone(),
                origin: candidate.origin,
                groups: candidate.groups,
                dependencies: candidate.dependencies,
                candidate: Some(candidate.prepared),
                changes,
                remove: false,
                blocked: false,
            }
        })
        .collect::<Vec<_>>();
    prepared.extend(pruned_ids.iter().filter_map(|id| {
        lock.skills
            .iter()
            .find(|entry| entry.id == *id)
            .map(|entry| PlannedGlobalUpdate {
                id: id.clone(),
                current_revision: Some(entry.resolved.version.clone()),
                target_revision: "removed".to_string(),
                origin: entry.origin.clone(),
                groups: entry.groups.clone(),
                dependencies: Vec::new(),
                candidate: None,
                changes: vec!["no longer reachable from a selected global root".to_string()],
                remove: true,
                blocked: false,
            })
    }));
    prepared.sort_by(|left, right| left.id.cmp(&right.id));
    if !args.json {
        for plan in &prepared {
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
    }
    if args.check || args.dry_run {
        if args.json {
            let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
                &service,
                "update",
                args.reindex,
                true,
                crate::config_file::load_auto_reindex_config(),
            )
            .await;
            emit_global_result(&prepared, true, &[], Some(&indexing))?;
        } else {
            crate::outln!("{}", messages::info("No changes were applied"));
        }
        return Ok(());
    }

    let state_root = lock_path
        .parent()
        .ok_or_else(|| CliError::Config("Global lock has no state directory".to_string()))?;
    let backup = TempDir::new().map_err(CliError::Io)?;
    let mut registry_snapshots = Vec::new();
    for plan in &prepared {
        if !plan.changes.is_empty() {
            let id = fastskill_core::SkillId::new(plan.id.clone()).map_err(CliError::Service)?;
            let existing = service
                .skill_manager()
                .get_skill(&id)
                .await
                .map_err(CliError::Service)?;
            registry_snapshots.push((id, existing));
        }
    }
    let guard = StateMutationGuard::acquire_for(
        state_root,
        Some(&service.config().skill_storage_path),
        "global update",
    )
    .map_err(CliError::Service)?;
    #[cfg(test)]
    if CHANGE_LOCK_BEFORE_COMMIT.swap(false, std::sync::atomic::Ordering::SeqCst) {
        fs::write(&lock_path, b"concurrent writer").map_err(CliError::Io)?;
    }
    let current_lock = match fs::read(&lock_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            guard.recovered().map_err(CliError::Service)?;
            return Err(CliError::Io(error));
        }
    };
    if current_lock != original_lock {
        guard.recovered().map_err(CliError::Service)?;
        return Err(CliError::Config(
            "Global state changed while the update was being planned; retry the command"
                .to_string(),
        ));
    }
    let snapshots = match capture_directories(
        &service.config().skill_storage_path,
        prepared
            .iter()
            .filter(|plan| !plan.changes.is_empty())
            .map(|plan| plan.id.clone()),
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

    let summary = match apply_global_plan(
        &service,
        &lock_path,
        &mut lock,
        &mut prepared,
        &pruned_ids,
        args.json,
    )
    .await
    {
        Ok(summary) => summary,
        Err(error) => {
            if let Err(restore_error) = rollback_global_update(
                &service,
                &snapshots,
                &lock_path,
                &original_lock,
                &registry_snapshots,
            )
            .await
            {
                let recovery_path = backup.keep();
                return Err(CliError::Config(format!(
                    "{error}; global update rollback also failed: {restore_error}; recovery inputs retained at {}",
                    recovery_path.display()
                )));
            }
            guard.recovered().map_err(CliError::Service)?;
            return Err(error);
        }
    };
    guard.commit().map_err(CliError::Service)?;

    let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
        &service,
        "update",
        args.reindex,
        args.no_reindex || args.offline,
        crate::config_file::load_auto_reindex_config(),
    )
    .await;
    if args.json {
        emit_global_result(&prepared, false, &summary.failures, Some(&indexing))?;
    } else {
        crate::outln!();
        crate::outln!(
            "{}",
            messages::ok(&format!(
                "Updated {} global skill(s)",
                summary.updated_count
            ))
        );
        if summary.pruned_count > 0 {
            crate::outln!(
                "   Removed {} obsolete dependency skill(s)",
                summary.pruned_count
            );
        }
        if summary.updated_count > 0 {
            crate::outln!("   Updated global-skills.lock");
        }
        match indexing.outcome {
            "succeeded" => crate::outln!("   Indexed {} skill(s)", indexing.count),
            "failed" => eprintln!(
                "{}",
                messages::warning(
                    indexing
                        .diagnostic
                        .as_deref()
                        .unwrap_or("automatic indexing failed")
                )
            ),
            _ => {}
        }
    }
    if !summary.failures.is_empty() {
        return Err(CliError::Config(format!(
            "Global update partially failed: {}",
            summary.failures.join("; ")
        )));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
#[path = "global_tests.rs"]
mod tests;

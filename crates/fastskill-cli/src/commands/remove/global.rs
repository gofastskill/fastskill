use crate::error::{CliError, CliResult};
use fastskill_core::core::global_ownership::{plan_removal, GlobalRemovalPlan};
use fastskill_core::core::lifecycle_transaction::LifecycleTransaction;
use fastskill_core::core::lock::{global_lock_path, GlobalSkillsLock};
use fastskill_core::core::origin::Origin;
use fastskill_core::core::skill_manager::SkillDefinition;
use fastskill_core::core::state_guard::StateMutationGuard;
use fastskill_core::{FastSkillService, SkillId};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
static CHANGE_LOCK_AFTER_PREVIEW: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static FAIL_AFTER_LOCK_SAVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static FAIL_REGISTRY_CAPTURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) fn preview(
    service: &FastSkillService,
    requested: &[String],
) -> CliResult<GlobalRemovalPlan> {
    let (path, bytes) = load_lock()?;
    let lock = lock_from_bytes(&bytes, &path)?;
    let plan = plan_removal(&lock, requested).map_err(CliError::Service)?;
    validate_unmodified(service, &lock, &plan.delete_files)?;
    Ok(plan)
}

pub(super) async fn remove(
    service: &FastSkillService,
    requested: &[String],
) -> CliResult<GlobalRemovalPlan> {
    let preview = preview(service, requested)?;
    if preview.remove_roots.is_empty() && preview.remove_lock_entries.is_empty() {
        return Ok(preview);
    }
    let (lock_path, original_lock) = load_lock()?;
    #[cfg(test)]
    if CHANGE_LOCK_AFTER_PREVIEW.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let mut changed = original_lock.clone();
        changed.extend_from_slice(b"\n# concurrent writer\n");
        fs::write(&lock_path, changed).map_err(CliError::Io)?;
    }
    let state_root = lock_path.parent().ok_or_else(|| {
        CliError::Config("global-skills.lock has no parent directory".to_string())
    })?;
    let storage = &service.config().skill_storage_path;
    let guard = StateMutationGuard::acquire_for(state_root, Some(storage), "remove global skills")
        .map_err(CliError::Service)?;
    let refreshed = (|| {
        let current_bytes = fs::read(&lock_path).map_err(CliError::Io)?;
        let lock = lock_from_bytes(&current_bytes, &lock_path)?;
        let current = plan_removal(&lock, requested).map_err(CliError::Service)?;
        Ok::<_, CliError>((current_bytes, lock, current))
    })();
    let (current_bytes, mut lock, current) = match refreshed {
        Ok(value) => value,
        Err(error) => {
            guard.recovered().map_err(CliError::Service)?;
            return Err(error);
        }
    };
    if current != preview || current_bytes != original_lock {
        guard.recovered().map_err(CliError::Service)?;
        return Err(CliError::Config(
            "global-skills.lock changed after removal planning; retry".to_string(),
        ));
    }
    if let Err(error) = validate_unmodified(service, &lock, &current.delete_files) {
        guard.recovered().map_err(CliError::Service)?;
        return Err(error);
    }
    let lifecycle = match LifecycleTransaction::capture_with_files(
        state_root,
        storage,
        &current.delete_files,
        std::slice::from_ref(&lock_path),
    ) {
        Ok(value) => value,
        Err(error) => {
            guard.recovered().map_err(CliError::Service)?;
            return Err(CliError::Service(error));
        }
    };
    let previous = match capture_registry(service, &current.delete_files).await {
        Ok(value) => value,
        Err(error) => {
            lifecycle.commit();
            guard.recovered().map_err(CliError::Service)?;
            return Err(error);
        }
    };
    let result = apply(service, &lock_path, &mut lock, &current, &previous).await;
    if let Err(error) = result {
        if let Err(recovery) = lifecycle.rollback() {
            drop(guard);
            return Err(CliError::Config(format!(
                "{error}; global removal recovery failed: {recovery}"
            )));
        }
        if let Err(recovery) = restore_registry(service, &previous).await {
            drop(guard);
            return Err(CliError::Config(format!(
                "{error}; global registry recovery failed: {recovery}"
            )));
        }
        guard.recovered().map_err(CliError::Service)?;
        return Err(error);
    }
    lifecycle.commit();
    guard.commit().map_err(CliError::Service)?;
    Ok(current)
}

fn load_lock() -> CliResult<(PathBuf, Vec<u8>)> {
    let path = global_lock_path()
        .map_err(|error| CliError::Config(format!("Failed to resolve global lock: {error}")))?;
    if !path.exists() {
        return Ok((path, Vec::new()));
    }
    let bytes = fs::read(&path).map_err(CliError::Io)?;
    Ok((path, bytes))
}

fn lock_from_bytes(bytes: &[u8], path: &Path) -> CliResult<GlobalSkillsLock> {
    if bytes.is_empty() {
        Ok(GlobalSkillsLock::new_empty())
    } else {
        GlobalSkillsLock::load_from_file(path)
            .map_err(|error| CliError::Config(format!("Failed to load global lock: {error}")))
    }
}

fn validate_unmodified(
    service: &FastSkillService,
    lock: &GlobalSkillsLock,
    ids: &[String],
) -> CliResult<()> {
    for id in ids {
        let Some(entry) = lock.skills.iter().find(|entry| entry.id == *id) else {
            continue;
        };
        let path = service.config().skill_storage_path.join(id);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        if matches!(entry.origin, Origin::Local { editable: true, .. }) {
            if metadata.file_type().is_symlink() {
                continue;
            }
            return Err(CliError::Config(format!(
                "editable global skill '{id}' is not installed as a link"
            )));
        }
        let expected = entry.resolved.checksum.as_ref().ok_or_else(|| {
            CliError::Config(format!(
                "global skill '{id}' has no locked content digest; restore it before removal"
            ))
        })?;
        let actual =
            fastskill_core::core::install::content_digest(&path).map_err(CliError::Service)?;
        if &actual != expected {
            return Err(CliError::Config(format!(
                "global skill '{id}' is locally modified; FastSkill will not delete it"
            )));
        }
    }
    Ok(())
}

async fn apply(
    service: &FastSkillService,
    lock_path: &Path,
    lock: &mut GlobalSkillsLock,
    plan: &GlobalRemovalPlan,
    previous: &[(SkillId, Option<SkillDefinition>)],
) -> CliResult<()> {
    for (id, definition) in previous {
        if definition.is_some() {
            service
                .skill_manager()
                .unregister_skill(id)
                .await
                .map_err(CliError::Service)?;
        }
    }
    lock.covered_roots = plan.remaining_roots.clone();
    lock.skills
        .retain(|entry| !plan.remove_lock_entries.contains(&entry.id));
    lock.save_to_file(lock_path)
        .map_err(|error| CliError::Config(format!("Failed to save global lock: {error}")))?;
    #[cfg(test)]
    if FAIL_AFTER_LOCK_SAVE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CliError::Config(
            "injected failure after global lock save".to_string(),
        ));
    }
    for id in &plan.delete_files {
        remove_path(&service.config().skill_storage_path.join(id))?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> CliResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(CliError::Io)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(CliError::Io)
    } else {
        Err(CliError::Config(format!(
            "unsupported global skill destination: {}",
            path.display()
        )))
    }
}

async fn capture_registry(
    service: &FastSkillService,
    ids: &[String],
) -> CliResult<Vec<(SkillId, Option<SkillDefinition>)>> {
    #[cfg(test)]
    if FAIL_REGISTRY_CAPTURE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(CliError::Config(
            "injected global registry read failure".to_string(),
        ));
    }
    let mut previous = Vec::new();
    for raw in ids {
        let id = SkillId::new(raw.clone()).map_err(CliError::Service)?;
        let value = service
            .skill_manager()
            .get_skill(&id)
            .await
            .map_err(CliError::Service)?;
        previous.push((id, value));
    }
    Ok(previous)
}

async fn restore_registry(
    service: &FastSkillService,
    previous: &[(SkillId, Option<SkillDefinition>)],
) -> Result<(), String> {
    let mut failures = Vec::new();
    for (id, definition) in previous {
        if service
            .skill_manager()
            .get_skill(id)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            if let Err(error) = service.skill_manager().unregister_skill(id).await {
                failures.push(error.to_string());
            }
        }
        if let Some(definition) = definition {
            if let Err(error) = service
                .skill_manager()
                .force_register_skill(definition.clone())
                .await
            {
                failures.push(error.to_string());
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

#[cfg(test)]
#[allow(clippy::await_holding_lock, clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fastskill_core::core::lock::GlobalLockedSkillEntry;
    use fastskill_core::core::origin::Resolved;
    use fastskill_core::ServiceConfig;
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

    async fn fixture(root: &Path) -> (FastSkillService, PathBuf, SkillId) {
        let storage = root.join("config/fastskill/skills");
        let installed = storage.join("managed");
        fs::create_dir_all(&installed).unwrap();
        fs::write(
            installed.join("SKILL.md"),
            "---\nname: managed\nversion: 1.0.0\ndescription: fixture\n---\nmanaged\n",
        )
        .unwrap();
        let origin = Origin::Local {
            path: root.join("source/managed"),
            editable: false,
        };
        let mut lock = GlobalSkillsLock::new_empty();
        lock.covered_roots = vec!["managed".to_string()];
        lock.skills = vec![GlobalLockedSkillEntry {
            id: "managed".to_string(),
            name: "managed".to_string(),
            origin: origin.clone(),
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum: Some(fastskill_core::core::install::content_digest(&installed).unwrap()),
            },
            dependencies: Vec::new(),
            groups: Vec::new(),
            installed_at: Utc::now(),
            last_checked_at: None,
            last_updated_at: None,
        }];
        let lock_path = root.join("config/fastskill/global-skills.lock");
        lock.save_to_file(&lock_path).unwrap();
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: storage,
            skill_cache_root: Some(root.join("cache")),
            ..ServiceConfig::default()
        })
        .await
        .unwrap();
        let id = SkillId::new("managed".to_string()).unwrap();
        let mut definition = SkillDefinition::new(
            id.clone(),
            "managed".to_string(),
            "fixture".to_string(),
            "1.0.0".to_string(),
            origin,
        );
        definition.skill_file = installed.join("SKILL.md");
        service
            .skill_manager()
            .force_register_skill(definition)
            .await
            .unwrap();
        (service, lock_path, id)
    }

    fn assert_no_recovery_markers(root: &Path) {
        assert!(!root
            .join("config/fastskill/.fastskill/recovery-required")
            .exists());
        assert!(!root
            .join("config/fastskill/skills/.fastskill-recovery-required")
            .exists());
    }

    #[tokio::test]
    async fn stale_global_plan_is_rejected_without_erasing_the_new_state() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let (service, lock_path, id) = fixture(root.path()).await;
        CHANGE_LOCK_AFTER_PREVIEW.store(true, std::sync::atomic::Ordering::SeqCst);

        let error = remove(&service, &[id.to_string()]).await.unwrap_err();

        assert!(error.to_string().contains("changed after removal planning"));
        assert!(fs::read_to_string(&lock_path)
            .unwrap()
            .contains("concurrent writer"));
        assert!(service
            .skill_manager()
            .get_skill(&id)
            .await
            .unwrap()
            .is_some());
        assert_no_recovery_markers(root.path());
    }

    #[tokio::test]
    async fn failure_after_lock_save_restores_files_lock_registry_and_markers() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let (service, lock_path, id) = fixture(root.path()).await;
        let before = fs::read(&lock_path).unwrap();
        FAIL_AFTER_LOCK_SAVE.store(true, std::sync::atomic::Ordering::SeqCst);

        let error = remove(&service, &[id.to_string()]).await.unwrap_err();

        assert!(error.to_string().contains("injected failure"));
        assert_eq!(fs::read(&lock_path).unwrap(), before);
        assert!(service.config().skill_storage_path.join("managed").is_dir());
        assert!(service
            .skill_manager()
            .get_skill(&id)
            .await
            .unwrap()
            .is_some());
        assert_no_recovery_markers(root.path());
    }

    #[tokio::test]
    async fn registry_read_failure_before_mutation_clears_the_writer_marker() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let (service, lock_path, id) = fixture(root.path()).await;
        let before = fs::read(&lock_path).unwrap();
        FAIL_REGISTRY_CAPTURE.store(true, std::sync::atomic::Ordering::SeqCst);

        let error = remove(&service, &[id.to_string()]).await.unwrap_err();

        assert!(error.to_string().contains("registry read failure"));
        assert_eq!(fs::read(&lock_path).unwrap(), before);
        assert!(service.config().skill_storage_path.join("managed").is_dir());
        assert_no_recovery_markers(root.path());
    }

    #[tokio::test]
    async fn successful_remove_commits_lock_files_registry_and_writer_markers() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let (service, lock_path, id) = fixture(root.path()).await;

        let plan = remove(&service, &[id.to_string()]).await.unwrap();

        assert_eq!(plan.remove_roots, vec!["managed"]);
        assert!(!service.config().skill_storage_path.join("managed").exists());
        assert!(GlobalSkillsLock::load_from_file(&lock_path)
            .unwrap()
            .skills
            .is_empty());
        assert!(service
            .skill_manager()
            .get_skill(&id)
            .await
            .unwrap()
            .is_none());
        assert_no_recovery_markers(root.path());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_failure_clears_writer_markers_without_mutating_state() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let (service, lock_path, id) = fixture(root.path()).await;
        let bundles = root.path().join("config/fastskill/.fastskill/bundles");
        fs::create_dir_all(&bundles).unwrap();
        std::os::unix::fs::symlink(&lock_path, bundles.join("nested-link")).unwrap();
        let before = fs::read(&lock_path).unwrap();

        let error = remove(&service, &[id.to_string()]).await.unwrap_err();

        assert!(error.to_string().contains("Symbolic links"));
        assert_eq!(fs::read(&lock_path).unwrap(), before);
        assert!(service.config().skill_storage_path.join("managed").exists());
        assert_no_recovery_markers(root.path());
    }

    #[tokio::test]
    async fn preview_validates_legacy_integrity_and_editable_install_shape() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let (service, lock_path, _) = fixture(root.path()).await;
        let mut lock = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
        lock.skills[0].resolved.checksum = None;
        lock.save_to_file(&lock_path).unwrap();
        assert!(preview(&service, &["managed".to_string()])
            .unwrap_err()
            .to_string()
            .contains("no locked content digest"));

        lock.skills[0].origin = Origin::Local {
            path: root.path().join("source/managed"),
            editable: true,
        };
        lock.save_to_file(&lock_path).unwrap();
        assert!(preview(&service, &["managed".to_string()])
            .unwrap_err()
            .to_string()
            .contains("not installed as a link"));

        fs::remove_dir_all(service.config().skill_storage_path.join("managed")).unwrap();
        assert!(preview(&service, &["managed".to_string()]).is_ok());
    }

    #[test]
    fn lock_and_path_helpers_cover_absent_invalid_file_and_directory_states() {
        let _mutex = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let (path, bytes) = load_lock().unwrap();
        assert!(bytes.is_empty());
        assert!(lock_from_bytes(&bytes, &path).unwrap().skills.is_empty());

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "invalid = [").unwrap();
        assert!(lock_from_bytes(b"invalid", &path).is_err());

        let missing = root.path().join("missing");
        remove_path(&missing).unwrap();
        let file = root.path().join("file");
        fs::write(&file, "content").unwrap();
        remove_path(&file).unwrap();
        let directory = root.path().join("directory");
        fs::create_dir_all(&directory).unwrap();
        remove_path(&directory).unwrap();
        assert!(!file.exists());
        assert!(!directory.exists());
    }
}

//! Transactional application of project removal plans.

use crate::core::lifecycle_transaction::LifecycleTransaction;
use crate::core::lock::ProjectSkillsLock;
use crate::core::manifest::SkillProjectToml;
use crate::core::ownership::{normalize_lock_ownership, ProjectOwnership, ProjectRemovalPlan};
use crate::core::project_state::save_project_preserving;
use crate::core::service::ServiceError;
use crate::core::service::SkillId;
use crate::core::state_guard::StateMutationGuard;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
static FAIL_AFTER_MANIFEST_SAVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static CHANGE_STATE_AFTER_PREVIEW: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static CHANGE_CONTENT_AFTER_PREVIEW: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static CORRUPT_LOCK_AFTER_PREVIEW: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static REPLACE_LOCK_WITH_DIRECTORY_BEFORE_SAVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub struct ProjectRemovalService {
    project_root: PathBuf,
    skills_directory: PathBuf,
}

impl ProjectRemovalService {
    pub fn new(project_root: impl Into<PathBuf>, skills_directory: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            skills_directory: skills_directory.into(),
        }
    }

    pub fn remove(&self, requested: &[String]) -> Result<ProjectRemovalPlan, ServiceError> {
        let preview = self.preview(requested)?;
        let manifest_path = self.project_root.join("skill-project.toml");
        let lock_path = self.project_root.join("skills.lock");
        #[cfg(test)]
        if CHANGE_STATE_AFTER_PREVIEW.swap(false, std::sync::atomic::Ordering::SeqCst) {
            let mut changed =
                SkillProjectToml::load_from_file(&manifest_path).map_err(|error| {
                    ServiceError::Config(format!("Failed to load skill-project.toml: {error}"))
                })?;
            if let Some(dependencies) = changed.dependencies.as_mut() {
                dependencies.dependencies.clear();
            }
            changed.save_to_file(&manifest_path).map_err(|error| {
                ServiceError::Config(format!("Failed to change test Manifest: {error}"))
            })?;
        }
        #[cfg(test)]
        if CHANGE_CONTENT_AFTER_PREVIEW.swap(false, std::sync::atomic::Ordering::SeqCst) {
            fs::write(
                self.skills_directory.join("item/LOCAL.md"),
                "concurrent edit",
            )
            .map_err(ServiceError::Io)?;
        }
        #[cfg(test)]
        if CORRUPT_LOCK_AFTER_PREVIEW.swap(false, std::sync::atomic::Ordering::SeqCst) {
            fs::write(&lock_path, "invalid = [").map_err(ServiceError::Io)?;
        }
        let state_guard = StateMutationGuard::acquire_for(
            &self.project_root,
            Some(&self.skills_directory),
            "remove skills",
        )?;
        let (manifest, lock, plan) = match load_and_plan(&manifest_path, &lock_path, requested) {
            Ok(state) => state,
            Err(error) => {
                state_guard.recovered()?;
                return Err(error);
            }
        };
        if plan != preview {
            state_guard.recovered()?;
            return Err(ServiceError::InvalidOperation(
                "Project state changed after the removal plan was prepared; retry the command"
                    .to_string(),
            ));
        }
        if let Err(error) = self.ensure_unmodified(&lock, &plan.delete_files) {
            state_guard.recovered()?;
            return Err(error);
        }
        self.apply_plan(manifest_path, lock_path, state_guard, manifest, lock, plan)
    }

    /// Build and validate the exact removal plan without modifying managed state.
    pub fn preview(&self, requested: &[String]) -> Result<ProjectRemovalPlan, ServiceError> {
        for id in requested {
            SkillId::new(id.clone())?;
        }
        let manifest_path = self.project_root.join("skill-project.toml");
        let lock_path = self.project_root.join("skills.lock");
        let (_, lock, plan) = load_and_plan(&manifest_path, &lock_path, requested)?;
        self.ensure_unmodified(&lock, &plan.delete_files)?;
        Ok(plan)
    }

    fn apply_plan(
        &self,
        manifest_path: PathBuf,
        lock_path: PathBuf,
        state_guard: StateMutationGuard,
        mut manifest: SkillProjectToml,
        mut lock: ProjectSkillsLock,
        plan: ProjectRemovalPlan,
    ) -> Result<ProjectRemovalPlan, ServiceError> {
        if plan.remove_manifest_dependencies.is_empty()
            && plan.remove_lock_entries.is_empty()
            && plan.delete_files.is_empty()
        {
            state_guard.commit()?;
            return Ok(plan);
        }

        let transaction = match LifecycleTransaction::capture(
            &self.project_root,
            &self.skills_directory,
            &plan.delete_files,
        ) {
            Ok(transaction) => transaction,
            Err(error) => {
                state_guard.recovered()?;
                return Err(error);
            }
        };
        let result = (|| {
            if let Some(dependencies) = manifest.dependencies.as_mut() {
                for id in &plan.remove_manifest_dependencies {
                    dependencies.dependencies.remove(id);
                }
            }
            for id in &plan.remove_lock_entries {
                lock.remove_skill(id);
            }
            normalize_lock_ownership(&mut lock, &plan.remaining_individual_roots);
            save_project_preserving(&manifest_path, &manifest).map_err(|error| {
                ServiceError::Config(format!("Failed to save skill-project.toml: {error}"))
            })?;
            #[cfg(test)]
            if FAIL_AFTER_MANIFEST_SAVE.swap(false, std::sync::atomic::Ordering::SeqCst) {
                return Err(ServiceError::Custom(
                    "injected failure after Manifest save".to_string(),
                ));
            }
            #[cfg(test)]
            if REPLACE_LOCK_WITH_DIRECTORY_BEFORE_SAVE
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                fs::remove_file(&lock_path).map_err(ServiceError::Io)?;
                fs::create_dir(&lock_path).map_err(ServiceError::Io)?;
            }
            lock.save_to_file(&lock_path).map_err(|error| {
                ServiceError::Config(format!("Failed to save skills.lock: {error}"))
            })?;
            for id in &plan.delete_files {
                remove_installed_path(&self.skills_directory.join(id))?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            if let Err(recovery_error) = transaction.rollback() {
                return Err(ServiceError::Custom(format!(
                    "Removal failed: {error}; recovery also failed: {recovery_error}"
                )));
            }
            state_guard.recovered()?;
            return Err(error);
        }
        transaction.commit();
        state_guard.commit()?;
        Ok(plan)
    }

    fn ensure_unmodified(
        &self,
        lock: &ProjectSkillsLock,
        ids: &[String],
    ) -> Result<(), ServiceError> {
        for id in ids {
            let Some(entry) = lock.skills.iter().find(|entry| entry.id == *id) else {
                continue;
            };
            let path = self.skills_directory.join(id);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(ServiceError::Io(error)),
            };
            if matches!(
                entry.origin,
                crate::core::origin::Origin::Local { editable: true, .. }
            ) {
                if metadata.file_type().is_symlink() {
                    continue;
                }
                return Err(ServiceError::InvalidOperation(format!(
                    "Editable skill '{id}' is not installed as a link; refusing to delete its directory"
                )));
            }
            let expected = entry.resolved.checksum.as_ref().ok_or_else(|| {
                ServiceError::InvalidOperation(format!(
                    "Skill '{id}' has no locked content digest; restore it before removal so FastSkill can protect local edits"
                ))
            })?;
            let actual = managed_tree_digest(&path)?;
            if &actual != expected {
                return Err(ServiceError::InvalidOperation(format!(
                    "Skill '{id}' is locally modified; FastSkill will not delete it"
                )));
            }
        }
        Ok(())
    }
}

fn load_and_plan(
    manifest_path: &Path,
    lock_path: &Path,
    requested: &[String],
) -> Result<(SkillProjectToml, ProjectSkillsLock, ProjectRemovalPlan), ServiceError> {
    let manifest = SkillProjectToml::load_from_file(manifest_path).map_err(|error| {
        ServiceError::Config(format!("Failed to load skill-project.toml: {error}"))
    })?;
    let lock = if lock_path.exists() {
        ProjectSkillsLock::load_from_file(lock_path)
            .map_err(|error| ServiceError::Config(format!("Failed to load skills.lock: {error}")))?
    } else {
        ProjectSkillsLock::new_empty()
    };
    let plan = ProjectOwnership::new(&manifest, &lock).plan_removal(requested)?;
    Ok((manifest, lock, plan))
}

/// Canonical digest used for immutable installed-tree edit protection.
pub fn managed_tree_digest(path: &Path) -> Result<String, ServiceError> {
    crate::core::bundle_persistence::digest_directory(path)
}

fn remove_installed_path(path: &Path) -> Result<(), ServiceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ServiceError::Io(error)),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(ServiceError::Io)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(ServiceError::Io)
    } else {
        Err(ServiceError::Validation(format!(
            "Refusing to remove unsupported skill destination: {}",
            path.display()
        )))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::lock::{ProjectLockedSkillEntry, ProjectSkillsLock};
    use crate::core::manifest::{DependenciesSection, DependencySpec, SkillProjectToml};
    use crate::core::origin::{Origin, Resolved};
    use std::collections::HashMap;
    use tempfile::TempDir;

    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        TEST_MUTEX.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn project(root: &Path, checksum: Option<String>) -> ProjectRemovalService {
        fs::create_dir_all(root.join("skills/item")).unwrap();
        fs::write(root.join("skills/item/SKILL.md"), "item").unwrap();
        SkillProjectToml {
            schema_version: None,
            metadata: None,
            dependencies: Some(DependenciesSection {
                dependencies: HashMap::from([(
                    "item".to_string(),
                    DependencySpec::Version("1.0.0".to_string()),
                )]),
            }),
            tool: None,
        }
        .save_to_file(&root.join("skill-project.toml"))
        .unwrap();
        let mut lock = ProjectSkillsLock::new_empty();
        lock.skills.push(ProjectLockedSkillEntry {
            id: "item".to_string(),
            name: "item".to_string(),
            origin: Origin::Local {
                path: PathBuf::from("origin"),
                editable: false,
            },
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum,
            },
            dependencies: Vec::new(),
            groups: Vec::new(),
            depth: 0,
            parent_skill: None,
            required_by: Vec::new(),
        });
        lock.save_to_file(&root.join("skills.lock")).unwrap();
        ProjectRemovalService::new(root, root.join("skills"))
    }

    #[test]
    fn preview_rejects_invalid_project_state_and_missing_integrity() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("skill-project.toml"), "not toml = [").unwrap();
        let service = ProjectRemovalService::new(root.path(), root.path().join("skills"));
        assert!(service.preview(&["item".to_string()]).is_err());

        let service = project(root.path(), None);
        assert!(service.preview(&["item".to_string()]).is_err());
        fs::write(root.path().join("skills.lock"), "invalid = [").unwrap();
        assert!(service.preview(&["item".to_string()]).is_err());
    }

    #[test]
    fn integrity_check_skips_untracked_and_missing_content_and_rejects_broken_editable_state() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let service = project(root.path(), None);
        let lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();

        service
            .ensure_unmodified(&ProjectSkillsLock::new_empty(), &["untracked".to_string()])
            .unwrap();
        fs::remove_dir_all(root.path().join("skills/item")).unwrap();
        service
            .ensure_unmodified(&lock, &["item".to_string()])
            .unwrap();

        project(root.path(), None);
        let mut editable = lock;
        editable.skills[0].origin = Origin::Local {
            path: PathBuf::from("origin"),
            editable: true,
        };
        let error = service
            .ensure_unmodified(&editable, &["item".to_string()])
            .unwrap_err();
        assert!(error.to_string().contains("not installed as a link"));
    }

    #[test]
    fn unchanged_remove_commits_without_rewriting_state() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let service = project(root.path(), None);
        let manifest = fs::read(root.path().join("skill-project.toml")).unwrap();
        let lock = fs::read(root.path().join("skills.lock")).unwrap();

        let plan = service.remove(&["absent".to_string()]).unwrap();

        assert_eq!(plan.unchanged, vec!["absent"]);
        assert_eq!(
            fs::read(root.path().join("skill-project.toml")).unwrap(),
            manifest
        );
        assert_eq!(fs::read(root.path().join("skills.lock")).unwrap(), lock);
        assert!(!root.path().join(".fastskill/recovery-required").exists());
    }

    #[test]
    fn failure_after_manifest_save_rolls_back_all_authoritative_state() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let digest = managed_tree_digest(&root.path().join("skills/item")).unwrap_err();
        assert!(digest.to_string().contains("Expected skill directory"));
        let service = project(root.path(), None);
        let installed = root.path().join("skills/item");
        let digest = managed_tree_digest(&installed).unwrap();
        let mut lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();
        lock.skills[0].resolved.checksum = Some(digest);
        lock.save_to_file(&root.path().join("skills.lock")).unwrap();
        let before_manifest = fs::read(root.path().join("skill-project.toml")).unwrap();
        let before_lock = fs::read(root.path().join("skills.lock")).unwrap();
        FAIL_AFTER_MANIFEST_SAVE.store(true, std::sync::atomic::Ordering::SeqCst);

        let error = service.remove(&["item".to_string()]).unwrap_err();

        assert!(error.to_string().contains("injected failure"));
        assert_eq!(
            fs::read(root.path().join("skill-project.toml")).unwrap(),
            before_manifest
        );
        assert_eq!(
            fs::read(root.path().join("skills.lock")).unwrap(),
            before_lock
        );
        assert!(installed.join("SKILL.md").is_file());
        assert!(!root.path().join(".fastskill/recovery-required").exists());
    }

    #[test]
    fn installed_path_removal_handles_files_links_and_missing_paths() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let file = root.path().join("file");
        fs::write(&file, "value").unwrap();
        remove_installed_path(&file).unwrap();
        assert!(!file.exists());
        remove_installed_path(&file).unwrap();

        let directory = root.path().join("directory");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("content"), "value").unwrap();
        remove_installed_path(&directory).unwrap();
        assert!(!directory.exists());

        #[cfg(unix)]
        {
            let target = root.path().join("target");
            let link = root.path().join("link");
            fs::write(&target, "source").unwrap();
            std::os::unix::fs::symlink(&target, &link).unwrap();
            remove_installed_path(&link).unwrap();
            assert!(target.is_file());
            assert!(fs::symlink_metadata(&link).is_err());
        }
    }

    #[test]
    fn removal_rejects_state_and_content_changes_after_preview_without_poisoning_state() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let service = project(root.path(), None);
        let installed = root.path().join("skills/item");
        let digest = managed_tree_digest(&installed).unwrap();
        let mut lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();
        lock.skills[0].resolved.checksum = Some(digest);
        lock.save_to_file(&root.path().join("skills.lock")).unwrap();

        CHANGE_STATE_AFTER_PREVIEW.store(true, std::sync::atomic::Ordering::SeqCst);
        let state_error = service.remove(&["item".to_string()]).unwrap_err();
        assert!(state_error.to_string().contains("state changed"));
        assert!(!root.path().join(".fastskill/recovery-required").exists());

        project(root.path(), None);
        let digest = managed_tree_digest(&installed).unwrap();
        let mut lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();
        lock.skills[0].resolved.checksum = Some(digest);
        lock.save_to_file(&root.path().join("skills.lock")).unwrap();
        CHANGE_CONTENT_AFTER_PREVIEW.store(true, std::sync::atomic::Ordering::SeqCst);

        let content_error = service.remove(&["item".to_string()]).unwrap_err();
        assert!(content_error.to_string().contains("locally modified"));
        assert!(installed.join("SKILL.md").is_file());
        assert!(!root.path().join(".fastskill/recovery-required").exists());
    }

    #[test]
    fn removal_recovers_from_post_preview_lock_corruption_and_capture_failure() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let service = project(root.path(), None);
        let installed = root.path().join("skills/item");
        let digest = managed_tree_digest(&installed).unwrap();
        let mut lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();
        lock.skills[0].resolved.checksum = Some(digest);
        lock.save_to_file(&root.path().join("skills.lock")).unwrap();
        CORRUPT_LOCK_AFTER_PREVIEW.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(service.remove(&["item".to_string()]).is_err());
        assert!(!root.path().join(".fastskill/recovery-required").exists());

        let service = project(root.path(), None);
        let digest = managed_tree_digest(&installed).unwrap();
        let mut lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();
        lock.skills[0].resolved.checksum = Some(digest);
        lock.save_to_file(&root.path().join("skills.lock")).unwrap();
        let bundles = root.path().join(".fastskill/bundles");
        fs::create_dir_all(&bundles).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&installed, bundles.join("linked")).unwrap();
        assert!(service.remove(&["item".to_string()]).is_err());
        assert!(installed.join("SKILL.md").is_file());
        assert!(!root.path().join(".fastskill/recovery-required").exists());
    }

    #[test]
    fn lock_write_failure_rolls_back_manifest_and_installed_content() {
        let _serial = serial();
        let root = TempDir::new().unwrap();
        let service = project(root.path(), None);
        let installed = root.path().join("skills/item");
        let digest = managed_tree_digest(&installed).unwrap();
        let mut lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();
        lock.skills[0].resolved.checksum = Some(digest);
        lock.save_to_file(&root.path().join("skills.lock")).unwrap();
        let before_manifest = fs::read(root.path().join("skill-project.toml")).unwrap();
        let before_lock = fs::read(root.path().join("skills.lock")).unwrap();
        REPLACE_LOCK_WITH_DIRECTORY_BEFORE_SAVE.store(true, std::sync::atomic::Ordering::SeqCst);

        assert!(service.remove(&["item".to_string()]).is_err());

        assert_eq!(
            fs::read(root.path().join("skill-project.toml")).unwrap(),
            before_manifest
        );
        assert_eq!(
            fs::read(root.path().join("skills.lock")).unwrap(),
            before_lock
        );
        assert!(installed.join("SKILL.md").is_file());
        assert!(!root.path().join(".fastskill/recovery-required").exists());
    }
}

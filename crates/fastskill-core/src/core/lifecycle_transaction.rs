//! Whole-project snapshots used to recover multi-phase lifecycle operations.

use crate::core::service::{ServiceError, SkillId};
use crate::utils::atomic_write;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;

enum Backup {
    Missing,
    Directory(PathBuf),
    /// The link target itself; keeping another live link in the backup tree
    /// would require an unnecessary second symlink privilege on Windows.
    Symlink {
        target: PathBuf,
        directory: bool,
    },
    File(PathBuf),
}

/// A failed rollback whose retained directory contains the recovery inputs.
#[derive(Debug)]
pub struct LifecycleRollbackError {
    source: ServiceError,
    backup_path: PathBuf,
}

impl LifecycleRollbackError {
    pub fn backup_path(&self) -> &Path {
        &self.backup_path
    }
}

impl std::fmt::Display for LifecycleRollbackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}; recovery inputs retained at {}",
            self.source,
            self.backup_path.display()
        )
    }
}

impl std::error::Error for LifecycleRollbackError {}

/// Snapshot of authoritative project state and every affected installed target.
///
/// Callers validate and acquire their writer lease before capture, then retain
/// the snapshot until every phase has completed.
pub struct LifecycleTransaction {
    temporary: TempDir,
    paths: Vec<(PathBuf, Backup)>,
}

impl LifecycleTransaction {
    pub fn capture(
        project_root: &Path,
        destination: &Path,
        ids: &[String],
    ) -> Result<Self, ServiceError> {
        Self::capture_with_files(project_root, destination, ids, &[])
    }

    /// Capture additional authoritative files, such as the global operational
    /// Lock, in the same rollback boundary as installed content.
    pub fn capture_with_files(
        project_root: &Path,
        destination: &Path,
        ids: &[String],
        additional_files: &[PathBuf],
    ) -> Result<Self, ServiceError> {
        for id in ids {
            SkillId::new(id.clone())?;
        }
        let temporary = TempDir::new().map_err(ServiceError::Io)?;
        let mut paths = vec![
            project_root.join("skill-project.toml"),
            project_root.join("skills.lock"),
            project_root.join(".fastskill/bundle-history.toml"),
            project_root.join(".fastskill/bundles"),
        ];
        paths.extend(additional_files.iter().cloned());
        paths.extend(ids.iter().map(|id| destination.join(id)));
        paths.sort();
        paths.dedup();
        let mut captured = Vec::with_capacity(paths.len());
        for (index, path) in paths.into_iter().enumerate() {
            captured.push((path.clone(), capture_path(&temporary, index, &path)?));
        }
        Ok(Self {
            temporary,
            paths: captured,
        })
    }

    pub fn rollback(self) -> Result<(), LifecycleRollbackError> {
        for (path, backup) in self.paths.iter().rev() {
            let result = remove_path(path).and_then(|()| {
                match backup {
                    Backup::Missing => {}
                    Backup::Directory(source) => copy_directory(source, path)?,
                    Backup::Symlink { target, directory } => {
                        create_symlink(target, path, *directory)?
                    }
                    Backup::File(source) => {
                        atomic_write(path, &fs::read(source).map_err(ServiceError::Io)?)
                            .map_err(ServiceError::Io)?
                    }
                }
                Ok(())
            });
            if let Err(source) = result {
                let backup_path = self.temporary.keep();
                return Err(LifecycleRollbackError {
                    source,
                    backup_path,
                });
            }
        }
        Ok(())
    }

    pub fn commit(self) {
        drop(self.temporary);
    }
}

fn capture_path(temporary: &TempDir, index: usize, path: &Path) -> Result<Backup, ServiceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Backup::Missing),
        Err(error) => return Err(ServiceError::Io(error)),
    };
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(path).map_err(ServiceError::Io)?;
        let directory = symlink_is_directory(&metadata);
        // Keep a passive record for manual recovery if a later rollback step
        // fails. Recording bytes avoids creating a second live link, which can
        // require elevated privileges on Windows.
        fs::write(
            temporary.path().join(format!("{index}.link-target")),
            target.as_os_str().as_encoded_bytes(),
        )
        .map_err(ServiceError::Io)?;
        fs::write(
            temporary.path().join(format!("{index}.link-kind")),
            symlink_kind_label(directory),
        )
        .map_err(ServiceError::Io)?;
        return Ok(Backup::Symlink { target, directory });
    }
    if metadata.is_file() {
        let backup = temporary.path().join(format!("{index}.file"));
        fs::copy(path, &backup).map_err(ServiceError::Io)?;
        return Ok(Backup::File(backup));
    }
    if metadata.is_dir() {
        let backup = temporary.path().join(index.to_string());
        copy_directory(path, &backup)?;
        return Ok(Backup::Directory(backup));
    }
    Err(ServiceError::Validation(format!(
        "Unsupported managed path: {}",
        path.display()
    )))
}

fn remove_path(path: &Path) -> Result<(), ServiceError> {
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
            "Unsupported managed path: {}",
            path.display()
        )))
    }
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), ServiceError> {
    for entry in WalkDir::new(source).sort_by_file_name() {
        let entry = entry.map_err(|error| ServiceError::Io(std::io::Error::other(error)))?;
        if entry.file_type().is_symlink() {
            return Err(ServiceError::Validation(format!(
                "Symbolic links are not permitted inside managed directories: {}",
                entry.path().display()
            )));
        }
        let relative = entry.path().strip_prefix(source).map_err(|error| {
            ServiceError::Custom(format!("Failed to form backup path: {error}"))
        })?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(ServiceError::Io)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(ServiceError::Io)?;
            }
            fs::copy(entry.path(), target).map_err(ServiceError::Io)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path, _directory: bool) -> Result<(), ServiceError> {
    std::os::unix::fs::symlink(target, link).map_err(ServiceError::Io)
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path, directory: bool) -> Result<(), ServiceError> {
    if directory {
        std::os::windows::fs::symlink_dir(target, link).map_err(ServiceError::Io)
    } else {
        std::os::windows::fs::symlink_file(target, link).map_err(ServiceError::Io)
    }
}

#[cfg(windows)]
fn symlink_is_directory(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::FileTypeExt;
    metadata.file_type().is_symlink_dir()
}

#[cfg(not(windows))]
fn symlink_is_directory(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn symlink_kind_label(directory: bool) -> &'static str {
    if directory {
        "directory"
    } else {
        "file"
    }
}

#[cfg(not(windows))]
fn symlink_kind_label(_directory: bool) -> &'static str {
    "generic"
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rollback_restores_state_directories_files_links_and_missing_paths() {
        let root = TempDir::new().unwrap();
        let destination = root.path().join("skills");
        fs::create_dir_all(destination.join("directory")).unwrap();
        fs::write(destination.join("directory/SKILL.md"), "directory").unwrap();
        fs::write(destination.join("file"), "file").unwrap();
        fs::write(root.path().join("skill-project.toml"), "before").unwrap();
        fs::create_dir_all(root.path().join(".fastskill/bundles")).unwrap();
        fs::write(
            root.path().join(".fastskill/bundles/release.zip"),
            "release",
        )
        .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("directory", destination.join("link")).unwrap();
        let mut ids = vec![
            "directory".to_string(),
            "file".to_string(),
            "missing".to_string(),
        ];
        #[cfg(unix)]
        ids.push("link".to_string());
        let transaction = LifecycleTransaction::capture(root.path(), &destination, &ids).unwrap();
        fs::remove_dir_all(destination.join("directory")).unwrap();
        fs::remove_file(destination.join("file")).unwrap();
        #[cfg(unix)]
        fs::remove_file(destination.join("link")).unwrap();
        fs::create_dir_all(destination.join("missing")).unwrap();
        fs::write(root.path().join("skill-project.toml"), "after").unwrap();
        fs::remove_dir_all(root.path().join(".fastskill/bundles")).unwrap();

        transaction.rollback().unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("directory/SKILL.md")).unwrap(),
            "directory"
        );
        assert_eq!(
            fs::read_to_string(destination.join("file")).unwrap(),
            "file"
        );
        assert!(!destination.join("missing").exists());
        #[cfg(unix)]
        assert!(fs::symlink_metadata(destination.join("link"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(root.path().join("skill-project.toml")).unwrap(),
            "before"
        );
        assert_eq!(
            fs::read_to_string(root.path().join(".fastskill/bundles/release.zip")).unwrap(),
            "release"
        );
    }

    #[test]
    fn capture_validates_ids_and_rejects_nested_links() {
        let root = TempDir::new().unwrap();
        let destination = root.path().join("skills");
        assert!(
            LifecycleTransaction::capture(root.path(), &destination, &["../bad".to_string()])
                .is_err()
        );
        fs::create_dir_all(destination.join("item")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("outside", destination.join("item/link")).unwrap();
            assert!(LifecycleTransaction::capture(
                root.path(),
                &destination,
                &["item".to_string()]
            )
            .is_err());
        }
    }

    #[test]
    fn commit_keeps_changes() {
        let root = TempDir::new().unwrap();
        let destination = root.path().join("skills");
        fs::create_dir_all(destination.join("item")).unwrap();
        fs::write(destination.join("item/SKILL.md"), "before").unwrap();
        let transaction =
            LifecycleTransaction::capture(root.path(), &destination, &["item".to_string()])
                .unwrap();
        fs::write(destination.join("item/SKILL.md"), "after").unwrap();
        transaction.commit();
        assert_eq!(
            fs::read_to_string(destination.join("item/SKILL.md")).unwrap(),
            "after"
        );
    }

    #[test]
    fn rollback_error_display_names_the_retained_backup() {
        let error = LifecycleRollbackError {
            source: ServiceError::Validation("restore failed".to_string()),
            backup_path: PathBuf::from("/tmp/recovery"),
        };
        let message = error.to_string();
        assert!(message.contains("restore failed"));
        assert!(message.contains("/tmp/recovery"));
    }

    #[test]
    fn failed_rollback_retains_file_backups_for_manual_recovery() {
        let root = TempDir::new().unwrap();
        let destination = root.path().join("skills");
        fs::create_dir_all(&destination).unwrap();
        fs::write(root.path().join("skill-project.toml"), "before").unwrap();
        fs::write(destination.join("item"), "before").unwrap();
        let mut ids = vec!["item".to_string()];
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("item", destination.join("link")).unwrap();
            ids.push("link".to_string());
        }
        let transaction = LifecycleTransaction::capture(root.path(), &destination, &ids).unwrap();
        fs::remove_dir_all(&destination).unwrap();
        fs::write(&destination, "blocks child restoration").unwrap();

        let error = transaction.rollback().unwrap_err();

        assert!(error.backup_path().exists());
        assert!(fs::read_dir(error.backup_path())
            .unwrap()
            .flatten()
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|value| value == "file")));
        #[cfg(unix)]
        assert!(fs::read_dir(error.backup_path())
            .unwrap()
            .flatten()
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|value| value == "link-target")));
        #[cfg(unix)]
        assert!(fs::read_dir(error.backup_path())
            .unwrap()
            .flatten()
            .any(|entry| entry
                .path()
                .extension()
                .is_some_and(|value| value == "link-kind")));
    }
}

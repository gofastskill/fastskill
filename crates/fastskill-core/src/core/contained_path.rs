//! Installed-skill locations proven to lie inside their skills directory.
//!
//! Every helper that deletes or replaces installed content takes a
//! [`ContainedPath`] rather than a bare path, so the only way to reach one is
//! through the checks below — never a raw `root.join(id)` with an id read from
//! a lock or manifest.

use crate::core::service::{ServiceError, SkillId};
use std::fs;
use std::path::{Path, PathBuf};

/// The installed location of one skill: `<root>/<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainedPath {
    parent: PathBuf,
    path: PathBuf,
}

impl ContainedPath {
    /// The location of skill `id` under the skills directory `root`.
    ///
    /// Refuses an id that is not a [`SkillId`], which rules out `..`,
    /// absolute paths and deeper nesting. For a `scope/name` id it also
    /// refuses a symlinked scope directory, which would otherwise carry the
    /// path outside `root`. The skill directory itself may be a symlink:
    /// removal unlinks it without following it.
    pub fn skill(root: &Path, id: &str) -> Result<Self, ServiceError> {
        let id = SkillId::new(id.to_string())?;
        let parent = match id.as_str().split_once('/') {
            Some((scope, _)) => root.join(scope),
            None => root.to_path_buf(),
        };
        if parent != root
            && fs::symlink_metadata(&parent).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(ServiceError::Validation(format!(
                "Refusing to act on '{id}': its scope directory {} is a symlink",
                parent.display()
            )));
        }
        Ok(Self {
            path: root.join(id.as_str()),
            parent,
        })
    }

    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// The directory that holds this skill: the root, or its scope directory.
    pub fn parent(&self) -> &Path {
        &self.parent
    }
}

/// Remove whatever sits at `path`: a symlink is unlinked without following
/// it, a file is deleted and a directory is removed recursively. Nothing there
/// is not an error.
pub fn remove_contained(path: &ContainedPath) -> Result<(), ServiceError> {
    let path = path.as_path();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ServiceError::Io(error)),
    };
    if metadata.file_type().is_symlink() {
        crate::core::lifecycle_transaction::unlink_symlink(path, &metadata)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(ServiceError::Io)
    } else {
        fs::remove_file(path).map_err(ServiceError::Io)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_skill_id_names_a_child_of_the_root() {
        let root = TempDir::new().unwrap();
        let plain = ContainedPath::skill(root.path(), "demo").unwrap();
        let scoped = ContainedPath::skill(root.path(), "team/demo").unwrap();
        assert_eq!(plain.as_path(), root.path().join("demo"));
        assert_eq!(plain.parent(), root.path());
        assert_eq!(scoped.as_path(), root.path().join("team/demo"));
        assert_eq!(scoped.parent(), root.path().join("team"));
    }

    #[test]
    fn an_id_that_is_not_a_skill_id_is_refused() {
        let root = TempDir::new().unwrap();
        for id in ["../victim", "/etc", "a/b/c", "", "a/../b", "."] {
            assert!(
                ContainedPath::skill(root.path(), id).is_err(),
                "{id:?} must be refused"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_scope_directory_is_refused() {
        let root = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("team")).unwrap();
        let error = ContainedPath::skill(root.path(), "team/demo")
            .unwrap_err()
            .to_string();
        assert!(error.contains("symlink"), "{error}");
    }

    #[test]
    fn removal_handles_missing_files_and_directories() {
        let root = TempDir::new().unwrap();
        let missing = ContainedPath::skill(root.path(), "missing").unwrap();
        remove_contained(&missing).unwrap();

        fs::write(root.path().join("file"), "x").unwrap();
        remove_contained(&ContainedPath::skill(root.path(), "file").unwrap()).unwrap();
        assert!(!root.path().join("file").exists());

        fs::create_dir_all(root.path().join("dir/nested")).unwrap();
        fs::write(root.path().join("dir/nested/SKILL.md"), "x").unwrap();
        remove_contained(&ContainedPath::skill(root.path(), "dir").unwrap()).unwrap();
        assert!(!root.path().join("dir").exists());
    }

    #[cfg(unix)]
    #[test]
    fn removal_unlinks_a_symlinked_skill_without_following_it() {
        let root = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        fs::write(source.path().join("SKILL.md"), "keep").unwrap();
        std::os::unix::fs::symlink(source.path(), root.path().join("linked")).unwrap();

        remove_contained(&ContainedPath::skill(root.path(), "linked").unwrap()).unwrap();

        assert!(!root.path().join("linked").exists());
        assert!(source.path().join("SKILL.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn removal_reports_an_unreadable_parent() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempDir::new().unwrap();
        let scope = root.path().join("team");
        fs::create_dir(&scope).unwrap();
        fs::set_permissions(&scope, fs::Permissions::from_mode(0o000)).unwrap();
        let result = remove_contained(&ContainedPath::skill(root.path(), "team/demo").unwrap());
        fs::set_permissions(&scope, fs::Permissions::from_mode(0o755)).unwrap();
        // Root bypasses permission bits, so only assert when they applied.
        if let Err(error) = result {
            assert!(matches!(error, ServiceError::Io(_)), "{error}");
        }
    }
}

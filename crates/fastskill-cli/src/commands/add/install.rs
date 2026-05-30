//! Unified skill installation function replacing install_local_skill + install_copied_skill.

use crate::error::{CliError, CliResult};
use std::path::Path;

/// Install a skill from `src` into `dst`.
///
/// When `editable` is `true` a symlink is created (like `poetry add -e`).
/// When `editable` is `false` the directory is copied recursively.
#[allow(dead_code)]
pub async fn install_skill(src: &Path, dst: &Path, editable: bool) -> CliResult<()> {
    // Remove any existing destination (directory, symlink, or file)
    if dst.exists() || dst.is_symlink() {
        if dst.is_symlink() || dst.is_file() {
            tokio::fs::remove_file(dst).await.map_err(CliError::Io)?;
        } else {
            tokio::fs::remove_dir_all(dst).await.map_err(CliError::Io)?;
        }
    }
    crate::utils::install_utils::setup_skill_in_storage(src, dst, editable).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_skill_dir(parent: &Path, name: &str) -> std::path::PathBuf {
        let dir = parent.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# Test Skill\n").unwrap();
        dir
    }

    #[tokio::test]
    async fn test_install_skill_editable_creates_symlink() {
        let tmp = TempDir::new().unwrap();
        let src = create_skill_dir(tmp.path(), "source-skill");
        let dst = tmp.path().join("installed-skill");

        install_skill(&src, &dst, true).await.unwrap();

        assert!(
            dst.is_symlink(),
            "editable install must create a symlink, not a copy"
        );
        let link_target = fs::read_link(&dst).unwrap();
        assert_eq!(
            link_target.canonicalize().unwrap(),
            src.canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn test_install_skill_non_editable_creates_copy() {
        let tmp = TempDir::new().unwrap();
        let src = create_skill_dir(tmp.path(), "source-skill");
        let dst = tmp.path().join("installed-skill");

        install_skill(&src, &dst, false).await.unwrap();

        assert!(
            !dst.is_symlink(),
            "non-editable install must create a copy, not a symlink"
        );
        assert!(dst.is_dir(), "non-editable install must create a directory");
        assert!(
            dst.join("SKILL.md").exists(),
            "SKILL.md must be present in copied directory"
        );
    }

    #[tokio::test]
    async fn test_install_skill_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let src = create_skill_dir(tmp.path(), "source-skill");
        let dst = tmp.path().join("installed-skill");

        // First install
        install_skill(&src, &dst, false).await.unwrap();
        assert!(dst.exists());

        // Second install should overwrite
        install_skill(&src, &dst, false).await.unwrap();
        assert!(dst.exists());
        assert!(!dst.is_symlink());
    }
}

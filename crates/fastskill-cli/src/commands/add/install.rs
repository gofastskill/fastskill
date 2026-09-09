//! Filesystem helpers used by add and global update operations.

use crate::error::{CliError, CliResult};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn component_is_hidden(c: std::path::Component<'_>) -> bool {
    matches!(c, std::path::Component::Normal(name) if name.to_string_lossy().starts_with('.'))
}

/// Discover all directories containing SKILL.md under the given base directory.
/// Skips hidden directories (those starting with '.').
pub(super) fn get_skill_dirs_recursive(base: &Path) -> CliResult<Vec<PathBuf>> {
    if !base.exists() {
        return Err(CliError::InvalidSource(format!(
            "Directory does not exist: {}",
            base.display()
        )));
    }
    if !base.is_dir() {
        return Err(CliError::InvalidSource(format!(
            "Path is not a directory: {}",
            base.display()
        )));
    }

    let base_canonical = base.canonicalize().map_err(CliError::Io)?;
    let mut skill_dirs = Vec::new();

    for entry in WalkDir::new(base)
        .min_depth(1)
        .max_depth(usize::MAX)
        .follow_links(false)
    {
        let entry = entry.map_err(|e| {
            CliError::Io(
                e.into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("WalkDir error")),
            )
        })?;
        let path = entry.path();
        if !path.is_dir() || !path.join("SKILL.md").exists() {
            continue;
        }

        let path_canonical = path.canonicalize().map_err(CliError::Io)?;
        let relative_path = path_canonical.strip_prefix(&base_canonical).map_err(|_| {
            CliError::Validation(format!(
                "Failed to compute relative path for: {}",
                path.display()
            ))
        })?;
        if relative_path.components().any(component_is_hidden) {
            continue;
        }
        skill_dirs.push(path_canonical);
    }

    Ok(skill_dirs)
}

/// Recursively copy a directory from src to dst
pub async fn copy_dir_recursive(src: &Path, dst: &Path) -> CliResult<()> {
    // Create destination directory
    tokio::fs::create_dir_all(dst).await.map_err(CliError::Io)?;

    let mut entries = tokio::fs::read_dir(src).await.map_err(CliError::Io)?;

    while let Some(entry) = entries.next_entry().await.map_err(CliError::Io)? {
        let ty = entry.file_type().await.map_err(CliError::Io)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // `read_dir`'s file_type does not follow symlinks, so a symlink entry is
        // neither a dir nor a regular file. Reject it (SEC-4) rather than letting
        // it fall into `tokio::fs::copy`, which *follows* the link and copies the
        // target's contents (e.g. `creds -> /etc/passwd` would exfiltrate secrets).
        // This matches the zip extractor's symlink-rejection stance.
        if ty.is_symlink() {
            return Err(CliError::Validation(format!(
                "refusing to copy symlink: {}",
                src_path.display()
            )));
        }

        if ty.is_dir() {
            // Recursively copy subdirectory (boxed to handle async recursion)
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            // Copy file
            tokio::fs::copy(&src_path, &dst_path)
                .await
                .map_err(CliError::Io)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use fastskill_core::{FastSkillService, ServiceConfig};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn recursive_discovery_rejects_a_file_root() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("skill.zip");
        fs::write(&file, "not a directory").unwrap();

        assert!(matches!(
            get_skill_dirs_recursive(&file),
            Err(CliError::InvalidSource(message)) if message.contains("not a directory")
        ));
    }

    // Unix-gated on purpose: builds the test fixture with
    // std::os::unix::fs::symlink. The product code's rejection path
    // (FileType::is_symlink()) is itself cross-platform, but creating a
    // symlink on Windows CI needs Developer Mode or elevated privileges
    // (std::os::windows::fs::symlink_file), which isn't reliably available
    // on hosted windows-latest runners. Left as a follow-up rather than a
    // shaky Windows test — see PR body.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_copy_dir_recursive_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# skill\n").unwrap();

        // A secret file outside the skill, and a symlink pointing at it.
        let secret = tmp.path().join("secret.txt");
        fs::write(&secret, "TOP SECRET").unwrap();
        symlink(&secret, src.join("creds")).unwrap();

        let dst = tmp.path().join("dst");
        let result = copy_dir_recursive(&src, &dst).await;

        match result {
            Err(CliError::Validation(msg)) => {
                assert!(
                    msg.contains("symlink"),
                    "expected symlink rejection, got: {msg}"
                );
            }
            other => unreachable!("expected Validation error, got {other:?}"),
        }

        // The dereferenced secret must NOT have been copied into the destination.
        assert!(
            !dst.join("creds").exists(),
            "symlinked file must not be copied (no content exfiltration)"
        );
    }

    // Not unix-gated: this exercises only tokio::fs directory/file copying,
    // no unix-specific API (unlike the symlink-rejection test above it), so
    // it runs cross-platform, including Windows.
    #[tokio::test]
    async fn test_copy_dir_recursive_copies_regular_tree() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("SKILL.md"), "# skill\n").unwrap();
        fs::write(src.join("nested/file.txt"), "data").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).await.unwrap();

        assert!(dst.join("SKILL.md").exists());
        assert!(dst.join("nested/file.txt").exists());
    }

    fn make_test_skill(dir: &std::path::Path) {
        fs::create_dir_all(dir).unwrap();
        let skill_content = r#"---
name: editable-test-skill
version: 1.0.0
description: A test skill for editable mode testing
---

# Editable Test Skill
"#;
        fs::write(dir.join("SKILL.md"), skill_content).unwrap();
        let skill_project_content = r#"[metadata]
id = "editable-test-skill"
version = "1.0.0"
"#;
        fs::write(dir.join("skill-project.toml"), skill_project_content).unwrap();
    }

    // Unix-gated on purpose: exercises install_skill's editable path, which
    // on Windows calls std::os::windows::fs::symlink_dir — that requires
    // Developer Mode or elevated privileges, not reliably available on
    // hosted windows-latest runners. Left as a follow-up rather than a
    // shaky Windows test — see PR body.
    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_editable_local_creates_symlink() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let source_dir = temp_dir.path().join("editable-skill-source");
        make_test_skill(&source_dir);

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = super::super::AddArgs {
            source: source_dir.display().to_string(),
            source_type: Some("local".to_string()),
            repository: None,
            branch: None,
            tag: None,
            force: false,
            editable: true,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
            offline: false,
            dry_run: false,
            json: false,
        };

        let result = super::super::execute_add(&service, args, false).await;
        assert!(result.is_ok(), "Editable add should succeed: {:?}", result);

        let storage_path = skills_dir.join("editable-test-skill");
        let metadata = fs::symlink_metadata(&storage_path).unwrap();
        assert!(
            metadata.file_type().is_symlink(),
            "Storage path should be a symlink for editable install, but it is not"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_add_non_editable_local_creates_directory() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let source_dir = temp_dir.path().join("non-editable-skill-source");
        make_test_skill(&source_dir);

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = super::super::AddArgs {
            source: source_dir.display().to_string(),
            source_type: Some("local".to_string()),
            repository: None,
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
            offline: false,
            dry_run: false,
            json: false,
        };

        let result = super::super::execute_add(&service, args, false).await;
        assert!(
            result.is_ok(),
            "Non-editable add should succeed: {:?}",
            result
        );

        let storage_path = skills_dir.join("editable-test-skill");
        let metadata = fs::symlink_metadata(&storage_path).unwrap();
        assert!(
            !metadata.file_type().is_symlink(),
            "Storage path should be a real directory for non-editable install, not a symlink"
        );
        assert!(
            storage_path.is_dir(),
            "Storage path should be a real directory"
        );
    }
}

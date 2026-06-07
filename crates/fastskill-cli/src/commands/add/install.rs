//! Unified skill installation function replacing install_local_skill + install_copied_skill.

use crate::error::{CliError, CliResult};
use crate::utils::install_utils;
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

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

/// Shared post-copy logic: update skill_def fields, register, persist metadata and manifest.
pub(super) async fn finish_skill_install(
    ctx: &super::AddContext<'_>,
    mut skill_def: fastskill_core::SkillDefinition,
    storage_dir: &Path,
    meta: super::SourceMeta,
    version_display: &str,
) -> CliResult<()> {
    use chrono::Utc;
    skill_def.skill_file = storage_dir.join("SKILL.md");
    skill_def.source_url = meta.source_url;
    skill_def.source_type = meta.source_type;
    skill_def.source_branch = meta.source_branch;
    skill_def.source_tag = meta.source_tag;
    skill_def.source_subdir = meta.source_subdir;
    skill_def.installed_from = meta.installed_from;
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = ctx.editable;

    super::register_skill_once(ctx, &skill_def).await?;

    let update = fastskill_core::core::skill_manager::SkillUpdate {
        source_url: skill_def.source_url.clone(),
        source_type: skill_def.source_type.clone(),
        source_branch: skill_def.source_branch.clone(),
        source_tag: skill_def.source_tag.clone(),
        source_subdir: skill_def.source_subdir.clone(),
        installed_from: skill_def.installed_from.clone(),
        fetched_at: skill_def.fetched_at,
        editable: Some(skill_def.editable),
        ..Default::default()
    };
    ctx.service
        .skill_manager()
        .update_skill(&skill_def.id, update)
        .await
        .map_err(|e| {
            crate::utils::service_error_to_cli(
                e,
                ctx.service.config().skill_storage_path.as_path(),
                ctx.global,
            )
        })?;

    if ctx.global {
        super::update_global_files(&skill_def)?;
        println!(
            "Successfully added skill: {} (v{})",
            skill_def.name, version_display
        );
        println!(
            "{}",
            crate::utils::messages::ok("Updated global-skills.lock")
        );
    } else {
        super::update_project_files(&skill_def, ctx.groups.clone(), ctx.editable)?;
        println!(
            "Successfully added skill: {} (v{})",
            skill_def.name, version_display
        );
        println!(
            "{}",
            crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
        );
    }
    Ok(())
}

/// Copy skill to storage, then delegate to `finish_skill_install`.
pub(super) async fn install_via_download(
    ctx: &super::AddContext<'_>,
    skill_path: &Path,
    skill_def: fastskill_core::SkillDefinition,
    target: super::InstallTarget,
) -> CliResult<()> {
    if target.storage_dir.exists() {
        if !ctx.force {
            return Err(CliError::Config(format!(
                "Skill directory '{}' already exists. Use --force to overwrite.",
                target.storage_dir.display()
            )));
        }
        tokio::fs::remove_dir_all(&target.storage_dir)
            .await
            .map_err(CliError::Io)?;
    }
    copy_dir_recursive(skill_path, &target.storage_dir).await?;
    finish_skill_install(
        ctx,
        skill_def,
        &target.storage_dir,
        target.meta,
        &target.version_display,
    )
    .await
}

/// Install a local skill using symlink (editable) or copy (non-editable).
pub(super) async fn install_via_local_path(
    ctx: &super::AddContext<'_>,
    skill_path: &Path,
    skill_def: fastskill_core::SkillDefinition,
    target: super::InstallTarget,
) -> CliResult<()> {
    // Check existence (including broken symlinks) before proceeding
    let path_exists = target.storage_dir.exists() || target.storage_dir.is_symlink();
    if path_exists {
        if !ctx.force {
            return Err(CliError::Config(format!(
                "Skill directory '{}' already exists. Use --force to overwrite.",
                target.storage_dir.display()
            )));
        }
        if target.storage_dir.is_symlink() || target.storage_dir.is_file() {
            tokio::fs::remove_file(&target.storage_dir)
                .await
                .map_err(CliError::Io)?;
        } else {
            tokio::fs::remove_dir_all(&target.storage_dir)
                .await
                .map_err(CliError::Io)?;
        }
    }
    install_utils::setup_skill_in_storage(skill_path, &target.storage_dir, ctx.editable).await?;
    finish_skill_install(
        ctx,
        skill_def,
        &target.storage_dir,
        target.meta,
        &target.version_display,
    )
    .await
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

/// Add all skills under a directory (--recursive flag)
pub(super) async fn handle_recursive_add(
    ctx: &super::AddContext<'_>,
    path: &Path,
) -> CliResult<()> {
    info!(
        "Adding skills recursively from directory: {}",
        path.display()
    );
    let skill_dirs = get_skill_dirs_recursive(path)?;
    if skill_dirs.is_empty() {
        return Err(CliError::Validation(format!(
            "No skill directories found under {}",
            path.display()
        )));
    }
    println!(
        "Found {} skill(s) in directory {}",
        skill_dirs.len(),
        path.display()
    );

    let mut failed: Vec<(PathBuf, CliError)> = Vec::new();
    let mut success_count = 0;
    for skill_path in skill_dirs {
        match super::sources::add_from_folder(ctx, &skill_path).await {
            Ok(()) => success_count += 1,
            Err(e) => {
                eprintln!("Skill at {}: {}", skill_path.display(), e);
                failed.push((skill_path, e));
            }
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    let total = success_count + failed.len();
    Err(CliError::Validation(format!(
        "{} of {} skills failed:\n{}",
        failed.len(),
        total,
        failed
            .iter()
            .map(|(path, err)| format!("  - {}: {}", path.display(), err))
            .collect::<Vec<_>>()
            .join("\n")
    )))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use fastskill_core::{FastSkillService, ServiceConfig};
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

    #[cfg(unix)]
    #[tokio::test]
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
            branch: None,
            tag: None,
            force: false,
            editable: true,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
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
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
            reindex: false,
            no_reindex: false,
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

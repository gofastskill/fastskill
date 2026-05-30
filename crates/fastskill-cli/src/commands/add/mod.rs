//! Add command implementation

pub mod install;
pub mod skill_def;
pub mod sources;

// Re-export public API consumed by install_utils.rs
use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::{detect_skill_source, SkillSource};
use chrono::Utc;
use clap::Args;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::core::skill_manager::SourceType;
use fastskill_core::{FastSkillService, SkillDefinition};
pub use install::copy_dir_recursive;
pub use skill_def::create_skill_from_path;
use std::env;
use std::path::{Path, PathBuf};

/// Bundled options for add operations (reduces argument count)
struct AddContext<'a> {
    service: &'a FastSkillService,
    force: bool,
    editable: bool,
    groups: Vec<String>,
    global: bool,
}

/// Source metadata to record after installing a skill
struct SourceMeta {
    source_url: Option<String>,
    source_type: Option<SourceType>,
    source_branch: Option<String>,
    source_tag: Option<String>,
    source_subdir: Option<PathBuf>,
    installed_from: Option<String>,
}

/// Target for install: where to copy and what to record
struct InstallTarget {
    storage_dir: PathBuf,
    meta: SourceMeta,
    version_display: String,
}

fn update_project_files(
    skill_def: &SkillDefinition,
    groups: Vec<String>,
    editable: bool,
) -> CliResult<()> {
    use crate::utils::manifest_utils;
    manifest_utils::add_skill_to_project_toml(skill_def, groups.clone(), editable)
        .map_err(|e| CliError::Config(format!("Failed to update skill-project.toml: {}", e)))?;

    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let lock_path = if let Some(parent) = project_file_result.path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };
    manifest_utils::update_lock_file(&lock_path, skill_def, groups)
        .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;

    Ok(())
}

/// Helper to update global-skills.lock (global scope only)
fn update_global_files(skill_def: &SkillDefinition) -> CliResult<()> {
    use crate::utils::manifest_utils;
    manifest_utils::update_global_lock_file(skill_def, Utc::now())
        .map_err(|e| CliError::Config(format!("Failed to update global lock file: {}", e)))?;
    Ok(())
}

async fn register_skill_once(ctx: &AddContext<'_>, skill_def: &SkillDefinition) -> CliResult<()> {
    if ctx.force {
        ctx.service
            .skill_manager()
            .force_register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    } else {
        if ctx
            .service
            .skill_manager()
            .get_skill(&skill_def.id)
            .await
            .map_err(CliError::Service)?
            .is_some()
        {
            return Err(CliError::Config(format!(
                "Skill '{}' already exists. Use --force to overwrite.",
                skill_def.id
            )));
        }
        ctx.service
            .skill_manager()
            .register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;
    }
    Ok(())
}

/// Add a skill (two operational modes)
///
/// Mode 1: Manifest-managed (when skill-project.toml exists)
/// Mode 2: Local-only (when no skill-project.toml or --global flag)
#[derive(Debug, Args)]
pub struct AddArgs {
    /// Source: path to zip file, folder, git URL, or skill ID (e.g., pptx@1.2.3)
    pub source: String,

    /// Override source type (registry, github/git, local)
    #[arg(long)]
    pub source_type: Option<String>,

    /// Git branch to checkout (only for git URLs)
    #[arg(long)]
    pub branch: Option<String>,

    /// Git tag to checkout (only for git URLs)
    #[arg(long)]
    pub tag: Option<String>,

    /// Force registration even if skill already exists
    #[arg(short, long)]
    pub force: bool,

    /// Install skill in editable mode (for local development, like poetry add -e)
    #[arg(short = 'e', long)]
    pub editable: bool,

    /// Add skill to a specific group (like poetry add --group dev)
    #[arg(long)]
    pub group: Option<String>,

    /// Add all skills found under the directory (only for local folders)
    #[arg(short = 'r', long)]
    pub recursive: bool,
}

fn resolve_source(args: &AddArgs) -> SkillSource {
    let Some(ref source_type) = args.source_type else {
        return detect_skill_source(&args.source);
    };
    match source_type.as_str() {
        "registry" => SkillSource::SkillId(args.source.clone()),
        "github" | "git" => SkillSource::GitUrl(args.source.clone()),
        "local" => {
            let path = PathBuf::from(&args.source);
            if path.extension().and_then(|s| s.to_str()) == Some("zip") {
                SkillSource::ZipFile(path)
            } else {
                SkillSource::Folder(path)
            }
        }
        _ => detect_skill_source(&args.source),
    }
}

fn ensure_manifest() -> CliResult<()> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }
    Ok(())
}

fn validate_folder_has_skill(path: &Path) -> CliResult<()> {
    if path.join("SKILL.md").exists() {
        return Ok(());
    }
    if let Ok(dirs) = install::get_skill_dirs_recursive(path) {
        if !dirs.is_empty() {
            return Err(CliError::Validation(format!(
                "This directory has no SKILL.md at the root but contains {} skill(s) in subdirectories. Add them all with: fastskill add {} --recursive",
                dirs.len(),
                path.display()
            )));
        }
    }
    Err(CliError::Validation(format!(
        "No SKILL.md found in {}. A skill directory must contain a SKILL.md file. If this folder has multiple skills in subdirectories, use --recursive.",
        path.display()
    )))
}

pub async fn execute_add(service: &FastSkillService, args: AddArgs, global: bool) -> CliResult<()> {
    let source = resolve_source(&args);

    if args.editable {
        match &source {
            SkillSource::Folder(_) => {}
            SkillSource::ZipFile(_) | SkillSource::GitUrl(_) | SkillSource::SkillId(_) => {
                return Err(CliError::Config(
                    "--editable (-e) is only supported for local folder sources. \
                     Remove -e or use a local path."
                        .to_string(),
                ));
            }
        }
    }

    if !global {
        ensure_manifest()?;
    }
    let groups = args.group.clone().map(|g| vec![g]).unwrap_or_default();
    let ctx = AddContext {
        service,
        force: args.force,
        editable: args.editable,
        groups,
        global,
    };

    if args.recursive {
        let path = match &source {
            SkillSource::Folder(p) => p,
            _ => {
                return Err(CliError::Config(
                    "Recursive add is only valid when source is a local directory".to_string(),
                ));
            }
        };
        return install::handle_recursive_add(&ctx, path).await;
    }

    match source {
        SkillSource::ZipFile(path) => sources::add_from_zip(&ctx, &path).await,
        SkillSource::Folder(path) => {
            validate_folder_has_skill(&path)?;
            sources::add_from_folder(&ctx, &path).await
        }
        SkillSource::GitUrl(url) => {
            sources::add_from_git(&ctx, &url, args.branch.as_deref(), args.tag.as_deref()).await
        }
        SkillSource::SkillId(skill_id) => sources::add_from_registry(&ctx, &skill_id).await,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock
)]
mod tests {
    use super::*;
    use fastskill_core::{FastSkillService, ServiceConfig};
    use tempfile::TempDir;

    async fn run_add_expect_err(source: &str, source_type: Option<&str>, force: bool) {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        let args = AddArgs {
            source: source.to_string(),
            source_type: source_type.map(String::from),
            branch: None,
            tag: None,
            force,
            editable: false,
            group: None,
            recursive: false,
        };
        let result = execute_add(&service, args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_add_nonexistent_source() {
        run_add_expect_err("/nonexistent/path", Some("local"), false).await;
    }

    #[tokio::test]
    async fn test_execute_add_invalid_skill_id() {
        run_add_expect_err("invalid@skill@id", Some("registry"), false).await;
    }

    #[tokio::test]
    async fn test_execute_add_with_force_flag() {
        run_add_expect_err("/nonexistent/path", Some("local"), true).await;
    }

    #[tokio::test]
    async fn test_execute_add_no_manifest_fails_with_instructions() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let original_dir = std::env::current_dir().ok();
        let _guard = DirGuard(original_dir);
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = AddArgs {
            source: "scope/some-skill".to_string(),
            source_type: Some("registry".to_string()),
            branch: None,
            tag: None,
            force: false,
            editable: false,
            group: None,
            recursive: false,
        };

        let result = execute_add(&service, args, false).await;
        assert!(
            result.is_err(),
            "Expected error when no manifest: {:?}",
            result
        );
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("skill-project.toml not found") && msg.contains("fastskill init"),
                "Error must mention skill-project.toml and fastskill init: '{}'",
                msg
            );
        } else {
            panic!("Expected Config error");
        }
    }
}

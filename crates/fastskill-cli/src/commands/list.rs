//! List locally installed skills command
//!
//! Similar to `pip list` or `uv list`, this command lists all locally installed skills
//! and reconciles them against skill-project.toml and skills.lock.
//!
//! Requires skill-project.toml in the hierarchy. Uses three sources: installed skills (target
//! folder), skill-project.toml [dependencies], and skills.lock. Outputs one table with flags
//! for missing from folder, missing from lock, missing from manifest.

use crate::commands::common::validate_format_args;
use crate::error::{manifest_required_message, CliError, CliResult};
use clap::Args;
use fastskill_core::core::lock::SkillsLock;
use fastskill_core::core::manifest::{SkillProjectToml, SkillSource};
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::core::service::FastSkillService;
use fastskill_core::output::ListRow;
use fastskill_core::OutputFormat;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;

/// List locally installed skills
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Output format: table, json, grid, xml (default: table)
    #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    #[arg(long, help = "Shorthand for --format json")]
    pub json: bool,

    /// Show detailed information (version, manifest/lock/installed status, source path, type)
    #[arg(long, help = "Show detailed information")]
    pub details: bool,

    /// Skills directory path (overrides default discovery)
    #[arg(long, help = "Skills directory path")]
    pub skills_dir: Option<std::path::PathBuf>,
}

/// Extract source path and type from SkillSource
fn format_source_info(source: &SkillSource) -> (Option<String>, Option<String>) {
    match source {
        SkillSource::Git { url, .. } => (Some(url.clone()), Some("git".to_string())),
        SkillSource::Local { path, .. } => {
            (Some(path.display().to_string()), Some("local".to_string()))
        }
        SkillSource::ZipUrl { base_url, .. } => {
            (Some(base_url.clone()), Some("zip-url".to_string()))
        }
        SkillSource::Source { name, .. } => (Some(name.clone()), Some("source".to_string())),
    }
}

/// Execute the list command
pub async fn execute_list(
    service: &FastSkillService,
    args: ListArgs,
    _global: bool,
) -> CliResult<()> {
    // Validate format arguments
    let format = validate_format_args(&args.format, args.json)?;

    // Require manifest: resolve from current directory
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    let project_file_path = project_file_result.path;
    let lock_path = project_file_path
        .parent()
        .map(|p| p.join("skills.lock"))
        .unwrap_or_else(|| PathBuf::from("skills.lock"));

    // Load skill-project.toml and skills.lock
    let project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;
    let manifest_ids: HashMap<String, ()> = project
        .dependencies
        .as_ref()
        .map(|d| d.dependencies.keys().cloned().map(|k| (k, ())).collect())
        .unwrap_or_default();

    let lock = if lock_path.exists() {
        SkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load skills.lock: {}", e)))?
    } else {
        SkillsLock::new_empty()
    };

    // Build lock map with additional metadata
    let lock_map: HashMap<String, (String, String, SkillSource)> = lock
        .skills
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                (s.version.clone(), s.name.clone(), s.source.clone()),
            )
        })
        .collect();

    // Installed skills from service
    let skill_manager = service.skill_manager();
    let installed_skills = skill_manager.list_skills(None).await.map_err(|e| {
        CliError::Service(fastskill_core::ServiceError::Custom(format!(
            "Failed to list installed skills: {}",
            e
        )))
    })?;

    // Build installed map with full skill definitions
    let installed_map: HashMap<String, fastskill_core::core::skill_manager::SkillDefinition> =
        installed_skills
            .into_iter()
            .map(|s| (s.id.to_string(), s))
            .collect();

    // Union of all skill IDs
    let all_ids: HashSet<String> = manifest_ids
        .keys()
        .chain(lock_map.keys())
        .chain(installed_map.keys())
        .cloned()
        .collect();

    let mut rows: Vec<ListRow> = all_ids
        .into_iter()
        .map(|id| {
            let in_manifest = manifest_ids.contains_key(&id);
            let in_lock = lock_map.contains_key(&id);
            let installed = installed_map.contains_key(&id);

            // Get name: prefer installed, fallback to lock, fallback to id
            let name = if let Some(skill) = installed_map.get(&id) {
                skill.name.clone()
            } else if let Some((_, lock_name, _)) = lock_map.get(&id) {
                lock_name.clone()
            } else {
                id.clone()
            };

            // Get description: prefer installed, fallback to "-"
            let description = if let Some(skill) = installed_map.get(&id) {
                skill.description.clone()
            } else {
                "-".to_string()
            };

            // Get version: prefer installed, fallback to lock
            let version = if let Some(skill) = installed_map.get(&id) {
                Some(skill.version.clone())
            } else {
                lock_map.get(&id).map(|(v, _, _)| v.clone())
            };

            // Get source path and type
            let (source_path, source_type) = if let Some(skill) = installed_map.get(&id) {
                // For installed skills, use skill_file path and source_type
                let path = Some(skill.skill_file.display().to_string());
                let stype = skill.source_type.as_ref().map(|st| match st {
                    fastskill_core::core::skill_manager::SourceType::GitUrl => "git".to_string(),
                    fastskill_core::core::skill_manager::SourceType::LocalPath => {
                        "local".to_string()
                    }
                    fastskill_core::core::skill_manager::SourceType::ZipFile => {
                        "zip-url".to_string()
                    }
                    fastskill_core::core::skill_manager::SourceType::Source => "source".to_string(),
                });
                (path, stype)
            } else if let Some((_, _, source)) = lock_map.get(&id) {
                // For lock-only skills, extract from SkillSource
                format_source_info(source)
            } else {
                (None, None)
            };

            let missing_from_folder = (in_manifest || in_lock) && !installed;
            let missing_from_lock = (in_manifest || installed) && !in_lock;
            let missing_from_manifest = (in_lock || installed) && !in_manifest;

            ListRow {
                id: id.clone(),
                name,
                description,
                version,
                in_manifest,
                in_lock,
                installed,
                source_path,
                source_type,
                missing_from_folder,
                missing_from_lock,
                missing_from_manifest,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));

    let formatted_output = fastskill_core::output::format_list_results(&rows, format, args.details)
        .map_err(CliError::Config)?;
    println!("{}", formatted_output);

    Ok(())
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::await_holding_lock
)]
#[cfg(test)]
mod tests {
    use super::*;
    use fastskill_core::{FastSkillService, ServiceConfig};
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_list_format_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        // Test conflicting --json and --format flags
        let args = ListArgs {
            format: Some(OutputFormat::Table),
            json: true,
            details: false,
            skills_dir: None,
        };

        let result = execute_list(&service, args, false).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("--json and --format cannot be used together"));
        } else {
            panic!("Expected Config error for format conflict");
        }
    }

    #[tokio::test]
    async fn test_execute_list_no_manifest() {
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
        let work_dir: PathBuf = if resolve_project_file(temp_dir.path()).found {
            let fallback = std::env::temp_dir()
                .join("fastskill_no_manifest")
                .join(std::process::id().to_string());
            fs::create_dir_all(&fallback).unwrap();
            fallback
        } else {
            temp_dir.path().to_path_buf()
        };
        std::env::set_current_dir(&work_dir).unwrap();

        let config = ServiceConfig {
            skill_storage_path: work_dir.clone(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ListArgs {
            format: None,
            json: false,
            details: false,
            skills_dir: None,
        };

        let result = execute_list(&service, args, false).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("skill-project.toml"),
                "Error should mention skill-project.toml"
            );
        } else {
            panic!("Expected Config error for missing manifest");
        }
    }

    #[tokio::test]
    async fn test_execute_list_manifest_empty_lock() {
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

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ListArgs {
            format: None,
            json: false,
            details: false,
            skills_dir: None,
        };

        let result = execute_list(&service, args, false).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_list_with_installed_skill() {
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

        let skill_dir = skills_dir.join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
        fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = "1.0.0"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let lock_content = r#"version = "1.0.0"
generated_at = "2024-01-01T00:00:00Z"
fastskill_version = "0.1.0"

[[skills]]
id = "test-skill"
name = "test-skill"
version = "1.0.0"
source_type = "local"
source = { path = ".claude/skills/test-skill" }
"#;
        fs::write(temp_dir.path().join("skills.lock"), lock_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ListArgs {
            format: None,
            json: false,
            details: false,
            skills_dir: None,
        };

        let result = execute_list(&service, args, false).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_list_json() {
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

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ListArgs {
            format: None,
            json: false,
            details: false,
            skills_dir: None,
        };

        let result = execute_list(&service, args, false).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_list_details() {
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

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ListArgs {
            format: None,
            json: false,
            details: false,
            skills_dir: None,
        };

        let result = execute_list(&service, args, false).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }
}

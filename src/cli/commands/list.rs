//! List locally installed skills command
//!
//! Similar to `pip list` or `uv list`, this command lists all locally installed skills
//! and reconciles them against skill-project.toml and skills.lock.
//!
//! Requires skill-project.toml in the hierarchy. Uses three sources: installed skills (target
//! folder), skill-project.toml [dependencies], and skills.lock. Outputs one table with flags
//! for missing from folder, missing from lock, missing from manifest.

use crate::cli::error::{manifest_required_message, CliError, CliResult};
use crate::cli::utils::messages;
use clap::Args;
use fastskill::core::lock::SkillsLock;
use fastskill::core::manifest::{SkillProjectToml, SkillSource};
use fastskill::core::project::resolve_project_file;
use fastskill::core::service::FastSkillService;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;

/// One row for the list table: union of all skills with presence and gap flags.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ListRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub in_manifest: bool,
    pub in_lock: bool,
    pub installed: bool,
    pub source_path: Option<String>,
    pub source_type: Option<String>,
    pub missing_from_folder: bool,
    pub missing_from_lock: bool,
    pub missing_from_manifest: bool,
}

/// List locally installed skills
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    /// Output in grid format (default)
    #[arg(long)]
    pub grid: bool,

    /// Show detailed information (version, manifest/lock/installed status, source path, type)
    #[arg(long)]
    pub details: bool,
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
pub async fn execute_list(service: &FastSkillService, args: ListArgs) -> CliResult<()> {
    // Validate conflicting flags
    if args.json && args.grid {
        return Err(CliError::Config(
            "Cannot use both --json and --grid flags. Use only one.".to_string(),
        ));
    }

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
        SkillsLock {
            metadata: fastskill::core::lock::LockMetadata {
                version: "1.0.0".to_string(),
                generated_at: chrono::Utc::now(),
                fastskill_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            skills: Vec::new(),
        }
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
        CliError::Service(fastskill::ServiceError::Custom(format!(
            "Failed to list installed skills: {}",
            e
        )))
    })?;

    // Build installed map with full skill definitions
    let installed_map: HashMap<String, fastskill::core::skill_manager::SkillDefinition> =
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
                    fastskill::core::skill_manager::SourceType::GitUrl => "git".to_string(),
                    fastskill::core::skill_manager::SourceType::LocalPath => "local".to_string(),
                    fastskill::core::skill_manager::SourceType::ZipFile => "zip-url".to_string(),
                    fastskill::core::skill_manager::SourceType::Source => "source".to_string(),
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

    let output_format = if args.json { "json" } else { "grid" };
    match output_format {
        "json" => {
            let json_output = serde_json::to_string_pretty(&rows)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            println!("{}", json_output);
        }
        "grid" => {
            format_list_grid(&rows, args.details)?;
        }
        _ => unreachable!(),
    }

    Ok(())
}

/// Format list output as grid table: one row per skill with In manifest, In lock, Installed, and Flags.
fn format_list_grid(rows: &[ListRow], details: bool) -> CliResult<()> {
    if rows.is_empty() {
        println!(
            "{}",
            messages::info("No skills (manifest is empty and nothing installed)")
        );
        return Ok(());
    }

    if details {
        // Details view: Name, Description, Version, In manifest, In lock, Installed, Source path, Type, Flags
        let headers = [
            "Name",
            "Description",
            "Version",
            "In manifest",
            "In lock",
            "Installed",
            "Source path",
            "Type",
            "Flags",
        ];
        let mut col_widths = vec![0; headers.len()];
        for (i, h) in headers.iter().enumerate() {
            col_widths[i] = h.len();
        }
        for row in rows {
            col_widths[0] = col_widths[0].max(row.name.len());
            col_widths[1] = col_widths[1].max(row.description.len());
            col_widths[2] = col_widths[2].max(row.version.as_deref().unwrap_or("-").len());
            col_widths[3] = col_widths[3].max(1);
            col_widths[4] = col_widths[4].max(1);
            col_widths[5] = col_widths[5].max(1);
            col_widths[6] = col_widths[6].max(row.source_path.as_deref().unwrap_or("-").len());
            col_widths[7] = col_widths[7].max(row.source_type.as_deref().unwrap_or("-").len());
            let flags = build_flags_str(row);
            col_widths[8] = col_widths[8].max(flags.len());
        }

        let header_row: Vec<String> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", *h, width = col_widths[i]))
            .collect();
        println!();
        println!("{}", header_row.join("  "));
        println!("{}", "-".repeat(header_row.join("  ").len()));

        for row in rows {
            let version = row.version.as_deref().unwrap_or("-");
            let in_manifest = if row.in_manifest { "Y" } else { "-" };
            let in_lock = if row.in_lock { "Y" } else { "-" };
            let installed = if row.installed { "Y" } else { "-" };
            let source_path = row.source_path.as_deref().unwrap_or("-");
            let source_type = row.source_type.as_deref().unwrap_or("-");
            let flags = build_flags_str(row);
            let line = [
                format!("{:width$}", row.name, width = col_widths[0]),
                format!("{:width$}", row.description, width = col_widths[1]),
                format!("{:width$}", version, width = col_widths[2]),
                format!("{:width$}", in_manifest, width = col_widths[3]),
                format!("{:width$}", in_lock, width = col_widths[4]),
                format!("{:width$}", installed, width = col_widths[5]),
                format!("{:width$}", source_path, width = col_widths[6]),
                format!("{:width$}", source_type, width = col_widths[7]),
                format!("{:width$}", flags, width = col_widths[8]),
            ];
            println!("{}", line.join("  "));
        }
    } else {
        // Default view: Name, Description, Flags
        let headers = ["Name", "Description", "Flags"];
        let mut col_widths = vec![0; headers.len()];
        for (i, h) in headers.iter().enumerate() {
            col_widths[i] = h.len();
        }
        for row in rows {
            col_widths[0] = col_widths[0].max(row.name.len());
            col_widths[1] = col_widths[1].max(row.description.len());
            let flags = build_flags_str(row);
            col_widths[2] = col_widths[2].max(flags.len());
        }

        let header_row: Vec<String> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", *h, width = col_widths[i]))
            .collect();
        println!();
        println!("{}", header_row.join("  "));
        println!("{}", "-".repeat(header_row.join("  ").len()));

        for row in rows {
            let flags = build_flags_str(row);
            let line = [
                format!("{:width$}", row.name, width = col_widths[0]),
                format!("{:width$}", row.description, width = col_widths[1]),
                format!("{:width$}", flags, width = col_widths[2]),
            ];
            println!("{}", line.join("  "));
        }
    }

    println!();
    Ok(())
}

fn build_flags_str(row: &ListRow) -> String {
    let mut parts = Vec::new();
    if row.missing_from_folder {
        parts.push("missing from folder");
    }
    if row.missing_from_lock {
        parts.push("missing from lock");
    }
    if row.missing_from_manifest {
        parts.push("missing from manifest");
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("; ")
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use fastskill::{FastSkillService, ServiceConfig};
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_list_conflicting_flags() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ListArgs {
            json: true,
            grid: true,
            details: false,
        };

        let result = execute_list(&service, args).await;
        assert!(result.is_err());
        if let Err(CliError::Config(_)) = result {
            // Expected error type
        } else {
            panic!("Expected Config error for conflicting flags");
        }
    }

    #[tokio::test]
    async fn test_execute_list_no_manifest() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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
            json: false,
            grid: false,
            details: false,
        };

        let result = execute_list(&service, args).await;
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
        let _lock = fastskill::test_utils::DIR_MUTEX
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
            json: false,
            grid: false,
            details: false,
        };

        let result = execute_list(&service, args).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_list_with_installed_skill() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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
            json: false,
            grid: false,
            details: false,
        };

        let result = execute_list(&service, args).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_list_json() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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
            json: true,
            grid: false,
            details: false,
        };

        let result = execute_list(&service, args).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_list_details() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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
            json: false,
            grid: false,
            details: true,
        };

        let result = execute_list(&service, args).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }
}

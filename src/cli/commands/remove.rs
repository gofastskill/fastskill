//! Remove command implementation

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::manifest_utils;
use clap::Args;
use fastskill::core::project::resolve_project_file;
use fastskill::FastSkillService;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::fs;

/// Remove skills and update skill-project.toml [dependencies]
///
/// Removes skills from both the installation and skill-project.toml [dependencies].
/// Updates skills.lock to reflect removals.
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Skill IDs to remove
    pub skill_ids: Vec<String>,

    /// Force removal without confirmation
    #[arg(short, long)]
    pub force: bool,
}

pub async fn execute_remove(service: &FastSkillService, args: RemoveArgs) -> CliResult<()> {
    if args.skill_ids.is_empty() {
        return Err(CliError::Config("No skill IDs provided".to_string()));
    }

    // Check that all skills exist before asking for confirmation
    let mut nonexistent_skills = Vec::new();
    for skill_id in &args.skill_ids {
        let skill_id_parsed = fastskill::SkillId::new(skill_id.clone())
            .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;

        // Check if skill exists in registry
        let exists_in_registry = service
            .skill_manager()
            .get_skill(&skill_id_parsed)
            .await
            .map(|opt| opt.is_some())
            .unwrap_or(false);

        // Check if skill exists in filesystem
        let exists_in_filesystem = (|| {
            let skill_dir = service.config().skill_storage_path.join(skill_id);
            if skill_dir.exists() && skill_dir.join("SKILL.md").exists() {
                return true;
            }
            // Search in subdirectories (handles nested directory structures)
            if let Ok(entries) = std::fs::read_dir(&service.config().skill_storage_path) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_dir() {
                            let candidate_dir = entry.path().join(skill_id);
                            if candidate_dir.exists() && candidate_dir.join("SKILL.md").exists() {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        })();

        if !exists_in_registry && !exists_in_filesystem {
            nonexistent_skills.push(skill_id.clone());
        }
    }

    // Return error if any skills don't exist (before showing confirmation)
    if !nonexistent_skills.is_empty() {
        let error_msg = if nonexistent_skills.len() == 1 {
            format!("Skill '{}' not found", nonexistent_skills[0])
        } else {
            format!("Skills not found: {}", nonexistent_skills.join(", "))
        };
        return Err(CliError::Validation(error_msg));
    }

    // Confirm removal unless force flag is set (only for existing skills)
    if !args.force {
        println!("Warning: This will permanently remove the following skills:");
        for skill_id in &args.skill_ids {
            println!("  - {}", skill_id);
        }
        print!("Are you sure you want to continue? (y/n): ");
        io::stdout()
            .flush()
            .map_err(|e| CliError::Config(format!("Failed to flush stdout: {}", e)))?;

        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(CliError::Io)?;

        let trimmed = input.trim().to_lowercase();
        if trimmed != "yes" && trimmed != "y" {
            println!("Removal cancelled.");
            return Ok(());
        }
    }

    // Remove skills
    let mut removed_count = 0;
    for skill_id in &args.skill_ids {
        let skill_id_parsed = fastskill::SkillId::new(skill_id.clone())
            .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;
        // Try to unregister skill (don't fail if not found)
        match service
            .skill_manager()
            .unregister_skill(&skill_id_parsed)
            .await
        {
            Ok(_) => {}
            Err(fastskill::ServiceError::SkillNotFound(_)) => {
                // Skill not in registry, but that's okay - we still want to clean up files
            }
            Err(e) => {
                return Err(CliError::Service(fastskill::ServiceError::Custom(format!(
                    "Failed to remove skill {}: {}",
                    skill_id, e
                ))));
            }
        }

        // Delete skill directory from filesystem
        // First try the expected path
        let mut skill_dir = service.config().skill_storage_path.join(skill_id);
        if !skill_dir.exists() {
            // If not found, search for it in subdirectories (handles nested directory structures)
            if let Ok(entries) = std::fs::read_dir(&service.config().skill_storage_path) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_dir() {
                            let candidate_dir = entry.path().join(skill_id);
                            if candidate_dir.exists() && candidate_dir.join("SKILL.md").exists() {
                                skill_dir = candidate_dir;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if skill_dir.exists() {
            fs::remove_dir_all(&skill_dir).await.map_err(|e| {
                CliError::Io(std::io::Error::other(format!(
                    "Failed to delete skill directory {}: {}",
                    skill_dir.display(),
                    e
                )))
            })?;
        }

        // T029: Remove from skill-project.toml
        manifest_utils::remove_skill_from_project_toml(skill_id)
            .map_err(|e| CliError::Config(format!("Failed to update skill-project.toml: {}", e)))?;

        // T033: Remove from lock file at project root
        let current_dir = env::current_dir()
            .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
        let project_file_result = resolve_project_file(&current_dir);
        let lock_path = if let Some(parent) = project_file_result.path.parent() {
            parent.join("skills.lock")
        } else {
            PathBuf::from("skills.lock")
        };
        if lock_path.exists() {
            let mut lock = fastskill::core::lock::SkillsLock::load_from_file(&lock_path)
                .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?;
            lock.remove_skill(skill_id);
            lock.save_to_file(&lock_path)
                .map_err(|e| CliError::Config(format!("Failed to save lock file: {}", e)))?;
        }

        println!("Removed skill: {}", skill_id);
        removed_count += 1;
    }

    if removed_count > 0 {
        println!(
            "{}",
            crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
        );
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use fastskill::ServiceConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_remove_empty_args() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec![],
            force: false,
        };

        let result = execute_remove(&service, args).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("No skill IDs provided"));
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_execute_remove_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["invalid@skill@id".to_string()],
            force: true,
        };

        let result = execute_remove(&service, args).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("Invalid skill ID format"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_execute_remove_nonexistent_skill() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["nonexistent@1.0.0".to_string()],
            force: true,
        };

        // This should fail because the skill doesn't exist
        let result = execute_remove(&service, args).await;
        assert!(result.is_err());
    }
}

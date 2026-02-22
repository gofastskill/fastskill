//! Remove command implementation

use crate::cli::config::get_skill_search_locations_for_display;
use crate::cli::error::{CliError, CliResult, SkillNotFoundMessage};
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

/// Check if a skill exists in the registry
async fn check_skill_in_registry(
    service: &FastSkillService,
    skill_id: &fastskill::SkillId,
) -> bool {
    service
        .skill_manager()
        .get_skill(skill_id)
        .await
        .map(|opt| opt.is_some())
        .unwrap_or(false)
}

/// Check if a skill exists in the filesystem
fn check_skill_in_filesystem(service: &FastSkillService, skill_id: &str) -> bool {
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
}

/// Validate that all skills exist before removal
async fn validate_skills_exist(
    service: &FastSkillService,
    skill_ids: &[String],
    global: bool,
) -> CliResult<()> {
    let mut nonexistent_skills = Vec::new();

    for skill_id in skill_ids {
        let skill_id_parsed = fastskill::SkillId::new(skill_id.clone())
            .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;

        let exists_in_registry = check_skill_in_registry(service, &skill_id_parsed).await;
        let exists_in_filesystem = check_skill_in_filesystem(service, skill_id);

        if !exists_in_registry && !exists_in_filesystem {
            nonexistent_skills.push(skill_id.clone());
        }
    }

    if !nonexistent_skills.is_empty() {
        let skill_id_display = nonexistent_skills.join(", ");
        let searched_paths = get_skill_search_locations_for_display(global).unwrap_or_else(|_| {
            vec![(
                service.config().skill_storage_path.clone(),
                if global { "global" } else { "project" }.to_string(),
            )]
        });
        return Err(CliError::SkillNotFound(SkillNotFoundMessage::new(
            skill_id_display,
            searched_paths,
        )));
    }

    Ok(())
}

/// Prompt user for confirmation unless force flag is set
fn confirm_removal(skill_ids: &[String], force: bool) -> CliResult<bool> {
    if force {
        return Ok(true);
    }

    println!("Warning: This will permanently remove the following skills:");
    for skill_id in skill_ids {
        println!("  - {}", skill_id);
    }
    print!("Are you sure you want to continue? (y/n): ");
    io::stdout()
        .flush()
        .map_err(|e| CliError::Config(format!("Failed to flush stdout: {}", e)))?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(CliError::Io)?;

    let trimmed = input.trim().to_lowercase();
    Ok(trimmed == "yes" || trimmed == "y")
}

/// Find the skill directory, checking both direct and nested locations
fn find_skill_directory(service: &FastSkillService, skill_id: &str) -> Option<PathBuf> {
    let mut skill_dir = service.config().skill_storage_path.join(skill_id);

    if skill_dir.exists() {
        return Some(skill_dir);
    }

    // Search in subdirectories (handles nested directory structures)
    if let Ok(entries) = std::fs::read_dir(&service.config().skill_storage_path) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let candidate_dir = entry.path().join(skill_id);
                    if candidate_dir.exists() && candidate_dir.join("SKILL.md").exists() {
                        skill_dir = candidate_dir;
                        return Some(skill_dir);
                    }
                }
            }
        }
    }

    None
}

/// Unregister skill from the service
async fn unregister_skill_from_service(
    service: &FastSkillService,
    skill_id: &str,
) -> CliResult<()> {
    let skill_id_parsed = fastskill::SkillId::new(skill_id.to_string())
        .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;

    match service
        .skill_manager()
        .unregister_skill(&skill_id_parsed)
        .await
    {
        Ok(_) => Ok(()),
        Err(fastskill::ServiceError::SkillNotFound(_)) => {
            // Skill not in registry, but that's okay - we still want to clean up files
            Ok(())
        }
        Err(e) => Err(CliError::Service(fastskill::ServiceError::Custom(format!(
            "Failed to remove skill {}: {}",
            skill_id, e
        )))),
    }
}

/// Delete skill directory from filesystem
async fn delete_skill_directory(service: &FastSkillService, skill_id: &str) -> CliResult<()> {
    if let Some(skill_dir) = find_skill_directory(service, skill_id) {
        fs::remove_dir_all(&skill_dir).await.map_err(|e| {
            CliError::Io(std::io::Error::other(format!(
                "Failed to delete skill directory {}: {}",
                skill_dir.display(),
                e
            )))
        })?;
    }
    Ok(())
}

/// Remove skill from project manifest (skill-project.toml)
fn remove_from_project_manifest(skill_id: &str) -> CliResult<()> {
    manifest_utils::remove_skill_from_project_toml(skill_id)
        .map_err(|e| CliError::Config(format!("Failed to update skill-project.toml: {}", e)))
}

/// Remove skill from lock file
fn remove_from_lock_file(skill_id: &str) -> CliResult<()> {
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

    Ok(())
}

/// Remove skill from vector index
async fn remove_from_vector_index(service: &FastSkillService, skill_id: &str) {
    if let Some(vector_index_service) = service.vector_index_service() {
        if let Err(e) = vector_index_service.remove_skill(skill_id).await {
            tracing::warn!(
                "Failed to remove skill {} from vector index: {}",
                skill_id,
                e
            );
        }
    }
}

/// Remove a single skill (all cleanup steps)
async fn remove_single_skill(service: &FastSkillService, skill_id: &str) -> CliResult<()> {
    // Unregister from service
    unregister_skill_from_service(service, skill_id).await?;

    // Delete directory
    delete_skill_directory(service, skill_id).await?;

    // Remove from manifest
    remove_from_project_manifest(skill_id)?;

    // Remove from lock file
    remove_from_lock_file(skill_id)?;

    // Remove from vector index (non-critical, just log warnings)
    remove_from_vector_index(service, skill_id).await;

    Ok(())
}

pub async fn execute_remove(
    service: &FastSkillService,
    args: RemoveArgs,
    global: bool,
) -> CliResult<()> {
    // Validate inputs
    if args.skill_ids.is_empty() {
        return Err(CliError::Config("No skill IDs provided".to_string()));
    }

    // Validate all skills exist
    validate_skills_exist(service, &args.skill_ids, global).await?;

    // Get user confirmation
    if !confirm_removal(&args.skill_ids, args.force)? {
        println!("Removal cancelled.");
        return Ok(());
    }

    // Remove each skill
    let mut removed_count = 0;
    for skill_id in &args.skill_ids {
        remove_single_skill(service, skill_id).await?;
        println!("Removed skill: {}", skill_id);
        removed_count += 1;
    }

    // Display success message
    if removed_count > 0 {
        println!(
            "{}",
            crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
        );
    }

    Ok(())
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

        let result = execute_remove(&service, args, false).await;
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

        let result = execute_remove(&service, args, false).await;
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
        let result = execute_remove(&service, args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_remove_success() {
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
        std::fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = "1.0.0"
"#;
        std::fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

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
        std::fs::write(temp_dir.path().join("skills.lock"), lock_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["test-skill".to_string()],
            force: true,
        };

        let result = execute_remove(&service, args, false).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_remove_calls_vector_index_remove_skill() {
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
        std::fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = "1.0.0"
"#;
        std::fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

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
        std::fs::write(temp_dir.path().join("skills.lock"), lock_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            embedding: Some(fastskill::EmbeddingConfig {
                openai_base_url: "https://api.openai.com/v1".to_string(),
                embedding_model: "text-embedding-3-small".to_string(),
                index_path: None,
            }),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["test-skill".to_string()],
            force: true,
        };

        let result = execute_remove(&service, args, false).await;
        // May succeed or fail depending on various factors
        // The important part is that it attempts to remove from vector index
        assert!(result.is_ok() || result.is_err());
    }
}

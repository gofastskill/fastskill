//! Remove command implementation

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::manifest_utils;
use clap::Args;
use fastskill::core::project::resolve_project_file;
use fastskill::FastSkillService;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

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

    // Confirm removal unless force flag is set
    if !args.force {
        println!("Warning: This will permanently remove the following skills:");
        for skill_id in &args.skill_ids {
            println!("  - {}", skill_id);
        }
        print!("Are you sure you want to continue? (yes/no): ");
        io::stdout()
            .flush()
            .map_err(|e| CliError::Config(format!("Failed to flush stdout: {}", e)))?;

        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(CliError::Io)?;

        if input.trim().to_lowercase() != "yes" {
            println!("Removal cancelled.");
            return Ok(());
        }
    }

    // Remove skills
    for skill_id in &args.skill_ids {
        let skill_id_parsed = fastskill::SkillId::new(skill_id.clone())
            .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;
        service.skill_manager().unregister_skill(&skill_id_parsed).await.map_err(|e| {
            CliError::Service(fastskill::ServiceError::Custom(format!(
                "Failed to remove skill {}: {}",
                skill_id, e
            )))
        })?;

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
    }

    println!(
        "{}",
        crate::cli::utils::messages::ok("Updated skill-project.toml and skills.lock")
    );
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

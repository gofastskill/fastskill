//! Disable command implementation

use crate::error::{CliError, CliResult};
use clap::Args;
use fastskill_core::FastSkillService;

/// Disable skills by ID
#[derive(Debug, Args)]
pub struct DisableArgs {
    /// Skill IDs to disable
    pub skill_ids: Vec<String>,
}

pub async fn execute_disable(service: &FastSkillService, args: DisableArgs) -> CliResult<()> {
    if args.skill_ids.is_empty() {
        return Err(CliError::Config("No skill IDs provided".to_string()));
    }

    for skill_id in &args.skill_ids {
        let skill_id_parsed = fastskill_core::SkillId::new(skill_id.clone())
            .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;
        service
            .skill_manager()
            .disable_skill(&skill_id_parsed)
            .await
            .map_err(|e| {
                CliError::Service(fastskill_core::ServiceError::Custom(format!(
                    "Failed to disable skill {}: {}",
                    skill_id, e
                )))
            })?;
        println!("Disabled skill: {}", skill_id);
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use fastskill_core::ServiceConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_disable_empty_args() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = DisableArgs { skill_ids: vec![] };

        let result = execute_disable(&service, args).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("No skill IDs provided"));
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_execute_disable_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = DisableArgs {
            skill_ids: vec!["invalid@skill@id".to_string()],
        };

        let result = execute_disable(&service, args).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("Invalid skill ID format"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_execute_disable_nonexistent_skill() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = DisableArgs {
            skill_ids: vec!["nonexistent@1.0.0".to_string()],
        };

        // This should fail because the skill doesn't exist
        let result = execute_disable(&service, args).await;
        assert!(result.is_err());
    }
}

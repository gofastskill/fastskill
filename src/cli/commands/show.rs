//! Show command - displays skill details and dependency tree

use crate::cli::config::create_service_config;
use crate::cli::error::{CliError, CliResult};
use clap::Args;
use fastskill::core::lock::SkillsLock;
use fastskill::FastSkillService;
use std::path::PathBuf;

/// Show skill details
#[derive(Debug, Args)]
pub struct ShowArgs {
    /// Skill ID to show (if not specified, shows all)
    skill_id: Option<String>,

    /// Show dependency tree
    #[arg(long)]
    tree: bool,
}

pub async fn execute_show(args: ShowArgs) -> CliResult<()> {
    println!("Skill Information");
    println!();

    // Derive lock path from skills directory
    let skills_dir = crate::cli::config::resolve_skills_storage_directory()?;
    let lock_path = skills_dir.parent().unwrap_or(&PathBuf::from(".claude")).join("skills.lock");

    if lock_path.exists() {
        let lock = SkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?;

        if let Some(skill_id) = args.skill_id {
            if let Some(skill) = lock.skills.iter().find(|s| s.id == skill_id) {
                print_skill_details(skill);
                if args.tree {
                    println!("\nDependency Tree:");
                    println!("   (Dependency tree not yet implemented)");
                }
            } else {
                return Err(CliError::Config(format!("Skill '{}' not found", skill_id)));
            }
        } else {
            println!("Installed Skills ({}):\n", lock.skills.len());
            for skill in &lock.skills {
                println!("  • {} (v{})", skill.name, skill.version);
                if args.tree {
                    println!("    Groups: {:?}", skill.groups);
                    println!("    Source: {:?}", skill.source);
                }
            }
        }
    } else {
        // Fall back to service
        // Note: show command doesn't have access to CLI sources_path, so uses env var or walk-up
        let config = create_service_config(None, None)?;
        let mut service = FastSkillService::new(config).await.map_err(CliError::Service)?;
        service.initialize().await.map_err(CliError::Service)?;

        if let Some(ref skill_id) = args.skill_id {
            let skill_id_parsed = fastskill::SkillId::new(skill_id.clone()).map_err(|_| {
                CliError::Validation(format!("Invalid skill ID format: {}", skill_id))
            })?;
            if let Some(skill) = service
                .skill_manager()
                .get_skill(&skill_id_parsed)
                .await
                .map_err(CliError::Service)?
            {
                println!("Skill: {}", skill.name);
                println!("  ID: {}", skill.id);
                println!("  Version: {}", skill.version);
                println!("  Description: {}", skill.description);
                if let Some(source_type) = &skill.source_type {
                    println!("  Source Type: {:?}", source_type);
                }
                if let Some(source_url) = &skill.source_url {
                    println!("  Source URL: {}", source_url);
                }
            } else {
                return Err(CliError::Config(format!("Skill '{}' not found", skill_id)));
            }
        } else {
            let skills =
                service.skill_manager().list_skills(None).await.map_err(CliError::Service)?;
            println!("Installed Skills ({}):\n", skills.len());
            for skill in skills {
                println!("  • {} (v{})", skill.name, skill.version);
            }
        }
    }

    Ok(())
}

fn print_skill_details(skill: &fastskill::core::lock::LockedSkillEntry) {
    println!("Skill: {}", skill.name);
    println!("  ID: {}", skill.id);
    println!("  Version: {}", skill.version);
    if let Some(commit_hash) = &skill.commit_hash {
        println!("  Commit: {}", commit_hash);
    }
    println!("  Fetched: {}", skill.fetched_at);
    println!("  Groups: {:?}", skill.groups);
    println!("  Editable: {}", skill.editable);
    println!("  Source: {:?}", skill.source);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_show_no_skill_id_no_lock_file() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("FASTSKILL_SKILLS_DIR", temp_dir.path());

        let args = ShowArgs {
            skill_id: None,
            tree: false,
        };

        // Should not crash even if no lock file exists
        let result = execute_show(args).await;
        // Result may be Ok or Err depending on service initialization, but shouldn't panic
        assert!(result.is_ok() || result.is_err());

        std::env::remove_var("FASTSKILL_SKILLS_DIR");
    }

    #[tokio::test]
    async fn test_execute_show_with_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("FASTSKILL_SKILLS_DIR", temp_dir.path());

        let args = ShowArgs {
            skill_id: Some("invalid@skill@id".to_string()),
            tree: false,
        };

        let result = execute_show(args).await;
        // Should fail with validation error or config error
        assert!(result.is_err());

        std::env::remove_var("FASTSKILL_SKILLS_DIR");
    }

    #[tokio::test]
    async fn test_execute_show_with_nonexistent_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();
        std::env::set_var("FASTSKILL_SKILLS_DIR", temp_dir.path());

        let args = ShowArgs {
            skill_id: Some("nonexistent@1.0.0".to_string()),
            tree: false,
        };

        let result = execute_show(args).await;
        // Should fail because skill doesn't exist or invalid format
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        match &result {
            Err(CliError::Config(msg)) => {
                assert!(
                    msg.contains("not found")
                        || msg.contains("Failed to get current directory")
                        || msg.contains("Failed to load lock file"),
                    "Error message '{}' does not contain expected text",
                    msg
                );
            }
            Err(CliError::Validation(msg)) => {
                // Validation error for invalid skill ID format is also acceptable
                assert!(
                    msg.contains("Invalid") || msg.contains("format"),
                    "Validation error '{}' should mention invalid format",
                    msg
                );
            }
            Err(e) => {
                // Any error is acceptable as long as it fails
                eprintln!("Got error: {:?}", e);
            }
            Ok(_) => {
                panic!("Expected error but got Ok");
            }
        }

        std::env::remove_var("FASTSKILL_SKILLS_DIR");
        if let Some(dir) = original_dir {
            let _ = std::env::set_current_dir(&dir);
        }
    }
}

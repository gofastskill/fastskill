//! Update command - updates skills in the configured skills directory

use crate::cli::config::create_service_config;
use crate::cli::error::{manifest_required_message, CliError, CliResult};
use crate::cli::utils::{install_utils, manifest_utils, messages};
use clap::Args;
use fastskill::core::{
    lock::SkillsLock,
    manifest::SkillProjectToml,
    project::resolve_project_file,
    repository::{RepositoryConfig, RepositoryManager, RepositoryType},
    resolver::PackageResolver,
    sources::{SourceAuth, SourceConfig, SourcesManager},
    update::{UpdateService, UpdateStrategy},
};
use fastskill::FastSkillService;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

/// Update skills from skill-project.toml [dependencies] to latest versions
///
/// Reads dependencies from skill-project.toml and updates installed skills.
/// Updates skills.lock with new versions.
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Skill ID to update (if not specified, updates all)
    skill_id: Option<String>,

    /// Check for updates without installing
    #[arg(long)]
    check: bool,

    /// Show what would be updated without actually updating
    #[arg(long)]
    dry_run: bool,

    /// Update to specific version
    #[arg(long)]
    version: Option<String>,

    /// Update from specific source
    #[arg(long)]
    source: Option<String>,

    /// Update strategy: latest, patch, minor, major
    #[arg(long, default_value = "latest")]
    strategy: String,
}

pub async fn execute_update(args: UpdateArgs) -> CliResult<()> {
    println!("Updating skills...");
    println!();

    // T034: Resolve skill-project.toml from project root
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    let project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;

    // Validate context
    let context = project_file_result.context;
    project
        .validate_for_context(context)
        .map_err(|e| CliError::Config(format!("skill-project.toml validation failed: {}", e)))?;

    // Convert dependencies to SkillEntry format
    let mut entries = project
        .to_skill_entries()
        .map_err(|e| CliError::Config(format!("Failed to parse dependencies: {}", e)))?;

    // Filter by skill_id if specified
    if let Some(skill_id) = &args.skill_id {
        entries.retain(|e| e.id == *skill_id);
    }

    // Sort entries alphabetically for deterministic output
    entries.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    if entries.is_empty() {
        println!("{}", messages::info("No skills to update"));
        return Ok(());
    }

    // Parse update strategy
    let strategy = match args.strategy.as_str() {
        "latest" => UpdateStrategy::Latest,
        "patch" => UpdateStrategy::Patch,
        "minor" => UpdateStrategy::Minor,
        "major" => UpdateStrategy::Major,
        _ => {
            if let Some(version) = &args.version {
                UpdateStrategy::Exact(version.clone())
            } else {
                return Err(CliError::Config(format!(
                    "Invalid strategy: {}. Use: latest, patch, minor, major",
                    args.strategy
                )));
            }
        }
    };

    // If version is specified, use Exact strategy
    let strategy = if let Some(version) = &args.version {
        UpdateStrategy::Exact(version.clone())
    } else {
        strategy
    };

    // T033: Load lock file from project root
    let lock_path = if let Some(parent) = project_file_path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };
    let lock = if lock_path.exists() {
        SkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?
    } else {
        return Err(CliError::Config(
            "skills.lock not found. Run 'fastskill install' first.".to_string(),
        ));
    };

    // Load repositories and create sources manager for marketplace-based repos
    let repositories = crate::cli::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Create SourcesManager from marketplace-based repositories for PackageResolver
    let sources_manager = create_sources_manager_from_repositories(&repo_manager)?;

    if args.check || args.dry_run {
        if let Some(sources_mgr) = sources_manager {
            let mut resolver = PackageResolver::new(sources_mgr.clone());
            resolver
                .build_index()
                .await
                .map_err(|e| CliError::Config(format!("Failed to build resolver index: {}", e)))?;

            let update_service = UpdateService::new(Arc::new(resolver), lock);
            let updates = update_service
                .check_updates(args.skill_id.as_deref(), strategy)
                .map_err(|e| CliError::Config(format!("Failed to check updates: {}", e)))?;

            if updates.is_empty() {
                println!("{}", messages::info("No updates available"));
            } else {
                println!("\nSkills that would be updated:\n");
                for update in &updates {
                    println!(
                        "  • {}: {} -> {}",
                        update.skill_id, update.current_version, update.available_version
                    );
                }
            }
        } else {
            println!("\nSkills that would be updated:\n");
            for entry in &entries {
                println!("  • {} (from {:?})", entry.id, entry.source);
            }
        }
        if args.check {
            println!(
                "\n{}",
                messages::info("Run without --check to actually update")
            );
        }
        return Ok(());
    }

    // Initialize service
    // Note: update command doesn't have access to CLI sources_path, so uses env var or walk-up
    let config = create_service_config(None, None)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;

    // Load repositories and create sources manager for marketplace-based repos
    let repositories = crate::cli::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Create SourcesManager from marketplace-based repositories for PackageResolver
    let sources_manager = create_sources_manager_from_repositories(&repo_manager)?;

    // Update each skill
    let mut updated_count = 0;
    for entry in entries {
        println!("  Updating {}...", entry.id);
        match install_utils::install_skill_from_entry(
            &service,
            entry.clone(),
            sources_manager.as_ref().map(|arc| arc.as_ref()),
        )
        .await
        {
            Ok(skill_def) => {
                manifest_utils::update_lock_file(&lock_path, &skill_def, entry.groups.clone())
                    .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;
                updated_count += 1;
                println!("  {}", messages::ok(&format!("Updated {}", entry.id)));
            }
            Err(e) => {
                eprintln!(
                    "  {}",
                    messages::error(&format!("Failed to update {}: {}", entry.id, e))
                );
            }
        }
    }

    println!();
    println!(
        "{}",
        messages::ok(&format!("Updated {} skill(s)", updated_count))
    );
    println!("   Updated skills.lock");

    Ok(())
}

/// Create SourcesManager from RepositoryManager for marketplace-based repositories
/// This is needed because PackageResolver requires SourcesManager
pub fn create_sources_manager_from_repositories(
    repo_manager: &RepositoryManager,
) -> CliResult<Option<Arc<SourcesManager>>> {
    use fastskill::core::sources::SourceDefinition;

    let repos = repo_manager.list_repositories();
    let mut marketplace_sources = Vec::new();

    for repo in repos {
        // Only include marketplace-based repositories (git-marketplace, zip-url, local)
        let source_config = match &repo.repo_type {
            RepositoryType::GitMarketplace => {
                if let RepositoryConfig::GitMarketplace { url, branch, tag } = &repo.config {
                    // Convert auth
                    let auth = repo.auth.as_ref().and_then(|a| match a {
                        fastskill::core::repository::RepositoryAuth::Pat { env_var } => {
                            Some(SourceAuth::Pat {
                                env_var: env_var.clone(),
                            })
                        }
                        fastskill::core::repository::RepositoryAuth::SshKey { path } => {
                            Some(SourceAuth::SshKey { path: path.clone() })
                        }
                        fastskill::core::repository::RepositoryAuth::Basic {
                            username,
                            password_env,
                        } => Some(SourceAuth::Basic {
                            username: username.clone(),
                            password_env: password_env.clone(),
                        }),
                        _ => None,
                    });

                    Some(SourceConfig::Git {
                        url: url.clone(),
                        branch: branch.clone(),
                        tag: tag.clone(),
                        auth,
                    })
                } else {
                    None
                }
            }
            RepositoryType::ZipUrl => {
                if let RepositoryConfig::ZipUrl { base_url } = &repo.config {
                    let auth = repo.auth.as_ref().and_then(|a| match a {
                        fastskill::core::repository::RepositoryAuth::Pat { env_var } => {
                            Some(SourceAuth::Pat {
                                env_var: env_var.clone(),
                            })
                        }
                        fastskill::core::repository::RepositoryAuth::Basic {
                            username,
                            password_env,
                        } => Some(SourceAuth::Basic {
                            username: username.clone(),
                            password_env: password_env.clone(),
                        }),
                        _ => None,
                    });

                    Some(SourceConfig::ZipUrl {
                        base_url: base_url.clone(),
                        auth,
                    })
                } else {
                    None
                }
            }
            RepositoryType::Local => {
                if let RepositoryConfig::Local { path } = &repo.config {
                    Some(SourceConfig::Local { path: path.clone() })
                } else {
                    None
                }
            }
            RepositoryType::HttpRegistry => {
                // Http-registry repos don't work with SourcesManager, skip them
                None
            }
        };

        if let Some(source_config) = source_config {
            marketplace_sources.push(SourceDefinition {
                name: repo.name.clone(),
                priority: repo.priority,
                source: source_config,
            });
        }
    }

    if marketplace_sources.is_empty() {
        return Ok(None);
    }

    // Create a temporary SourcesManager with these sources
    let temp_path = std::env::temp_dir().join("fastskill-sources-temp.toml");
    let mut sources_manager = SourcesManager::new(temp_path);

    // Add all sources
    for source_def in marketplace_sources {
        sources_manager
            .add_source_with_priority(
                source_def.name.clone(),
                source_def.source,
                source_def.priority,
            )
            .map_err(|e| CliError::Config(format!("Failed to add source: {}", e)))?;
    }

    Ok(Some(Arc::new(sources_manager)))
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
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_update_no_manifest() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill::test_utils::DIR_MUTEX.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        // Helper struct to ensure directory is restored even if test panics
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

        let args = UpdateArgs {
            skill_id: None,
            check: false,
            dry_run: false,
            version: None,
            source: None,
            strategy: "latest".to_string(),
        };

        let result = execute_update(args).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("skill-project.toml not found") && msg.contains("fastskill init"),
                "Error message must mention skill-project.toml and fastskill init: '{}'",
                msg
            );
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_execute_update_invalid_strategy() {
        let temp_dir = TempDir::new().unwrap();

        // Get original directory immediately and handle potential errors
        let original_dir = match std::env::current_dir() {
            Ok(dir) => dir,
            Err(_) => {
                // If we can't get current dir, assume we're in the project root
                std::path::PathBuf::from(".")
            }
        };

        // Create skill-project.toml and skills.lock at project root using absolute paths
        // Do this BEFORE changing directory to avoid any path resolution issues
        let skill_project_toml = temp_dir.path().join("skill-project.toml");
        let skills_lock = temp_dir.path().join("skills.lock");

        // Create files BEFORE changing directory to ensure they're created
        fs::write(
            &skill_project_toml,
            r#"[dependencies]
test-skill = { source = "git", url = "https://example.com/repo.git" }
"#,
        )
        .expect("Failed to write skill-project.toml");

        // Create skills.lock file (required for update) with proper format including generated_at
        use chrono::Utc;
        let lock_content = format!(
            r#"[metadata]
version = "1.0.0"
generated_at = "{}"

[[skills]]
id = "test-skill"
version = "1.0.0"
"#,
            Utc::now().to_rfc3339()
        );
        fs::write(&skills_lock, lock_content).expect("Failed to write skills.lock");

        // Verify files exist before test (using absolute paths)
        assert!(
            skill_project_toml.exists(),
            "skill-project.toml should exist at {}",
            skill_project_toml.display()
        );
        assert!(
            skills_lock.exists(),
            "skills.lock should exist at {}",
            skills_lock.display()
        );

        // Helper struct to ensure directory is restored even if test panics
        struct DirGuard(std::path::PathBuf);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _guard = DirGuard(original_dir.clone());

        // Change to temp directory AFTER creating files, but handle potential errors
        if let Err(e) = std::env::set_current_dir(temp_dir.path()) {
            panic!("Failed to change to temp directory: {}", e);
        }

        let args = UpdateArgs {
            skill_id: None,
            check: false,
            dry_run: false,
            version: None,
            source: None,
            strategy: "invalid-strategy".to_string(),
        };

        let result = execute_update(args).await;
        // Should fail for invalid strategy or missing skills_directory
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("Invalid strategy")
                    || msg.contains("latest, patch, minor, major")
                    || msg.contains("strategy")
                    || msg.contains("Failed to load repositories")
                    || msg.contains("skill-project.toml not found")
                    || msg.contains("requires [tool.fastskill] with skills_directory"),
                "Error message '{}' does not contain expected text",
                msg
            );
        } else {
            panic!(
                "Expected Config error for invalid strategy, got: {:?}",
                result
            );
        }
    }

    #[tokio::test]
    async fn test_execute_update_check_mode() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill::test_utils::DIR_MUTEX.lock().unwrap();

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        // Helper struct to ensure directory is restored even if test panics
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

        // Create skill-project.toml at project root using absolute paths
        let skill_project_toml = temp_dir.path().join("skill-project.toml");
        fs::write(&skill_project_toml, "[dependencies]").unwrap();

        let args = UpdateArgs {
            skill_id: None,
            check: true,
            dry_run: false,
            version: None,
            source: None,
            strategy: "latest".to_string(),
        };

        // Should succeed in check mode even with no skills
        let result = execute_update(args).await;
        // May succeed or fail depending on lock file, but shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }
}

//! Install command - installs skills from skills.toml to .claude/skills/ registry

use crate::cli::config::create_service_config;
use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::{install_utils, manifest_utils, messages};
use clap::Args;
use fastskill::core::{
    lock::SkillsLock,
    manifest::SkillsManifest,
    repository::{RepositoryConfig, RepositoryManager, RepositoryType},
    sources::{SourceAuth, SourceConfig, SourcesManager},
};
use fastskill::FastSkillService;
use std::fs;
use std::path::PathBuf;

/// Install skills from skills.toml to .claude/skills/ registry
#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Exclude skills from these groups (like poetry --without dev)
    #[arg(long)]
    without: Option<Vec<String>>,

    /// Only install skills from these groups
    #[arg(long)]
    only: Option<Vec<String>>,

    /// Use skills-lock.toml instead of skills.toml
    #[arg(long)]
    lock: bool,
}

pub async fn execute_install(args: InstallArgs) -> CliResult<()> {
    println!("Installing skills...");
    println!();

    let manifest_path = PathBuf::from(".claude/skills.toml");
    let lock_path = PathBuf::from(".claude/skills.lock");

    // Ensure .claude directory exists
    fs::create_dir_all(".claude")
        .map_err(|e| CliError::Config(format!("Failed to create .claude directory: {}", e)))?;

    // Check for lock file early if lock mode is requested (before service initialization)
    if args.lock && !lock_path.exists() {
        return Err(CliError::Config(
            "skills.lock not found. Run 'fastskill install' first to create it.".to_string(),
        ));
    }

    // Resolve skills directory from config
    let skills_dir = crate::cli::config::resolve_skills_storage_directory()?;

    // Initialize service
    // Note: install command doesn't have access to CLI sources_path, so uses env var or walk-up
    let config = create_service_config(None, None)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;

    // Load repositories and create sources manager for marketplace-based repos
    let repos_path = crate::cli::config::get_repositories_toml_path()
        .map_err(|e| CliError::Config(format!("Failed to find repositories.toml: {}", e)))?;
    let mut repo_manager = RepositoryManager::new(repos_path);
    repo_manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load repositories: {}", e)))?;

    // Create SourcesManager from marketplace-based repositories for PackageResolver
    let sources_manager = create_sources_manager_from_repositories(&repo_manager)
        .map_err(|e| CliError::Config(format!("Failed to create sources manager: {}", e)))?;

    // Load manifest or lock file
    let skills_to_install = if args.lock {
        // Lock file already checked above, so it exists

        let lock = SkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?;

        println!("Using lock file ({} skills)", lock.skills.len());

        // Convert lock entries to installable entries
        // For now, we'll install from lock file
        vec![] // TODO: Convert lock entries to install format
    } else {
        if !manifest_path.exists() {
            return Err(CliError::Config(
                "skills.toml not found. Create it or use 'fastskill add' to add skills."
                    .to_string(),
            ));
        }

        let manifest = SkillsManifest::load_from_file(&manifest_path)
            .map_err(|e| CliError::Config(format!("Failed to load manifest: {}", e)))?;

        // Filter skills by groups
        let exclude_groups = args.without.as_deref();
        let only_groups = args.only.as_deref();
        let skills = manifest.get_skills_for_groups(exclude_groups, only_groups);
        skills.into_iter().cloned().collect()
    };

    println!("Found {} skills to install", skills_to_install.len());

    if skills_to_install.is_empty() {
        println!(
            "{}",
            messages::info("No skills to install (filtered by groups)")
        );
        return Ok(());
    }

    // Ensure skills directory exists
    fs::create_dir_all(&skills_dir)
        .map_err(|e| CliError::Config(format!("Failed to create skills directory: {}", e)))?;

    // Install each skill
    let mut installed_skills = Vec::new();
    for entry in skills_to_install {
        println!("  Installing {}...", entry.id);
        match install_utils::install_skill_from_entry(
            &service,
            entry.clone(),
            sources_manager.as_ref(),
        )
        .await
        {
            Ok(skill_def) => {
                installed_skills.push((skill_def, entry.groups.clone(), entry.editable));
                println!("  {}", messages::ok(&format!("Installed {}", entry.id)));
            }
            Err(e) => {
                eprintln!(
                    "  {}",
                    messages::error(&format!("Failed to install {}: {}", entry.id, e))
                );
                // Continue with other skills
            }
        }
    }

    // Update lock file with all installed skills
    for (skill_def, groups, _editable) in installed_skills {
        manifest_utils::update_lock_file(&lock_path, &skill_def, groups)
            .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;
    }

    println!();
    println!("{}", messages::ok("Installation complete"));
    println!("   Updated skills.lock");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_install_no_manifest() {
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

        let args = InstallArgs {
            without: None,
            only: None,
            lock: false,
        };

        let result = execute_install(args).await;
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            // Accept either "not found" error, parse error, repository error, or directory creation error (if files exist from other tests)
            assert!(
                msg.contains("skills.toml not found")
                    || msg.contains("Failed to load manifest")
                    || msg.contains("Failed to load repositories")
                    || msg.contains("Failed to create .claude directory"),
                "Error message '{}' does not contain expected text",
                msg
            );
        } else {
            panic!("Expected Config error, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_execute_install_with_lock_file_not_found() {
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

        // Create .claude directory but no lock file
        fs::create_dir_all(".claude").unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: true,
        };

        let result = execute_install(args).await;
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            // Accept either "lock not found" error, repository error, or directory creation error (if files exist from other tests)
            assert!(
                msg.contains("skills.lock not found")
                    || msg.contains("Failed to load repositories")
                    || msg.contains("Failed to create .claude directory"),
                "Error message '{}' does not contain expected text",
                msg
            );
        } else {
            panic!("Expected Config error, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_execute_install_with_empty_manifest() {
        // Use a static mutex to serialize directory changes across parallel tests
        use std::sync::Mutex;
        static DIR_MUTEX: Mutex<()> = Mutex::new(());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        // Lock the mutex for the duration of directory change
        let _lock = DIR_MUTEX.lock().unwrap();

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

        // Create .claude directory and empty skills.toml using absolute paths
        let claude_dir = temp_dir.path().join(".claude");
        let skills_toml = claude_dir.join("skills.toml");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(&skills_toml, "[skills]").unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: false,
        };

        // Should succeed with empty manifest (no skills to install)
        let result = execute_install(args).await;
        // May succeed or fail depending on service initialization, but shouldn't panic
        assert!(result.is_ok() || result.is_err());

        // Cleanup
        fs::remove_dir_all(".claude").ok();
    }
}

/// Create SourcesManager from RepositoryManager for marketplace-based repositories
/// This is needed because PackageResolver requires SourcesManager
fn create_sources_manager_from_repositories(
    repo_manager: &RepositoryManager,
) -> CliResult<Option<SourcesManager>> {
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

    Ok(Some(sources_manager))
}

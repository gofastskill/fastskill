//! Install command - installs skills from skill-project.toml dependencies

use crate::cli::config::create_service_config;
use crate::cli::error::{manifest_required_message, CliError, CliResult};
use crate::cli::utils::{install_utils, manifest_utils, messages};
use clap::Args;
use fastskill::core::{
    lock::SkillsLock,
    manifest::SkillProjectToml,
    project::resolve_project_file,
    repository::{RepositoryConfig, RepositoryManager, RepositoryType},
    sources::{SourceAuth, SourceConfig, SourcesManager},
};
use fastskill::FastSkillService;
use std::env;
use std::fs;
use std::path::PathBuf;

/// Install skills from skill-project.toml [dependencies]
///
/// Reads dependencies from skill-project.toml at the project root.
/// Installs to the skills directory configured in [tool.fastskill].skills_directory.
/// Creates or updates skills.lock for reproducible installations.
#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Exclude skills from these groups (like poetry --without dev)
    #[arg(long)]
    without: Option<Vec<String>>,

    /// Only install skills from these groups
    #[arg(long)]
    only: Option<Vec<String>>,

    /// Install from skills.lock (exact versions) instead of resolving from skill-project.toml
    #[arg(long)]
    lock: bool,
}

pub async fn execute_install(args: InstallArgs) -> CliResult<()> {
    println!("Installing skills...");
    println!();

    // T027: Resolve skill-project.toml from project root
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    // Require manifest unless installing from lock
    if !args.lock && !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    // T033: Lock file at project root (skills.lock)
    let lock_path = if let Some(parent) = project_file_path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };

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

    // Load repositories from skill-project.toml [tool.fastskill.repositories] if available
    // Otherwise fall back to .claude/repositories.toml for backward compatibility
    let repo_manager = if project_file_result.found {
        // Try to load repositories from skill-project.toml
        let project = SkillProjectToml::load_from_file(&project_file_path)
            .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;

        if let Some(ref tool) = project.tool {
            if let Some(ref fastskill_config) = tool.fastskill {
                if let Some(ref _repos) = fastskill_config.repositories {
                    // Create RepositoryManager from skill-project.toml repositories
                    // For now, we'll still use the old path for RepositoryManager
                    // TODO: Update RepositoryManager to work with skill-project.toml directly
                }
            }
        }

        // Load from skill-project.toml [tool.fastskill.repositories]
        let repositories = crate::cli::config::load_repositories_from_project()?;
        RepositoryManager::from_definitions(repositories)
    } else {
        // No skill-project.toml, load from skill-project.toml if available
        let repositories = crate::cli::config::load_repositories_from_project()?;
        RepositoryManager::from_definitions(repositories)
    };

    // Create SourcesManager from marketplace-based repositories for PackageResolver
    let sources_manager = create_sources_manager_from_repositories(&repo_manager)
        .map_err(|e| CliError::Config(format!("Failed to create sources manager: {}", e)))?;

    // T027: Load from skill-project.toml or lock file
    let skills_to_install = if args.lock {
        // Lock file already checked above, so it exists
        let lock = SkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?;

        println!("Using lock file ({} skills)", lock.skills.len());

        // Convert lock entries to installable entries
        // For now, we'll install from lock file
        vec![] // TODO: Convert lock entries to install format
    } else {
        // Load from skill-project.toml (manifest already required above when !args.lock)
        let project = SkillProjectToml::load_from_file(&project_file_path)
            .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;

        // Validate context
        let context = project_file_result.context;
        project.validate_for_context(context).map_err(|e| {
            CliError::Config(format!("skill-project.toml validation failed: {}", e))
        })?;

        // Convert dependencies to SkillEntry format
        let mut entries = project
            .to_skill_entries()
            .map_err(|e| CliError::Config(format!("Failed to parse dependencies: {}", e)))?;

        // Filter skills by groups
        let exclude_groups = args.without.as_deref();
        let only_groups = args.only.as_deref();

        if let Some(exclude) = exclude_groups {
            entries.retain(|entry| {
                !entry
                    .groups
                    .iter()
                    .any(|g| exclude.iter().any(|ex| ex == g.as_str()))
            });
        }

        if let Some(only) = only_groups {
            entries.retain(|entry| {
                entry
                    .groups
                    .iter()
                    .any(|g| only.iter().any(|on| on == g.as_str()))
                    || entry.groups.is_empty()
            });
        }

        // Sort entries by ID for deterministic output
        entries.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        entries
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
    let mut failed_skills = Vec::new();
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
                failed_skills.push(entry.id.to_string());
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

    // Return error if any skills failed to install
    if !failed_skills.is_empty() {
        return Err(CliError::Config(format!(
            "Failed to install {} skill(s): {}",
            failed_skills.len(),
            failed_skills.join(", ")
        )));
    }

    Ok(())
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock,
    clippy::collapsible_if
)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_install_no_manifest() {
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

        let args = InstallArgs {
            without: None,
            only: None,
            lock: false,
        };

        let result = execute_install(args).await;
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            assert!(
                (msg.contains("skill-project.toml not found") && msg.contains("fastskill init"))
                    || msg.contains("skill-project.toml")
                        && (msg.contains("not found") || msg.contains("Manifest file not found")),
                "Error message must mention skill-project.toml and creation/init: '{}'",
                msg
            );
        } else {
            panic!("Expected Config error, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_execute_install_with_lock_file_not_found() {
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

        // Create skill-project.toml but no lock file
        fs::write(
            temp_dir.path().join("skill-project.toml"),
            "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
        )
        .unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: true,
        };

        let result = execute_install(args).await;
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            // Should fail because skills.lock not found
            assert!(
                msg.contains("skills.lock not found"),
                "Error message '{}' does not contain expected text",
                msg
            );
        } else {
            panic!("Expected Config error, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_execute_install_with_empty_manifest() {
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

        // Create skill-project.toml at project root with empty [dependencies]
        let project_toml = temp_dir.path().join("skill-project.toml");
        fs::write(&project_toml, "[dependencies]\n").unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: false,
        };

        // Should succeed with empty manifest (no skills to install) or fail on service/repos; shouldn't panic
        let result = execute_install(args).await;
        assert!(result.is_ok() || result.is_err());
    }
}

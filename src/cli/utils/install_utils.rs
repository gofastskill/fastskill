//! Utilities for installing skills from various sources

use crate::cli::error::{CliError, CliResult};
use chrono::Utc;
use fastskill::core::repository::{RepositoryConfig, RepositoryManager, RepositoryType};
use fastskill::core::sources::{SourceAuth, SourceConfig, SourcesManager};
use fastskill::core::{manifest::SkillEntry, manifest::SkillSource, skill_manager::SourceType};
use fastskill::{FastSkillService, SkillDefinition};
use std::path::PathBuf;

/// Create a symlink for editable installs
/// This is platform-specific: Unix uses tokio::fs::symlink, Windows uses std::os::windows::fs::symlink_dir
/// On unsupported platforms, returns EditableNotSupported error
async fn create_editable_symlink(src: &PathBuf, dst: &PathBuf) -> CliResult<()> {
    #[cfg(unix)]
    {
        tokio::fs::symlink(src, dst).await.map_err(CliError::Io)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_dir;
        symlink_dir(src, dst).map_err(|e| CliError::WindowsSymlinkPermission(e.to_string()))
    }

    #[cfg(all(not(unix), not(windows)))]
    {
        Err(CliError::EditableNotSupported)
    }
}

/// Install a single skill from a SkillEntry
pub async fn install_skill_from_entry(
    service: &FastSkillService,
    entry: SkillEntry,
    sources_manager: Option<&SourcesManager>,
) -> CliResult<SkillDefinition> {
    match &entry.source {
        SkillSource::Git {
            url,
            branch,
            tag,
            subdir,
        } => {
            install_from_git(
                service,
                url,
                branch.as_deref(),
                tag.as_deref(),
                subdir.as_ref(),
            )
            .await
        }
        SkillSource::Local { path, editable } => install_from_local(service, path, *editable).await,
        SkillSource::ZipUrl { base_url, version } => {
            install_from_zip_url(service, base_url, version.as_deref()).await
        }
        SkillSource::Source {
            name,
            skill,
            version,
        } => {
            if let Some(sources_mgr) = sources_manager {
                install_from_source(service, sources_mgr, name, skill, version.as_deref()).await
            } else {
                Err(CliError::Config(
                    "Sources manager required for source-based installation".to_string(),
                ))
            }
        }
    }
}

async fn install_from_git(
    service: &FastSkillService,
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    subdir: Option<&PathBuf>,
) -> CliResult<SkillDefinition> {
    {
        use crate::cli::commands::add::{copy_dir_recursive, create_skill_from_path};
        use crate::cli::utils::parse_git_url;
        use fastskill::storage::git::{clone_repository, validate_cloned_skill};

        let git_info = parse_git_url(url)?;
        let branch = branch.or(git_info.branch.as_deref());
        let temp_dir = clone_repository(&git_info.repo_url, branch, tag, None)
            .await
            .map_err(|e| CliError::GitCloneFailed(e.to_string()))?;

        let skill_base_path = if let Some(subdir) = subdir.or(git_info.subdir.as_ref()) {
            let subdir_path = temp_dir.path().join(subdir);
            if !subdir_path.exists() {
                return Err(CliError::InvalidSource(format!(
                    "Specified subdirectory '{}' does not exist",
                    subdir.display()
                )));
            }
            subdir_path
        } else {
            temp_dir.path().to_path_buf()
        };

        let skill_path = validate_cloned_skill(&skill_base_path)
            .map_err(|e| CliError::SkillValidationFailed(e.to_string()))?;

        let mut skill_def = create_skill_from_path(&skill_path)?;
        let skill_storage_dir = service
            .config()
            .skill_storage_path
            .join(skill_def.id.as_str());

        // Copy to storage
        tokio::fs::create_dir_all(&skill_storage_dir)
            .await
            .map_err(CliError::Io)?;
        copy_dir_recursive(&skill_path, &skill_storage_dir).await?;

        skill_def.skill_file = skill_storage_dir.join("SKILL.md");
        skill_def.source_url = Some(url.to_string());
        skill_def.source_type = Some(SourceType::GitUrl);
        skill_def.source_branch = branch.map(|s| s.to_string());
        skill_def.source_tag = tag.map(|s| s.to_string());
        skill_def.source_subdir = subdir.cloned();
        skill_def.fetched_at = Some(Utc::now());
        skill_def.editable = false;

        // Register skill
        service
            .skill_manager()
            .force_register_skill(skill_def.clone())
            .await
            .map_err(CliError::Service)?;

        // Update with source tracking
        service
            .skill_manager()
            .update_skill(
                &skill_def.id,
                fastskill::core::skill_manager::SkillUpdate {
                    source_url: skill_def.source_url.clone(),
                    source_type: skill_def.source_type.clone(),
                    source_branch: skill_def.source_branch.clone(),
                    source_tag: skill_def.source_tag.clone(),
                    source_subdir: skill_def.source_subdir.clone(),
                    fetched_at: skill_def.fetched_at,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| {
                super::service_error_to_cli(e, service.config().skill_storage_path.as_path())
            })?;

        Ok(skill_def)
    }
}

async fn install_from_local(
    service: &FastSkillService,
    path: &PathBuf,
    editable: bool,
) -> CliResult<SkillDefinition> {
    use crate::cli::commands::add::{copy_dir_recursive, create_skill_from_path};

    let skill_path = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir().map_err(CliError::Io)?.join(path)
    };

    if !skill_path.exists() {
        return Err(CliError::InvalidSource(format!(
            "Local path does not exist: {}",
            skill_path.display()
        )));
    }

    let mut skill_def = create_skill_from_path(&skill_path)?;
    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(skill_def.id.as_str());

    if editable {
        // Editable installs use symlinks only - no copy fallback
        if skill_storage_dir.exists() {
            tokio::fs::remove_dir_all(&skill_storage_dir)
                .await
                .map_err(CliError::Io)?;
        }
        create_editable_symlink(&skill_path, &skill_storage_dir).await?;
        skill_def.skill_file = skill_storage_dir.join("SKILL.md");
    } else {
        // Non-editable installs: copy to storage
        if skill_storage_dir.exists() {
            tokio::fs::remove_dir_all(&skill_storage_dir)
                .await
                .map_err(CliError::Io)?;
        }
        copy_dir_recursive(&skill_path, &skill_storage_dir).await?;
        skill_def.skill_file = skill_storage_dir.join("SKILL.md");
    }

    skill_def.source_url = Some(skill_path.to_string_lossy().to_string());
    skill_def.source_type = Some(SourceType::LocalPath);
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = editable;

    // Register skill
    service
        .skill_manager()
        .force_register_skill(skill_def.clone())
        .await
        .map_err(CliError::Service)?;

    // Update with source tracking
    service
        .skill_manager()
        .update_skill(
            &skill_def.id,
            fastskill::core::skill_manager::SkillUpdate {
                source_url: skill_def.source_url.clone(),
                source_type: skill_def.source_type.clone(),
                fetched_at: skill_def.fetched_at,
                editable: Some(skill_def.editable),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            super::service_error_to_cli(e, service.config().skill_storage_path.as_path())
        })?;

    Ok(skill_def)
}

async fn install_from_zip_url(
    _service: &FastSkillService,
    _base_url: &str,
    _version: Option<&str>,
) -> CliResult<SkillDefinition> {
    // TODO: Implement ZIP URL installation
    // This would download the ZIP file and extract it
    Err(CliError::Config(
        "ZIP URL installation not yet implemented".to_string(),
    ))
}

async fn install_from_source(
    service: &FastSkillService,
    _sources_manager: &SourcesManager,
    source_name: &str,
    skill_name: &str,
    version: Option<&str>,
) -> CliResult<SkillDefinition> {
    use fastskill::core::resolver::{ConflictStrategy, PackageResolver};
    use fastskill::core::version::VersionConstraint;
    use std::sync::Arc;

    // Create resolver - load from skill-project.toml [tool.fastskill.repositories]
    let repositories = crate::cli::config::load_repositories_from_project()?;
    let repo_manager =
        fastskill::core::repository::RepositoryManager::from_definitions(repositories);

    // Create SourcesManager from marketplace-based repositories
    let sources_mgr = if let Some(sources_manager) =
        create_sources_manager_from_repositories(&repo_manager)
            .map_err(|e| CliError::Config(format!("Failed to create sources manager: {}", e)))?
    {
        Arc::new(sources_manager)
    } else {
        return Err(CliError::Config(
            "No marketplace-based repositories found. PackageResolver requires at least one marketplace repository.".to_string(),
        ));
    };

    let mut resolver = PackageResolver::new(sources_mgr.clone());
    resolver
        .build_index()
        .await
        .map_err(|e| CliError::Config(format!("Failed to build resolver index: {}", e)))?;

    // Parse version constraint if provided
    let version_constraint = if let Some(ver) = version {
        Some(VersionConstraint::parse(ver).map_err(|e| {
            CliError::Config(format!("Invalid version constraint '{}': {}", ver, e))
        })?)
    } else {
        None
    };

    // Resolve the skill
    let resolution = resolver
        .resolve_skill(
            skill_name,
            version_constraint.as_ref(),
            Some(source_name),
            ConflictStrategy::Priority,
        )
        .map_err(|e| {
            CliError::Config(format!("Failed to resolve skill '{}': {}", skill_name, e))
        })?;

    let candidate = &resolution.candidate;

    // Install based on source type
    match &candidate.source_config {
        fastskill::core::sources::SourceConfig::Git {
            url,
            branch,
            tag,
            auth: _auth,
        } => install_from_git(service, url, branch.as_deref(), tag.as_deref(), None).await,
        fastskill::core::sources::SourceConfig::Local { path } => {
            install_from_local(service, path, false).await
        }
        fastskill::core::sources::SourceConfig::ZipUrl { base_url, .. } => {
            install_from_zip_url(service, base_url, version).await
        }
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

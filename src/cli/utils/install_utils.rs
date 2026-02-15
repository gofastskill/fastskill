//! Utilities for installing skills from various sources

use crate::cli::error::{CliError, CliResult};
use chrono::Utc;
use fastskill::core::repository::{RepositoryConfig, RepositoryManager, RepositoryType};
use fastskill::core::sources::{SourceAuth, SourceConfig, SourcesManager};
use fastskill::core::{manifest::SkillEntry, manifest::SkillSource, skill_manager::SourceType};
use fastskill::{FastSkillService, SkillDefinition};
use std::path::{Path, PathBuf};

/// Create a symlink for editable installs
/// This is platform-specific: Unix uses tokio::fs::symlink, Windows uses std::os::windows::fs::symlink_dir
/// On unsupported platforms, returns EditableNotSupported error
async fn create_editable_symlink(src: &Path, dst: &Path) -> CliResult<()> {
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

/// Configuration for git-based skill installation
struct GitInstallConfig<'a> {
    url: &'a str,
    branch: Option<&'a str>,
    tag: Option<&'a str>,
    subdir: Option<&'a PathBuf>,
}

/// Clone git repository and determine skill path
async fn clone_and_find_skill(
    config: &GitInstallConfig<'_>,
) -> CliResult<(tempfile::TempDir, PathBuf)> {
    use crate::cli::utils::parse_git_url;
    use fastskill::storage::git::{clone_repository, validate_cloned_skill};

    let git_info = parse_git_url(config.url)?;
    let branch = config.branch.or(git_info.branch.as_deref());
    let temp_dir = clone_repository(&git_info.repo_url, branch, config.tag, None)
        .await
        .map_err(|e| CliError::GitCloneFailed(e.to_string()))?;

    let skill_base_path = if let Some(subdir) = config.subdir.or(git_info.subdir.as_ref()) {
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

    Ok((temp_dir, skill_path))
}

/// Copy skill to storage and update metadata
async fn copy_skill_to_storage(
    service: &FastSkillService,
    skill_path: &Path,
    skill_def: &mut SkillDefinition,
) -> CliResult<()> {
    use crate::cli::commands::add::copy_dir_recursive;

    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(skill_def.id.as_str());

    tokio::fs::create_dir_all(&skill_storage_dir)
        .await
        .map_err(CliError::Io)?;
    copy_dir_recursive(skill_path, &skill_storage_dir).await?;

    skill_def.skill_file = skill_storage_dir.join("SKILL.md");
    Ok(())
}

/// Register skill and update with source tracking
async fn register_skill_with_source(
    service: &FastSkillService,
    skill_def: &SkillDefinition,
) -> CliResult<()> {
    service
        .skill_manager()
        .force_register_skill(skill_def.clone())
        .await
        .map_err(CliError::Service)?;

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
            super::service_error_to_cli(e, service.config().skill_storage_path.as_path(), false)
        })?;

    Ok(())
}

async fn install_from_git(
    service: &FastSkillService,
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    subdir: Option<&PathBuf>,
) -> CliResult<SkillDefinition> {
    use crate::cli::commands::add::create_skill_from_path;

    let config = GitInstallConfig {
        url,
        branch,
        tag,
        subdir,
    };

    let (_temp_dir, skill_path) = clone_and_find_skill(&config).await?;
    let mut skill_def = create_skill_from_path(&skill_path, "git", false)?;

    copy_skill_to_storage(service, &skill_path, &mut skill_def).await?;

    skill_def.source_url = Some(url.to_string());
    skill_def.source_type = Some(SourceType::GitUrl);
    skill_def.source_branch = branch.map(|s| s.to_string());
    skill_def.source_tag = tag.map(|s| s.to_string());
    skill_def.source_subdir = subdir.cloned();
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = false;

    register_skill_with_source(service, &skill_def).await?;

    Ok(skill_def)
}

/// Resolve absolute path from potentially relative path
fn resolve_absolute_path(path: &PathBuf) -> CliResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.clone())
    } else {
        Ok(std::env::current_dir().map_err(CliError::Io)?.join(path))
    }
}

/// Validate that skill path exists
fn validate_skill_path_exists(skill_path: &Path) -> CliResult<()> {
    if !skill_path.exists() {
        return Err(CliError::InvalidSource(format!(
            "Local path does not exist: {}",
            skill_path.display()
        )));
    }
    Ok(())
}

/// Setup skill in storage directory (editable or copy)
async fn setup_skill_in_storage(
    skill_path: &Path,
    skill_storage_dir: &Path,
    editable: bool,
) -> CliResult<()> {
    use crate::cli::commands::add::copy_dir_recursive;

    if skill_storage_dir.exists() {
        tokio::fs::remove_dir_all(skill_storage_dir)
            .await
            .map_err(CliError::Io)?;
    }

    if editable {
        create_editable_symlink(skill_path, skill_storage_dir).await?;
    } else {
        copy_dir_recursive(skill_path, skill_storage_dir).await?;
    }

    Ok(())
}

/// Register local skill with source tracking
async fn register_local_skill(
    service: &FastSkillService,
    skill_def: &SkillDefinition,
) -> CliResult<()> {
    service
        .skill_manager()
        .force_register_skill(skill_def.clone())
        .await
        .map_err(CliError::Service)?;

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
            super::service_error_to_cli(e, service.config().skill_storage_path.as_path(), false)
        })?;

    Ok(())
}

async fn install_from_local(
    service: &FastSkillService,
    path: &PathBuf,
    editable: bool,
) -> CliResult<SkillDefinition> {
    use crate::cli::commands::add::create_skill_from_path;

    let skill_path = resolve_absolute_path(path)?;
    validate_skill_path_exists(&skill_path)?;

    let mut skill_def = create_skill_from_path(&skill_path, "local", editable)?;
    let skill_storage_dir = service
        .config()
        .skill_storage_path
        .join(skill_def.id.as_str());

    setup_skill_in_storage(&skill_path, &skill_storage_dir, editable).await?;

    skill_def.skill_file = skill_storage_dir.join("SKILL.md");
    skill_def.source_url = Some(skill_path.to_string_lossy().to_string());
    skill_def.source_type = Some(SourceType::LocalPath);
    skill_def.fetched_at = Some(Utc::now());
    skill_def.editable = editable;

    register_local_skill(service, &skill_def).await?;

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

/// Create and initialize package resolver
async fn create_package_resolver() -> CliResult<fastskill::core::resolver::PackageResolver> {
    use fastskill::core::resolver::PackageResolver;
    use std::sync::Arc;

    let repositories = crate::cli::config::load_repositories_from_project()?;
    let repo_manager =
        fastskill::core::repository::RepositoryManager::from_definitions(repositories);

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

    Ok(resolver)
}

/// Parse version constraint from string
fn parse_version_constraint(
    version: Option<&str>,
) -> CliResult<Option<fastskill::core::version::VersionConstraint>> {
    use fastskill::core::version::VersionConstraint;

    if let Some(ver) = version {
        let constraint = VersionConstraint::parse(ver).map_err(|e| {
            CliError::Config(format!("Invalid version constraint '{}': {}", ver, e))
        })?;
        Ok(Some(constraint))
    } else {
        Ok(None)
    }
}

/// Resolve skill from source
fn resolve_skill_from_source(
    resolver: &fastskill::core::resolver::PackageResolver,
    source_name: &str,
    skill_name: &str,
    version_constraint: Option<&fastskill::core::version::VersionConstraint>,
) -> CliResult<fastskill::core::resolver::ResolutionResult> {
    use fastskill::core::resolver::ConflictStrategy;

    resolver
        .resolve_skill(
            skill_name,
            version_constraint,
            Some(source_name),
            ConflictStrategy::Priority,
        )
        .map_err(|e| CliError::Config(format!("Failed to resolve skill '{}': {}", skill_name, e)))
}

/// Install skill based on resolved source configuration
async fn install_from_resolved_source(
    service: &FastSkillService,
    source_config: &fastskill::core::sources::SourceConfig,
    version: Option<&str>,
) -> CliResult<SkillDefinition> {
    match source_config {
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

async fn install_from_source(
    service: &FastSkillService,
    _sources_manager: &SourcesManager,
    source_name: &str,
    skill_name: &str,
    version: Option<&str>,
) -> CliResult<SkillDefinition> {
    let resolver = create_package_resolver().await?;
    let version_constraint = parse_version_constraint(version)?;
    let resolution = resolve_skill_from_source(
        &resolver,
        source_name,
        skill_name,
        version_constraint.as_ref(),
    )?;

    install_from_resolved_source(service, &resolution.candidate.source_config, version).await
}

/// Convert repository authentication to source authentication
fn convert_repo_auth(auth: &fastskill::core::repository::RepositoryAuth) -> Option<SourceAuth> {
    match auth {
        fastskill::core::repository::RepositoryAuth::Pat { env_var } => Some(SourceAuth::Pat {
            env_var: env_var.clone(),
        }),
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
    }
}

/// Convert a single repository to a source config
fn repository_to_source_config(
    repo: &fastskill::core::repository::RepositoryDefinition,
) -> Option<SourceConfig> {
    match &repo.repo_type {
        RepositoryType::GitMarketplace => {
            if let RepositoryConfig::GitMarketplace { url, branch, tag } = &repo.config {
                let auth = repo.auth.as_ref().and_then(convert_repo_auth);
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
                let auth = repo.auth.as_ref().and_then(convert_repo_auth);
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
        RepositoryType::HttpRegistry => None, // Http-registry repos don't work with SourcesManager
    }
}

/// Create source definitions from repositories
fn create_source_definitions(
    repos: Vec<&fastskill::core::repository::RepositoryDefinition>,
) -> Vec<fastskill::core::sources::SourceDefinition> {
    use fastskill::core::sources::SourceDefinition;

    repos
        .into_iter()
        .filter_map(|repo| {
            repository_to_source_config(repo).map(|source_config| SourceDefinition {
                name: repo.name.clone(),
                priority: repo.priority,
                source: source_config,
            })
        })
        .collect()
}

/// Build SourcesManager from source definitions
fn build_sources_manager(
    marketplace_sources: Vec<fastskill::core::sources::SourceDefinition>,
) -> CliResult<SourcesManager> {
    let temp_path = std::env::temp_dir().join("fastskill-sources-temp.toml");
    let mut sources_manager = SourcesManager::new(temp_path);

    for source_def in marketplace_sources {
        sources_manager
            .add_source_with_priority(
                source_def.name.clone(),
                source_def.source,
                source_def.priority,
            )
            .map_err(|e| CliError::Config(format!("Failed to add source: {}", e)))?;
    }

    Ok(sources_manager)
}

/// Create SourcesManager from RepositoryManager for marketplace-based repositories
/// This is needed because PackageResolver requires SourcesManager
fn create_sources_manager_from_repositories(
    repo_manager: &RepositoryManager,
) -> CliResult<Option<SourcesManager>> {
    let repos = repo_manager.list_repositories();
    let marketplace_sources = create_source_definitions(repos);

    if marketplace_sources.is_empty() {
        return Ok(None);
    }

    Ok(Some(build_sources_manager(marketplace_sources)?))
}

//! Utilities for installing skills from various sources

use crate::error::{CliError, CliResult};
use chrono::Utc;
use fastskill_core::core::repository::RepositoryManager;
use fastskill_core::core::sources::SourcesManager;
use fastskill_core::core::{
    manifest::SkillEntry, manifest::SkillSource, skill_manager::SourceType,
};
use fastskill_core::{FastSkillService, SkillDefinition};
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
        symlink_dir(src, dst).map_err(|e| {
            CliError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Windows symlink permission denied: {}. Enable Developer Mode \
                     (Settings → For developers) or run as Administrator.",
                    e
                ),
            ))
        })
    }

    #[cfg(all(not(unix), not(windows)))]
    {
        Err(CliError::Io(std::io::Error::other(
            "Editable installations are not supported on this platform. Use a Unix-based system or Docker.",
        )))
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
    use crate::utils::parse_git_url;
    use fastskill_core::storage::git::{clone_repository, validate_cloned_skill};

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
    use crate::commands::add::copy_dir_recursive;

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

async fn register_skill(
    service: &FastSkillService,
    skill_def: &SkillDefinition,
    update: fastskill_core::core::skill_manager::SkillUpdate,
) -> CliResult<()> {
    service
        .skill_manager()
        .force_register_skill(skill_def.clone())
        .await
        .map_err(CliError::Service)?;

    service
        .skill_manager()
        .update_skill(&skill_def.id, update)
        .await
        .map_err(|e| {
            super::service_error_to_cli(e, service.config().skill_storage_path.as_path(), false)
        })?;

    Ok(())
}

fn base_skill_update(def: &SkillDefinition) -> fastskill_core::core::skill_manager::SkillUpdate {
    fastskill_core::core::skill_manager::SkillUpdate {
        source_url: def.source_url.clone(),
        source_type: def.source_type.clone(),
        fetched_at: def.fetched_at,
        ..Default::default()
    }
}

async fn install_from_git(
    service: &FastSkillService,
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    subdir: Option<&PathBuf>,
) -> CliResult<SkillDefinition> {
    use crate::commands::add::create_skill_from_path;

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

    register_skill(
        service,
        &skill_def,
        fastskill_core::core::skill_manager::SkillUpdate {
            source_branch: skill_def.source_branch.clone(),
            source_tag: skill_def.source_tag.clone(),
            source_subdir: skill_def.source_subdir.clone(),
            ..base_skill_update(&skill_def)
        },
    )
    .await?;

    Ok(skill_def)
}

/// Setup skill in storage directory (editable or copy)
pub(crate) async fn setup_skill_in_storage(
    skill_path: &Path,
    skill_storage_dir: &Path,
    editable: bool,
) -> CliResult<()> {
    use crate::commands::add::copy_dir_recursive;

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

async fn install_from_local(
    service: &FastSkillService,
    path: &PathBuf,
    editable: bool,
) -> CliResult<SkillDefinition> {
    use crate::commands::add::create_skill_from_path;

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

    register_skill(
        service,
        &skill_def,
        fastskill_core::core::skill_manager::SkillUpdate {
            editable: Some(skill_def.editable),
            ..base_skill_update(&skill_def)
        },
    )
    .await?;

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
async fn create_package_resolver() -> CliResult<fastskill_core::core::resolver::PackageResolver> {
    use fastskill_core::core::resolver::PackageResolver;
    use std::sync::Arc;

    let repositories = crate::config::load_repositories_from_project()?;
    let repo_manager =
        fastskill_core::core::repository::RepositoryManager::from_definitions(repositories);

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

/// Install skill based on resolved source configuration
async fn install_from_resolved_source(
    service: &FastSkillService,
    source_config: &fastskill_core::core::sources::SourceConfig,
    version: Option<&str>,
) -> CliResult<SkillDefinition> {
    match source_config {
        fastskill_core::core::sources::SourceConfig::Git {
            url,
            branch,
            tag,
            auth: _auth,
        } => install_from_git(service, url, branch.as_deref(), tag.as_deref(), None).await,
        fastskill_core::core::sources::SourceConfig::Local { path } => {
            install_from_local(service, path, false).await
        }
        fastskill_core::core::sources::SourceConfig::ZipUrl { base_url, .. } => {
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
    use fastskill_core::core::resolver::ConflictStrategy;

    let resolver = create_package_resolver().await?;
    let version_constraint = version
        .map(|v| {
            fastskill_core::core::version::VersionConstraint::parse(v)
                .map_err(|e| CliError::Config(format!("Invalid version constraint '{}': {}", v, e)))
        })
        .transpose()?;
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

    install_from_resolved_source(service, &resolution.candidate.source_config, version).await
}

/// Create SourcesManager from RepositoryManager for marketplace-based repositories
/// This is needed because PackageResolver requires SourcesManager
pub fn create_sources_manager_from_repositories(
    repo_manager: &RepositoryManager,
) -> CliResult<Option<SourcesManager>> {
    SourcesManager::from_repositories(repo_manager)
        .map_err(|e| CliError::Config(format!("Failed to create sources manager: {}", e)))
}

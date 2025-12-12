//! Configuration and skills directory resolution for CLI

use crate::cli::error::{CliError, CliResult};
use fastskill::{core::BlobStorageConfig, ServiceConfig};
use std::env;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Get skills.toml path with priority:
/// 1. FASTSKILL_SKILLS_TOML_PATH environment variable
/// 2. Walk up directory tree to find .claude/skills.toml
/// 3. Default to .claude/skills.toml in current directory
#[allow(dead_code)]
pub fn get_skills_toml_path() -> CliResult<PathBuf> {
    // Priority 1: Environment variable
    if let Ok(env_path) = env::var("FASTSKILL_SKILLS_TOML_PATH") {
        return Ok(PathBuf::from(env_path));
    }

    // Priority 2: Walk up directory tree to find .claude/skills.toml
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    if let Some(found_path) = walk_up_for_skills_toml(current_dir.as_path()) {
        debug!(
            "Found .claude/skills.toml by walking up: {}",
            found_path.display()
        );
        return Ok(found_path);
    }

    // Priority 3: Default to current directory (but don't create it)
    Ok(PathBuf::from(".claude/skills.toml"))
}

/// Walk up the directory tree searching for existing .claude/skills.toml file
#[allow(dead_code)]
fn walk_up_for_skills_toml(start_path: &Path) -> Option<PathBuf> {
    let mut current = start_path.to_path_buf();

    loop {
        let skills_toml = current.join(".claude/skills.toml");
        if skills_toml.is_file() {
            return Some(skills_toml.canonicalize().unwrap_or(skills_toml));
        }

        // Check if we've reached the filesystem root
        if !current.pop() {
            break;
        }
    }

    None
}

/// Walk up the directory tree searching for existing .claude/skills/ folder
fn walk_up_for_skills_dir(start_path: &Path) -> Option<PathBuf> {
    let mut current = start_path.to_path_buf();

    loop {
        let skills_dir = current.join(".claude/skills");
        if skills_dir.is_dir() {
            return Some(skills_dir.canonicalize().unwrap_or(skills_dir));
        }

        // Check if we've reached the filesystem root
        if !current.pop() {
            break;
        }
    }

    None
}

/// Get repositories.toml path with priority:
/// 1. Walk up directory tree to find .claude/repositories.toml
/// 2. Default to .claude/repositories.toml in current directory
pub fn get_repositories_toml_path() -> CliResult<PathBuf> {
    // Priority 1: Walk up directory tree to find .claude/repositories.toml
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    if let Some(found_path) = walk_up_for_repositories_toml(current_dir.as_path()) {
        debug!(
            "Found .claude/repositories.toml by walking up: {}",
            found_path.display()
        );
        return Ok(found_path);
    }

    // Priority 2: Default to current directory (but don't create it)
    Ok(PathBuf::from(".claude/repositories.toml"))
}

/// Walk up the directory tree searching for existing .claude/repositories.toml file
fn walk_up_for_repositories_toml(start_path: &Path) -> Option<PathBuf> {
    let mut current = start_path.to_path_buf();

    loop {
        let repositories_toml = current.join(".claude/repositories.toml");
        if repositories_toml.is_file() {
            return Some(repositories_toml.canonicalize().unwrap_or(repositories_toml));
        }

        // Check if we've reached the filesystem root
        if !current.pop() {
            break;
        }
    }

    None
}

/// Resolve skills storage directory
/// Priority:
/// 1. skills_directory from .fastskill.yaml (if exists)
/// 2. Walk up directory tree to find existing .claude/skills/
/// 3. Default to .claude/skills/ in current directory (but don't auto-create)
pub fn resolve_skills_storage_directory() -> CliResult<PathBuf> {
    // Load configuration from file if available
    let config_file = crate::cli::config_file::load_config()?;

    // Priority 1: Check if skills_directory is configured in .fastskill.yaml
    if let Some(config) = &config_file {
        if let Some(skills_dir) = &config.skills_directory {
            let path = if skills_dir.is_absolute() {
                skills_dir.clone()
            } else {
                // Resolve relative to current directory
                env::current_dir()
                    .map_err(|e| {
                        CliError::Config(format!("Failed to get current directory: {}", e))
                    })?
                    .join(skills_dir)
            };

            // If the directory exists, use it
            if path.is_dir() {
                debug!(
                    "Using skills_directory from .fastskill.yaml: {}",
                    path.display()
                );
                return Ok(path.canonicalize().unwrap_or(path));
            }

            // If it doesn't exist but is explicitly configured, still return it
            // (caller will handle creation if needed)
            debug!(
                "Using configured skills_directory (may not exist yet): {}",
                path.display()
            );
            return Ok(path);
        }
    }

    // Priority 2: Walk up directory tree to find existing .claude/skills/
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    if let Some(found_dir) = walk_up_for_skills_dir(current_dir.as_path()) {
        debug!(
            "Found .claude/skills by walking up: {}",
            found_dir.display()
        );
        return Ok(found_dir);
    }

    // Priority 3: Default to .claude/skills/ in current directory (but don't auto-create)
    let default_dir = current_dir.join(".claude/skills");
    debug!("Using default skills directory: {}", default_dir.display());
    Ok(default_dir)
}

/// Create service configuration with resolved skills directory
pub fn create_service_config(
    _skills_dir_override: Option<PathBuf>,
    _sources_path_override: Option<PathBuf>,
) -> CliResult<ServiceConfig> {
    // Resolve skills storage directory from .fastskill.yaml or default location
    let resolved_dir = resolve_skills_storage_directory()?;

    // Load configuration from file if available
    let config_file = crate::cli::config_file::load_config()?;

    // Extract embedding config from file
    let embedding_config = config_file.and_then(|config| config.embedding).map(|embedding| {
        fastskill::EmbeddingConfig {
            openai_base_url: embedding.openai_base_url,
            embedding_model: embedding.embedding_model,
            index_path: embedding.index_path,
        }
    });

    // Read registry configuration from environment variables
    let registry_blob_storage =
        if let (Ok(bucket), Ok(region)) = (env::var("S3_BUCKET"), env::var("S3_REGION")) {
            let access_key = env::var("AWS_ACCESS_KEY_ID").unwrap_or_default();
            let secret_key = env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default();
            let endpoint = env::var("S3_ENDPOINT").ok();
            let base_url = env::var("BLOB_BASE_URL").ok();

            if !bucket.is_empty() && !region.is_empty() {
                Some(BlobStorageConfig {
                    storage_type: "s3".to_string(),
                    base_path: String::new(),
                    bucket,
                    region,
                    endpoint,
                    access_key,
                    secret_key,
                    base_url,
                })
            } else {
                None
            }
        } else {
            None
        };

    // Read registry index path from environment
    let registry_index_path = env::var("REGISTRY_INDEX_PATH").ok().map(PathBuf::from);

    // Read staging directory from environment or use default
    let staging_dir = env::var("REGISTRY_STAGING_DIR").ok().map(PathBuf::from);

    Ok(ServiceConfig {
        skill_storage_path: resolved_dir,
        embedding: embedding_config,
        registry_blob_storage,
        registry_index_path,
        staging_dir,
        ..Default::default()
    })
}

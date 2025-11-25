//! Configuration file parsing for FastSkill CLI

use crate::cli::error::{CliError, CliResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Embedding configuration from .fastskill.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// OpenAI API base URL
    pub openai_base_url: String,
    /// Embedding model name (e.g., "text-embedding-3-small")
    pub embedding_model: String,
    /// Optional custom path for vector index database
    #[serde(default)]
    pub index_path: Option<PathBuf>,
}

/// Main configuration structure for .fastskill.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastSkillConfig {
    /// Embedding service configuration
    pub embedding: Option<EmbeddingConfig>,
    /// Skills storage directory (where installed skills are stored)
    /// Default: ".claude/skills"
    #[serde(default)]
    pub skills_directory: Option<PathBuf>,
}

/// Load configuration from .fastskill.yaml file
/// Priority: current directory -> walk up directory tree -> user home config directory
pub fn load_config() -> CliResult<Option<FastSkillConfig>> {
    // Try current directory first
    if let Ok(config) = load_config_from_path(".fastskill.yaml") {
        return Ok(Some(config));
    }

    // Try walking up directory tree
    if let Some(found_config) = walk_up_for_config() {
        if let Ok(config) = load_config_from_path(&found_config) {
            return Ok(Some(config));
        }
    }

    // Try user home config directory
    if let Some(home_dir) = dirs::home_dir() {
        let home_config = home_dir.join(".fastskill").join("config.yaml");
        if let Ok(config) = load_config_from_path(&home_config) {
            return Ok(Some(config));
        }
    }

    // No configuration found
    Ok(None)
}

/// Walk up the directory tree searching for `.fastskill.yaml` file
fn walk_up_for_config() -> Option<PathBuf> {
    let current_dir = std::env::current_dir().ok()?;
    let mut current = current_dir;

    loop {
        let config_file = current.join(".fastskill.yaml");
        if config_file.is_file() {
            return Some(config_file);
        }

        // Check if we've reached the filesystem root
        if !current.pop() {
            break;
        }
    }

    None
}

/// Load configuration from a specific file path
fn load_config_from_path<P: AsRef<std::path::Path>>(path: P) -> CliResult<FastSkillConfig> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path).map_err(|e| {
        CliError::Config(format!(
            "Failed to read config file {}: {}",
            path.display(),
            e
        ))
    })?;

    let config: FastSkillConfig = serde_yaml::from_str(&contents).map_err(|e| {
        CliError::Config(format!(
            "Failed to parse config file {}: {}",
            path.display(),
            e
        ))
    })?;

    Ok(config)
}

/// Get OpenAI API key from environment
pub fn get_openai_api_key() -> CliResult<String> {
    std::env::var("OPENAI_API_KEY")
        .map_err(|_| CliError::Config("OPENAI_API_KEY environment variable not set".to_string()))
}

/// Get all searched configuration paths for error reporting
pub fn get_config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Current directory
    paths.push(PathBuf::from(".fastskill.yaml"));

    // Walk up directory tree
    if let Ok(current_dir) = std::env::current_dir() {
        let mut current = current_dir;
        loop {
            let config_file = current.join(".fastskill.yaml");
            if config_file.as_path() != std::path::Path::new(".fastskill.yaml") {
                paths.push(config_file);
            }

            if !current.pop() {
                break;
            }
        }
    }

    // User home config directory
    if let Some(home_dir) = dirs::home_dir() {
        let home_config = home_dir.join(".fastskill").join("config.yaml");
        paths.push(home_config);
    }

    paths
}

//! Configuration file parsing for FastSkill CLI

use crate::cli::error::{CliError, CliResult};
use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::project;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Embedding configuration (runtime version)
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

/// Main configuration structure loaded from skill-project.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastSkillConfig {
    /// Embedding service configuration
    pub embedding: Option<EmbeddingConfig>,
    /// Skills storage directory (where installed skills are stored)
    /// Required in project-level skill-project.toml; no default.
    #[serde(default)]
    pub skills_directory: Option<PathBuf>,
}

/// Load configuration from skill-project.toml [tool.fastskill] section
pub fn load_config() -> CliResult<Option<FastSkillConfig>> {
    let current_dir = std::env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    load_config_from_skill_project(&current_dir)
}

/// Load configuration from skill-project.toml in current directory or parent directories
pub fn load_config_from_skill_project(current_dir: &Path) -> CliResult<Option<FastSkillConfig>> {
    // Try to find skill-project.toml walking up the directory tree
    let project_file = project::resolve_project_file(current_dir);
    if !project_file.found {
        return Ok(None); // No skill-project.toml found
    }

    let project_path = project_file.path;
    let project = SkillProjectToml::load_from_file(&project_path).map_err(|e| {
        CliError::Config(format!(
            "Failed to load skill-project.toml from {}: {}",
            project_path.display(),
            e
        ))
    })?;

    // Extract [tool.fastskill] configuration
    let tool_config = project.tool.and_then(|t| t.fastskill);

    if let Some(config) = tool_config {
        // Convert EmbeddingConfigToml to EmbeddingConfig
        let embedding = config.embedding.map(|e| EmbeddingConfig {
            openai_base_url: e.openai_base_url,
            embedding_model: e.embedding_model,
            index_path: e.index_path,
        });

        Ok(Some(FastSkillConfig {
            embedding,
            skills_directory: config.skills_directory,
        }))
    } else {
        // skill-project.toml exists but no [tool.fastskill] section
        Ok(None)
    }
}

/// Get OpenAI API key from environment
pub fn get_openai_api_key() -> CliResult<String> {
    std::env::var("OPENAI_API_KEY")
        .map_err(|_| CliError::Config("OPENAI_API_KEY environment variable not set".to_string()))
}

/// Get all searched configuration paths for error reporting
pub fn get_config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Walk up directory tree for skill-project.toml
    if let Ok(current_dir) = std::env::current_dir() {
        let mut current = current_dir;
        loop {
            let project_file = current.join("skill-project.toml");
            paths.push(project_file);

            if !current.pop() {
                break;
            }
        }
    }

    paths
}

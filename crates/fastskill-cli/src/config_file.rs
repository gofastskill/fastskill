//! Configuration file parsing for FastSkill CLI

use crate::error::{CliError, CliResult};
use fastskill_core::core::manifest::SkillProjectToml;
use fastskill_core::core::project;
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
    /// HTTP server configuration
    #[serde(default)]
    pub server: Option<HttpServerConfig>,
    /// Automatically reindex after add/install/update/remove (default: true)
    #[serde(default = "default_true")]
    pub auto_reindex: bool,
}

fn default_true() -> bool {
    true
}

/// HTTP server configuration (CLI version)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServerConfig {
    /// List of origins allowed for CORS
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Optional: allow list of request headers
    #[serde(default = "default_allowed_headers_config")]
    pub allowed_headers: Vec<String>,
}

fn default_allowed_headers_config() -> Vec<String> {
    vec!["Content-Type".to_string(), "Authorization".to_string()]
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
        // Convert EmbeddingConfigToml to EmbeddingConfig, then let the
        // environment override the non-secret fields (K4).
        let embedding = config.embedding.map(|e| {
            embedding_with_env_overrides(EmbeddingConfig {
                openai_base_url: e.openai_base_url,
                embedding_model: e.embedding_model,
                index_path: e.index_path,
            })
        });

        // Convert HttpServerConfigToml to HttpServerConfig
        let server = config.server.map(|s| HttpServerConfig {
            allowed_origins: s.allowed_origins,
            allowed_headers: s.allowed_headers,
        });

        Ok(Some(FastSkillConfig {
            embedding,
            skills_directory: config.skills_directory,
            server,
            auto_reindex: config.auto_reindex,
        }))
    } else {
        // skill-project.toml exists but no [tool.fastskill] section
        Ok(None)
    }
}

/// Environment override for [`EmbeddingConfig::openai_base_url`].
///
/// Named to match `OPENAI_API_KEY` and the variable the OpenAI SDKs themselves
/// read, so pointing FastSkill at an OpenAI-compatible gateway uses the same
/// spelling as every other tool in the stack.
pub const ENV_OPENAI_BASE_URL: &str = "OPENAI_BASE_URL";

/// Environment override for [`EmbeddingConfig::embedding_model`].
///
/// Deliberately *not* `OPENAI_EMBEDDING_MODEL`: there is no such OpenAI
/// convention, and the value is frequently not an OpenAI model at all (a
/// self-hosted gateway may serve something else entirely).
pub const ENV_EMBEDDING_MODEL: &str = "FASTSKILL_EMBEDDING_MODEL";

/// Apply environment overrides to the embedding config read from the manifest.
///
/// **Environment wins over the manifest.** `skill-project.toml` is committed and
/// shared by everyone on the project; the environment is per-deployment. Pointing
/// one machine (or one CI job) at a different embedding endpoint must not require
/// editing — let alone committing — a tracked file.
///
/// This closes the asymmetry recorded as K4: the base URL and model were
/// manifest-only while the API key was environment-only, so a gateway
/// redirection had to be split across two mechanisms. Note the fix is to make the
/// *non-secret* fields environment-overridable, **not** to add an API-key field
/// to the manifest — a committed file is the wrong home for a secret.
///
/// Takes a lookup closure rather than reading the process environment directly so
/// it can be tested without `set_var`, which is process-global and races with any
/// other test running in parallel.
fn apply_embedding_env_overrides(
    mut embedding: EmbeddingConfig,
    lookup: impl Fn(&str) -> Option<String>,
) -> EmbeddingConfig {
    // An empty or whitespace-only value is treated as unset. `FOO=` in a shell
    // profile or a CI matrix that leaves a variable blank should not silently
    // blank out a working manifest setting.
    let present = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());

    if let Some(base_url) = present(ENV_OPENAI_BASE_URL) {
        embedding.openai_base_url = base_url.trim().to_string();
    }
    if let Some(model) = present(ENV_EMBEDDING_MODEL) {
        embedding.embedding_model = model.trim().to_string();
    }
    embedding
}

/// [`apply_embedding_env_overrides`] against the real process environment.
pub fn embedding_with_env_overrides(embedding: EmbeddingConfig) -> EmbeddingConfig {
    apply_embedding_env_overrides(embedding, |name| std::env::var(name).ok())
}

/// Get OpenAI API key from environment
pub fn get_openai_api_key() -> CliResult<String> {
    std::env::var("OPENAI_API_KEY")
        .map_err(|_| CliError::Config("OPENAI_API_KEY environment variable not set".to_string()))
}

/// Load the auto_reindex setting from config (defaults to true)
pub fn load_auto_reindex_config() -> bool {
    if let Ok(Some(config)) = load_config() {
        config.auto_reindex
    } else {
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod embedding_env_override_tests {
    use super::*;
    use std::collections::HashMap;

    fn manifest_config() -> EmbeddingConfig {
        EmbeddingConfig {
            openai_base_url: "https://api.openai.com/v1".to_string(),
            embedding_model: "text-embedding-3-small".to_string(),
            index_path: None,
        }
    }

    /// Build a lookup over a fixed map — no process env, so these tests are
    /// safe to run in parallel with everything else.
    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    #[test]
    fn manifest_values_survive_when_no_env_is_set() {
        let out = apply_embedding_env_overrides(manifest_config(), env_of(&[]));
        assert_eq!(out.openai_base_url, "https://api.openai.com/v1");
        assert_eq!(out.embedding_model, "text-embedding-3-small");
    }

    #[test]
    fn env_overrides_base_url_and_model() {
        let out = apply_embedding_env_overrides(
            manifest_config(),
            env_of(&[
                (ENV_OPENAI_BASE_URL, "https://llm-gateway.example.ts.net/v1"),
                (ENV_EMBEDDING_MODEL, "kalm-embed"),
            ]),
        );
        assert_eq!(out.openai_base_url, "https://llm-gateway.example.ts.net/v1");
        assert_eq!(out.embedding_model, "kalm-embed");
    }

    #[test]
    fn each_override_is_independent() {
        let out = apply_embedding_env_overrides(
            manifest_config(),
            env_of(&[(ENV_OPENAI_BASE_URL, "https://gateway.internal/v1")]),
        );
        assert_eq!(out.openai_base_url, "https://gateway.internal/v1");
        assert_eq!(
            out.embedding_model, "text-embedding-3-small",
            "overriding the base URL must not disturb the model"
        );
    }

    /// A blank value in a shell profile or CI matrix must not silently blank out
    /// a working manifest setting — that failure is near-impossible to diagnose
    /// from the resulting "connection refused to ''".
    #[test]
    fn blank_env_values_are_treated_as_unset() {
        let out = apply_embedding_env_overrides(
            manifest_config(),
            env_of(&[(ENV_OPENAI_BASE_URL, ""), (ENV_EMBEDDING_MODEL, "   ")]),
        );
        assert_eq!(out.openai_base_url, "https://api.openai.com/v1");
        assert_eq!(out.embedding_model, "text-embedding-3-small");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let out = apply_embedding_env_overrides(
            manifest_config(),
            env_of(&[(ENV_EMBEDDING_MODEL, "  kalm-embed\n")]),
        );
        assert_eq!(out.embedding_model, "kalm-embed");
    }

    #[test]
    fn index_path_is_never_touched_by_env() {
        let mut cfg = manifest_config();
        cfg.index_path = Some(PathBuf::from("/custom/index"));
        let out = apply_embedding_env_overrides(
            cfg,
            env_of(&[(ENV_OPENAI_BASE_URL, "https://gateway.internal/v1")]),
        );
        assert_eq!(out.index_path, Some(PathBuf::from("/custom/index")));
    }
}

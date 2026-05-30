//! Type definitions for the sources system.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::SourcesError;

/// Main sources configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesConfig {
    #[serde(default)]
    pub sources: Vec<SourceDefinition>,
}

impl SourcesConfig {
    /// Load sources config from TOML file
    pub fn load_from_file(path: &Path) -> Result<Self, SourcesError> {
        if !path.exists() {
            return Err(SourcesError::NotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path).map_err(SourcesError::Io)?;

        let config: SourcesConfig =
            toml::from_str(&content).map_err(|e| SourcesError::Parse(e.to_string()))?;

        Ok(config)
    }

    /// Save sources config to TOML file
    pub fn save_to_file(&self, path: &Path) -> Result<(), SourcesError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| SourcesError::Serialize(e.to_string()))?;

        crate::utils::atomic_write(path, content.as_bytes()).map_err(SourcesError::Io)?;

        Ok(())
    }
}

/// Source definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDefinition {
    pub name: String,
    #[serde(default = "default_priority")]
    pub priority: u32,
    pub source: SourceConfig,
}

impl SourceDefinition {
    /// Check if this source supports listing skills
    /// Returns true for git-marketplace, zip-url, and local sources
    /// Returns false for HTTP registries (http-registry)
    pub fn supports_listing(&self) -> bool {
        matches!(
            &self.source,
            SourceConfig::Git { .. } | SourceConfig::ZipUrl { .. } | SourceConfig::Local { .. }
        )
    }
}

/// Default priority value (0 = highest priority)
pub fn default_priority() -> u32 {
    0
}

/// Source authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SourceAuth {
    #[serde(rename = "pat")]
    Pat { env_var: String },
    #[serde(rename = "ssh-key")]
    SshKey { path: PathBuf },
    #[serde(rename = "basic")]
    Basic {
        username: String,
        password_env: String,
    },
}

/// Source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SourceConfig {
    #[serde(rename = "git")]
    Git {
        url: String,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        tag: Option<String>,
        #[serde(default)]
        auth: Option<SourceAuth>,
    },
    #[serde(rename = "zip-url")]
    ZipUrl {
        base_url: String,
        #[serde(default)]
        auth: Option<SourceAuth>,
    },
    #[serde(rename = "local")]
    Local { path: PathBuf },
}

/// Information about a skill available from a source
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub source_name: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_source_config_variants() {
        let git_config = SourceConfig::Git {
            url: "https://github.com/org/repo.git".to_string(),
            branch: Some("main".to_string()),
            tag: None,
            auth: None,
        };
        let zip_config = SourceConfig::ZipUrl {
            base_url: "https://skills.example.com/".to_string(),
            auth: None,
        };
        let local_config = SourceConfig::Local {
            path: PathBuf::from("./local-sources"),
        };

        let git_toml = toml::to_string(&git_config).unwrap();
        assert!(git_toml.contains("type = \"git\""));

        let zip_toml = toml::to_string(&zip_config).unwrap();
        assert!(zip_toml.contains("type = \"zip-url\""));

        let local_toml = toml::to_string(&local_config).unwrap();
        assert!(local_toml.contains("type = \"local\""));
    }
}

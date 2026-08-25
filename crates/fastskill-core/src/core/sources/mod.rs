//! Sources system for managing skill repositories

pub mod local;
pub mod manager;
pub mod marketplace;
pub mod model;

// Curated re-exports — everything external callers need
pub use manager::SourcesManager;
pub use marketplace::{
    ClaudeCodeMarketplaceJson, ClaudeCodeMetadata, ClaudeCodeOwner, ClaudeCodePlugin,
    MarketplaceJson, MarketplaceSkill,
};
pub use model::{SkillInfo, SourceAuth, SourceConfig, SourceDefinition, SourcesConfig};

use std::path::PathBuf;

/// Sources-related errors
#[derive(Debug, thiserror::Error)]
pub enum SourcesError {
    #[error("Sources config file not found: {0}")]
    NotFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Serialize error: {0}")]
    Serialize(String),

    #[error("Source already exists: {0}")]
    AlreadyExists(String),

    #[error("Source not found: {0}")]
    SourceNotFound(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Zip URL error: {0}")]
    ZipUrl(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sources_config_parsing() {
        let toml_content = r#"
            [[sources]]
            name = "team-tools"
            source = { type = "git", url = "https://github.com/org/team-plugins.git", branch = "main" }

            [[sources]]
            name = "official-skills"
            source = { type = "zip-url", base_url = "https://skills.example.com/" }

            [[sources]]
            name = "local"
            source = { type = "local", path = "./local-sources" }
        "#;

        let config: SourcesConfig = toml::from_str(toml_content).unwrap();

        assert_eq!(config.sources.len(), 3);
        assert_eq!(config.sources[0].name, "team-tools");
        assert_eq!(config.sources[1].name, "official-skills");
        assert_eq!(config.sources[2].name, "local");
    }

    #[tokio::test]
    async fn test_sources_manager() {
        // A TempDir, not a NamedTempFile: the latter keeps the file open, and
        // Windows refuses to rewrite a file another handle still holds, so
        // `manager.save()` fails there with access denied. The config file
        // does not need to exist up front.
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("sources.toml");

        let mut manager = SourcesManager::new(config_path.clone());

        manager.load().unwrap();
        assert_eq!(manager.list_sources().len(), 0);

        manager
            .add_source(
                "team-tools".to_string(),
                SourceConfig::Git {
                    url: "https://github.com/org/team-plugins.git".to_string(),
                    branch: Some("main".to_string()),
                    tag: None,
                    auth: None,
                },
            )
            .unwrap();

        manager
            .add_source(
                "official".to_string(),
                SourceConfig::ZipUrl {
                    base_url: "https://skills.example.com/".to_string(),
                    auth: None,
                },
            )
            .unwrap();

        assert_eq!(manager.list_sources().len(), 2);

        manager.save().unwrap();

        let mut new_manager = SourcesManager::new(config_path);
        new_manager.load().unwrap();
        assert_eq!(new_manager.list_sources().len(), 2);
    }
}

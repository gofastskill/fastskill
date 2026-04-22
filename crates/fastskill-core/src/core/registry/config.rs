//! Registry configuration management

use crate::core::service::ServiceError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Registry configuration manager
pub struct RegistryConfigManager {
    config_path: PathBuf,
    config: Option<RegistriesConfig>,
}

/// Main registries configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistriesConfig {
    /// Default registry configuration
    #[serde(default)]
    pub registry: Option<DefaultRegistryConfig>,

    /// Additional named registries
    #[serde(default)]
    pub registries: Vec<RegistryConfig>,
}

/// Default registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultRegistryConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub registry_type: String,
    pub index_url: String,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    #[serde(default)]
    pub storage: Option<StorageConfig>,
}

/// Registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub registry_type: String,
    pub index_url: String,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    #[serde(default)]
    pub storage: Option<StorageConfig>,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthConfig {
    #[serde(rename = "pat")]
    Pat { env_var: String },
    #[serde(rename = "ssh")]
    Ssh { key_path: PathBuf },
    #[serde(rename = "api_key")]
    ApiKey { env_var: String },
}

/// Storage backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: String,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

impl RegistryConfigManager {
    /// Create a new registry config manager
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            config: None,
        }
    }

    /// Load configuration from file
    pub fn load(&mut self) -> Result<(), ServiceError> {
        if !self.config_path.exists() {
            // Return empty config if file doesn't exist
            self.config = Some(RegistriesConfig {
                registry: None,
                registries: Vec::new(),
            });
            return Ok(());
        }

        let content = fs::read_to_string(&self.config_path).map_err(ServiceError::Io)?;

        let config: RegistriesConfig = toml::from_str(&content).map_err(|e| {
            ServiceError::Custom(format!("Failed to parse registries config: {}", e))
        })?;

        self.config = Some(config);
        Ok(())
    }

    /// Get default registry configuration
    pub fn get_default_registry(&self) -> Result<Option<RegistryConfig>, ServiceError> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| ServiceError::Custom("Registry config not loaded".to_string()))?;

        if let Some(ref default) = config.registry {
            Ok(Some(RegistryConfig {
                name: default.name.clone(),
                registry_type: default.registry_type.clone(),
                index_url: default.index_url.clone(),
                auth: default.auth.clone(),
                storage: default.storage.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get registry by name
    pub fn get_registry(&self, name: &str) -> Result<Option<RegistryConfig>, ServiceError> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| ServiceError::Custom("Registry config not loaded".to_string()))?;

        // Check if it's the default registry
        if let Some(ref default) = config.registry {
            if default.name == name {
                return Ok(Some(RegistryConfig {
                    name: default.name.clone(),
                    registry_type: default.registry_type.clone(),
                    index_url: default.index_url.clone(),
                    auth: default.auth.clone(),
                    storage: default.storage.clone(),
                }));
            }
        }

        // Check named registries
        for registry in &config.registries {
            if registry.name == name {
                return Ok(Some(registry.clone()));
            }
        }

        Ok(None)
    }

    /// List all configured registries
    pub fn list_registries(&self) -> Result<Vec<String>, ServiceError> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| ServiceError::Custom("Registry config not loaded".to_string()))?;

        let mut names = Vec::new();

        if let Some(ref default) = config.registry {
            names.push(default.name.clone());
        }

        for registry in &config.registries {
            names.push(registry.name.clone());
        }

        Ok(names)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_registry_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("registries.toml");

        let config_content = r#"
[registry]
name = "primary"
type = "github"
index_url = "https://github.com/org/registry-index"

[[registries]]
name = "enterprise"
type = "github"
index_url = "https://github.com/org/enterprise-registry"
"#;

        std::fs::write(&config_path, config_content).unwrap();

        let mut manager = RegistryConfigManager::new(config_path);
        manager.load().unwrap();

        let default = manager.get_default_registry().unwrap();
        assert!(default.is_some());
        assert_eq!(default.unwrap().name, "primary");

        let enterprise = manager.get_registry("enterprise").unwrap();
        assert!(enterprise.is_some());
        assert_eq!(enterprise.unwrap().name, "enterprise");
    }
}

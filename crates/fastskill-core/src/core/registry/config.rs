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
    /// `Authorization: Bearer <token>` from an environment variable (ADR-0018).
    #[serde(rename = "bearer")]
    Bearer { env_var: String },
    /// `Authorization: Bearer <token>` printed by a program (ADR-0018).
    #[serde(rename = "command")]
    Command { command: Vec<String> },
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

    #[test]
    fn a_missing_file_loads_as_empty_and_lookups_need_a_load() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = RegistryConfigManager::new(temp_dir.path().join("absent.toml"));
        for result in [
            manager.get_default_registry().map(|_| ()),
            manager.get_registry("x").map(|_| ()),
            manager.list_registries().map(|_| ()),
        ] {
            assert!(result.unwrap_err().to_string().contains("not loaded"));
        }

        manager.load().unwrap();
        assert!(manager.get_default_registry().unwrap().is_none());
        assert!(manager.get_registry("x").unwrap().is_none());
        assert!(manager.list_registries().unwrap().is_empty());
    }

    #[test]
    fn an_invalid_file_is_a_parse_error() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("registries.toml");
        std::fs::write(&config_path, "registries = 3").unwrap();
        let error = RegistryConfigManager::new(config_path)
            .load()
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Failed to parse registries config"),
            "{error}"
        );
    }

    #[test]
    fn bearer_and_command_auth_parse_and_registries_are_listed() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("registries.toml");
        std::fs::write(
            &config_path,
            r#"
[registry]
name = "primary"
type = "http"
index_url = "https://registry.example/index"
auth = { type = "bearer", env_var = "REGISTRY_TOKEN" }

[[registries]]
name = "second"
type = "http"
index_url = "https://other.example/index"
auth = { type = "command", command = ["my-login", "token"] }
"#,
        )
        .unwrap();
        let mut manager = RegistryConfigManager::new(config_path);
        manager.load().unwrap();

        assert_eq!(manager.list_registries().unwrap(), ["primary", "second"]);
        let primary = manager.get_registry("primary").unwrap().unwrap();
        assert!(matches!(
            primary.auth,
            Some(AuthConfig::Bearer { ref env_var }) if env_var == "REGISTRY_TOKEN"
        ));
        let second = manager.get_registry("second").unwrap().unwrap();
        assert!(matches!(
            second.auth,
            Some(AuthConfig::Command { ref command }) if command == &["my-login", "token"]
        ));
        assert!(manager.get_registry("third").unwrap().is_none());
    }
}

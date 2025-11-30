//! Registry client for querying and downloading from skill registries

use crate::core::metadata::SkillMetadata;
use crate::core::registry::auth::Auth;
use crate::core::registry::config::RegistryConfig;
use crate::core::registry_index::{Dependency as RegistryDependency, IndexMetadata};
use crate::core::service::ServiceError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;

/// Registry client for querying and downloading skills
pub struct RegistryClient {
    config: RegistryConfig,
    client: Client,
    auth: Option<Box<dyn Auth>>,
}

/// Index entry for a skill version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub vers: String,
    pub deps: Vec<RegistryDependency>,
    pub cksum: String,
    pub features: HashMap<String, Vec<String>>,
    pub yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    pub download_url: String,
    #[serde(default)]
    pub metadata: Option<IndexMetadata>,
}

// IndexMetadata is defined in registry_index.rs

// Dependency is defined in registry_index.rs

impl RegistryClient {
    /// Create a new registry client
    pub fn new(config: RegistryConfig) -> Result<Self, ServiceError> {
        let client = Client::builder()
            .user_agent("fastskill/0.6.8")
            .build()
            .map_err(|e| ServiceError::Custom(format!("Failed to create HTTP client: {}", e)))?;

        // Setup authentication
        let auth: Option<Box<dyn Auth>> = if let Some(ref auth_config) = config.auth {
            match auth_config {
                crate::core::registry::config::AuthConfig::Pat { env_var } => Some(Box::new(
                    crate::core::registry::auth::GitHubPat::new(env_var.clone()),
                )),
                crate::core::registry::config::AuthConfig::Ssh { key_path } => Some(Box::new(
                    crate::core::registry::auth::SshKey::new(key_path.clone()),
                )),
                crate::core::registry::config::AuthConfig::ApiKey { env_var } => Some(Box::new(
                    crate::core::registry::auth::ApiKey::new(env_var.clone()),
                )),
            }
        } else {
            None
        };

        Ok(Self {
            config,
            client,
            auth,
        })
    }

    /// Get the index URL for a skill (flat layout: scope/skill-name)
    fn get_index_url(&self, skill_id: &str) -> String {
        // Flat layout: use skill_id directly (e.g., "dev-user/test-skill")
        // index_url should be the base URL for the index (e.g., http://159.69.182.11:8080/index)
        format!("{}/{}", self.config.index_url.trim_end_matches('/'), skill_id)
    }

    /// Get skill information from registry
    /// Returns all versions for the skill (reads single file with newline-delimited JSON)
    pub async fn get_skill(&self, name: &str) -> Result<Vec<IndexEntry>, ServiceError> {
        let url = self.get_index_url(name);

        let mut request = self.client.get(&url);

        // Add authentication if available
        if let Some(ref auth) = self.auth {
            if auth.is_configured() {
                if let Ok(header_value) = auth.get_auth_header() {
                    request = request.header("Authorization", header_value);
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to fetch skill index: {}", e)))?;

        if !response.status().is_success() {
            if response.status() == 404 {
                return Ok(Vec::new()); // Skill not found
            }
            return Err(ServiceError::Custom(format!(
                "Failed to fetch skill index: HTTP {}",
                response.status()
            )));
        }

        let content = response
            .text()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read index file: {}", e)))?;

        // Parse line-by-line (newline-delimited JSON)
        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<IndexEntry>(line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    // Log error but continue parsing other lines
                    eprintln!(
                        "Warning: Failed to parse index entry: {} (line: {})",
                        e, line
                    );
                }
            }
        }

        Ok(entries)
    }

    /// Get index entry for a specific skill version
    async fn get_index_entry(
        &self,
        name: &str,
        version: &str,
    ) -> Result<Option<IndexEntry>, ServiceError> {
        // Get all versions and find the matching one
        let entries = self.get_skill(name).await?;
        Ok(entries.into_iter().find(|e| e.vers == version))
    }

    /// Get available versions for a skill
    pub async fn get_versions(&self, name: &str) -> Result<Vec<String>, ServiceError> {
        let entries = self.get_skill(name).await?;
        let mut versions: Vec<String> = entries.iter().map(|e| e.vers.clone()).collect();
        // Sort by semantic version (newest first)
        versions.sort_by(|a, b| {
            // Simple reverse string comparison for now
            // TODO: Use semver crate for proper version comparison
            b.cmp(a)
        });
        Ok(versions)
    }

    /// Get latest version for a skill (excluding pre-releases by default)
    pub async fn get_latest_version(
        &self,
        name: &str,
        include_pre_release: bool,
    ) -> Result<Option<String>, ServiceError> {
        let versions = self.get_versions(name).await?;

        if versions.is_empty() {
            return Ok(None);
        }

        // Filter out pre-releases if not requested
        let candidates: Vec<&String> = if include_pre_release {
            versions.iter().collect()
        } else {
            versions
                .iter()
                .filter(|v| !v.contains('-')) // Simple pre-release detection
                .collect()
        };

        if candidates.is_empty() {
            // If all are pre-releases and not requested, return None
            return Ok(None);
        }

        // Use semver crate for proper version comparison
        use semver::Version;
        let mut parsed_versions: Vec<(Version, &String)> = candidates
            .iter()
            .filter_map(|v| {
                // Try to parse as semver, skip if invalid
                Version::parse(v).ok().map(|ver| (ver, *v))
            })
            .collect();

        if parsed_versions.is_empty() {
            // Fallback to first version if none can be parsed as semver
            return Ok(Some(candidates[0].clone()));
        }

        // Sort by version (highest first)
        parsed_versions.sort_by(|a, b| b.0.cmp(&a.0));

        Ok(Some(parsed_versions[0].1.clone()))
    }

    /// Get specific version of a skill
    pub async fn get_version(
        &self,
        name: &str,
        version: &str,
    ) -> Result<Option<IndexEntry>, ServiceError> {
        self.get_index_entry(name, version).await
    }

    /// Download skill package
    pub async fn download(&self, name: &str, version: &str) -> Result<Vec<u8>, ServiceError> {
        let entry = self.get_version(name, version).await?.ok_or_else(|| {
            ServiceError::Custom(format!(
                "Skill {} version {} not found in registry",
                name, version
            ))
        })?;

        if entry.yanked {
            return Err(ServiceError::Custom(format!(
                "Skill {} version {} has been yanked",
                name, version
            )));
        }

        let mut request = self.client.get(&entry.download_url);

        // Add authentication if available
        if let Some(ref auth) = self.auth {
            if auth.is_configured() {
                if let Ok(header_value) = auth.get_auth_header() {
                    request = request.header("Authorization", header_value);
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to download package: {}", e)))?;

        if !response.status().is_success() {
            return Err(ServiceError::Custom(format!(
                "Failed to download package: HTTP {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read package data: {}", e)))?;

        // Verify checksum
        let calculated = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
        if calculated != entry.cksum {
            return Err(ServiceError::Custom(format!(
                "Checksum mismatch: expected {}, got {}",
                entry.cksum, calculated
            )));
        }

        Ok(bytes.to_vec())
    }

    /// Search skills in registry (basic implementation - scans index)
    pub async fn search(&self, _query: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        // For now, return empty - full search requires scanning entire index
        // This would be implemented by fetching a search endpoint or scanning index
        // For GitHub-based registries, we'd need to use GitHub API or clone the repo
        Ok(Vec::new())
    }
}

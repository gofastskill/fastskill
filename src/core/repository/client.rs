//! Repository client abstraction for unified skill access

use crate::core::metadata::SkillMetadata;
use crate::core::registry::{RegistryClient, RegistryConfig as OldRegistryConfig};
use crate::core::registry_index::{ListSkillsOptions, SkillSummary};
use crate::core::repository::{RepositoryConfig, RepositoryDefinition, RepositoryType};
use crate::core::service::{ServiceError, SkillId};
use crate::core::sources::{SourceConfig, SourceDefinition, SourcesManager};
use reqwest::Client;
use std::sync::Arc;

/// Error type for repository client operations
#[derive(Debug, thiserror::Error)]
pub enum RepositoryClientError {
    #[error("Repository error: {0}")]
    Service(#[from] ServiceError),
    #[error("Client error: {0}")]
    Client(String),
    #[error("Not implemented for this repository type")]
    NotImplemented,
}

/// Trait for unified repository clients
#[async_trait::async_trait]
pub trait RepositoryClient: Send + Sync {
    /// List all skills in the repository
    async fn list_skills(&self) -> Result<Vec<SkillMetadata>, RepositoryClientError>;

    /// Get a specific skill by ID and optional version
    async fn get_skill(
        &self,
        id: &str,
        version: Option<&str>,
    ) -> Result<Option<SkillMetadata>, RepositoryClientError>;

    /// Search for skills matching a query
    async fn search(&self, query: &str) -> Result<Vec<SkillMetadata>, RepositoryClientError>;

    /// Download a skill package
    async fn download(&self, id: &str, version: &str) -> Result<Vec<u8>, RepositoryClientError>;

    /// Get all versions for a skill
    async fn get_versions(&self, id: &str) -> Result<Vec<String>, RepositoryClientError>;
}

/// Create a repository client from a repository definition
pub async fn create_client(
    repo: &RepositoryDefinition,
) -> Result<Arc<dyn RepositoryClient + Send + Sync>, ServiceError> {
    match repo.repo_type {
        RepositoryType::GitMarketplace | RepositoryType::ZipUrl | RepositoryType::Local => {
            Ok(Arc::new(MarketplaceRepositoryClient::new(repo)?))
        }
        RepositoryType::HttpRegistry => Ok(Arc::new(CratesRegistryClient::new(repo)?)),
    }
}

/// Marketplace-based repository client (wraps SourcesManager logic)
pub struct MarketplaceRepositoryClient {
    sources_manager: SourcesManager,
    source_name: String,
}

impl MarketplaceRepositoryClient {
    pub fn new(repo: &RepositoryDefinition) -> Result<Self, ServiceError> {
        // Convert RepositoryConfig to SourceConfig
        let source_config = match &repo.config {
            RepositoryConfig::GitMarketplace { url, branch, tag } => {
                // Convert auth
                let auth = repo.auth.as_ref().and_then(|a| {
                    match a {
                        crate::core::repository::RepositoryAuth::Pat { env_var } => {
                            Some(crate::core::sources::SourceAuth::Pat {
                                env_var: env_var.clone(),
                            })
                        }
                        crate::core::repository::RepositoryAuth::SshKey { path } => {
                            Some(crate::core::sources::SourceAuth::SshKey { path: path.clone() })
                        }
                        crate::core::repository::RepositoryAuth::Basic {
                            username,
                            password_env,
                        } => Some(crate::core::sources::SourceAuth::Basic {
                            username: username.clone(),
                            password_env: password_env.clone(),
                        }),
                        _ => None, // Other auth types not supported for marketplace
                    }
                });

                SourceConfig::Git {
                    url: url.clone(),
                    branch: branch.clone(),
                    tag: tag.clone(),
                    auth,
                }
            }
            RepositoryConfig::ZipUrl { base_url } => {
                let auth = repo.auth.as_ref().and_then(|a| match a {
                    crate::core::repository::RepositoryAuth::Pat { env_var } => {
                        Some(crate::core::sources::SourceAuth::Pat {
                            env_var: env_var.clone(),
                        })
                    }
                    crate::core::repository::RepositoryAuth::Basic {
                        username,
                        password_env,
                    } => Some(crate::core::sources::SourceAuth::Basic {
                        username: username.clone(),
                        password_env: password_env.clone(),
                    }),
                    _ => None,
                });

                SourceConfig::ZipUrl {
                    base_url: base_url.clone(),
                    auth,
                }
            }
            RepositoryConfig::Local { path } => SourceConfig::Local { path: path.clone() },
            RepositoryConfig::HttpRegistry { .. } => {
                return Err(ServiceError::Custom(
                    "HttpRegistry should use CratesRegistryClient".to_string(),
                ));
            }
        };

        // Create a temporary sources manager with just this source
        let temp_path = std::env::temp_dir().join(format!("fastskill-repo-{}", repo.name));
        let mut sources_manager = SourcesManager::new(temp_path);
        sources_manager
            .load()
            .map_err(|e| ServiceError::Custom(format!("Failed to load sources manager: {}", e)))?;

        let source_def = SourceDefinition {
            name: repo.name.clone(),
            priority: repo.priority,
            source: source_config,
        };

        sources_manager
            .add_source_with_priority(repo.name.clone(), source_def.source.clone(), repo.priority)
            .map_err(|e| ServiceError::Custom(format!("Failed to add source: {}", e)))?;

        Ok(Self {
            sources_manager,
            source_name: repo.name.clone(),
        })
    }
}

#[async_trait::async_trait]
impl RepositoryClient for MarketplaceRepositoryClient {
    async fn list_skills(&self) -> Result<Vec<SkillMetadata>, RepositoryClientError> {
        let source_def = self
            .sources_manager
            .get_source(&self.source_name)
            .ok_or_else(|| RepositoryClientError::Client("Source not found".to_string()))?;

        let skill_infos = self
            .sources_manager
            .get_skills_from_source(&self.source_name, source_def)
            .await
            .map_err(|e| RepositoryClientError::Client(format!("Failed to get skills: {}", e)))?;

        use crate::core::service::SkillId;
        use chrono::Utc;

        Ok(skill_infos
            .into_iter()
            .map(|info| SkillMetadata {
                id: SkillId::from(info.id.clone()),
                name: info.name,
                description: info.description,
                version: info.version.unwrap_or_else(|| "1.0.0".to_string()),
                author: None,
                enabled: true,
                token_estimate: 0,
                last_updated: Utc::now(),
            })
            .collect())
    }

    async fn get_skill(
        &self,
        id: &str,
        version: Option<&str>,
    ) -> Result<Option<SkillMetadata>, RepositoryClientError> {
        use crate::core::service::SkillId;
        let skill_id = SkillId::from(id.to_string());
        let skills = self.list_skills().await?;
        Ok(skills
            .into_iter()
            .find(|s| s.id == skill_id && version.map(|v| s.version == v).unwrap_or(true)))
    }

    async fn search(&self, query: &str) -> Result<Vec<SkillMetadata>, RepositoryClientError> {
        let skills = self.list_skills().await?;
        let query_lower = query.to_lowercase();
        Ok(skills
            .into_iter()
            .filter(|s| {
                s.id.to_string().to_lowercase().contains(&query_lower)
                    || s.name.to_lowercase().contains(&query_lower)
                    || s.description.to_lowercase().contains(&query_lower)
            })
            .collect())
    }

    async fn download(&self, _id: &str, _version: &str) -> Result<Vec<u8>, RepositoryClientError> {
        // For marketplace repositories, we need to get the download URL from marketplace.json
        // This is a simplified implementation - full implementation would fetch marketplace.json
        Err(RepositoryClientError::NotImplemented)
    }

    async fn get_versions(&self, id: &str) -> Result<Vec<String>, RepositoryClientError> {
        use crate::core::service::SkillId;
        let skill_id = SkillId::from(id.to_string());
        let skills = self.list_skills().await?;
        Ok(skills.into_iter().filter(|s| s.id == skill_id).map(|s| s.version).collect())
    }
}

/// HTTP registry client (wraps RegistryClient logic)
pub struct CratesRegistryClient {
    registry_client: RegistryClient,
    index_url: String,
    auth: Option<crate::core::registry::config::AuthConfig>,
}

impl CratesRegistryClient {
    pub fn new(repo: &RepositoryDefinition) -> Result<Self, ServiceError> {
        let index_url = match &repo.config {
            RepositoryConfig::HttpRegistry { index_url } => index_url.clone(),
            _ => {
                return Err(ServiceError::Custom(
                    "HttpRegistry requires index_url".to_string(),
                ))
            }
        };

        // Validate URL is not empty
        if index_url.trim().is_empty() {
            return Err(ServiceError::Custom(
                "index_url cannot be empty".to_string(),
            ));
        }

        // Validate URL format
        if url::Url::parse(&index_url).is_err() {
            return Err(ServiceError::Custom(format!(
                "Invalid index_url format: {}",
                index_url
            )));
        }

        // Convert auth
        let auth = repo.auth.as_ref().map(|a| {
            match a {
                crate::core::repository::RepositoryAuth::Pat { env_var } => {
                    crate::core::registry::config::AuthConfig::Pat {
                        env_var: env_var.clone(),
                    }
                }
                crate::core::repository::RepositoryAuth::Ssh { key_path } => {
                    crate::core::registry::config::AuthConfig::Ssh {
                        key_path: key_path.clone(),
                    }
                }
                crate::core::repository::RepositoryAuth::ApiKey { env_var } => {
                    crate::core::registry::config::AuthConfig::ApiKey {
                        env_var: env_var.clone(),
                    }
                }
                _ => {
                    // Fallback to PAT for unsupported types
                    crate::core::registry::config::AuthConfig::Pat {
                        env_var: "GITHUB_TOKEN".to_string(),
                    }
                }
            }
        });

        let registry_config = OldRegistryConfig {
            name: repo.name.clone(),
            registry_type: "git".to_string(),
            index_url,
            auth,
            storage: repo.storage.clone().map(|s| crate::core::registry::config::StorageConfig {
                storage_type: s.storage_type,
                repository: s.repository,
                bucket: s.bucket,
                region: s.region,
                endpoint: s.endpoint,
                base_url: s.base_url,
            }),
        };

        let registry_client = RegistryClient::new(registry_config.clone())?;

        Ok(Self {
            registry_client,
            index_url: registry_config.index_url.clone(),
            auth: registry_config.auth.clone(),
        })
    }

    /// Fetch skills from the registry HTTP API endpoint
    pub async fn fetch_skills(
        &self,
        options: &ListSkillsOptions,
    ) -> Result<Vec<SkillSummary>, RepositoryClientError> {
        use crate::core::registry::auth::Auth;

        // Build the API endpoint URL
        let base_url = self.index_url.trim_end_matches('/');
        let mut url = format!("{}/api/registry/index/skills", base_url);

        // Add query parameters
        let mut query_params = Vec::new();
        if let Some(ref scope) = options.scope {
            query_params.push(("scope", scope.clone()));
        }
        if options.all_versions {
            query_params.push(("all_versions", "true".to_string()));
        }
        if options.include_pre_release {
            query_params.push(("include_pre_release", "true".to_string()));
        }

        if !query_params.is_empty() {
            let mut url_obj = url::Url::parse(&url)
                .map_err(|e| RepositoryClientError::Client(format!("Invalid URL: {}", e)))?;
            for (k, v) in query_params {
                url_obj.query_pairs_mut().append_pair(k, &v);
            }
            url = url_obj.to_string();
        }

        // Create HTTP client
        let client = Client::builder().user_agent("fastskill/0.8.6").build().map_err(|e| {
            RepositoryClientError::Client(format!("Failed to create HTTP client: {}", e))
        })?;

        // Build request
        let mut request = client.get(&url);

        // Add authentication if configured
        if let Some(ref auth_config) = self.auth {
            let auth: Option<Box<dyn Auth>> = match auth_config {
                crate::core::registry::config::AuthConfig::Pat { env_var } => Some(Box::new(
                    crate::core::registry::auth::GitHubPat::new(env_var.clone()),
                )),
                crate::core::registry::config::AuthConfig::Ssh { key_path } => Some(Box::new(
                    crate::core::registry::auth::SshKey::new(key_path.clone()),
                )),
                crate::core::registry::config::AuthConfig::ApiKey { env_var } => Some(Box::new(
                    crate::core::registry::auth::ApiKey::new(env_var.clone()),
                )),
            };

            if let Some(auth) = auth {
                if auth.is_configured() {
                    if let Ok(header_value) = auth.get_auth_header() {
                        request = request.header("Authorization", header_value);
                    }
                }
            }
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| RepositoryClientError::Client(format!("HTTP request failed: {}", e)))?;

        // Handle HTTP status codes
        let status = response.status();
        match status.as_u16() {
            200 => {
                // Parse JSON response
                let summaries: Vec<SkillSummary> = response.json().await.map_err(|e| {
                    RepositoryClientError::Client(format!("Failed to parse JSON response: {}", e))
                })?;
                Ok(summaries)
            }
            400 => Err(RepositoryClientError::Client(
                "Bad request: Invalid query parameters".to_string(),
            )),
            401 => Err(RepositoryClientError::Client(
                "Unauthorized: Authentication required for this scope".to_string(),
            )),
            403 => Err(RepositoryClientError::Client(
                "Forbidden: Access denied to this scope".to_string(),
            )),
            404 => Err(RepositoryClientError::Client(
                "Not found: Registry endpoint not available".to_string(),
            )),
            500..=599 => Err(RepositoryClientError::Client(format!(
                "Server error: HTTP {}",
                status
            ))),
            _ => Err(RepositoryClientError::Client(format!(
                "Unexpected HTTP status: {}",
                status
            ))),
        }
    }
}

#[async_trait::async_trait]
impl RepositoryClient for CratesRegistryClient {
    async fn list_skills(&self) -> Result<Vec<SkillMetadata>, RepositoryClientError> {
        // Use default options (no filtering)
        let options = ListSkillsOptions::default();
        let summaries = self.fetch_skills(&options).await?;

        // Convert SkillSummary to SkillMetadata
        let metadata: Vec<SkillMetadata> = summaries
            .into_iter()
            .map(|s| {
                SkillMetadata {
                    id: SkillId::from(s.id.clone()),
                    name: s.name.clone(),
                    description: s.description.clone(),
                    version: s.latest_version.clone(),
                    author: None, // Not available in SkillSummary
                    enabled: true,
                    token_estimate: s.description.len() / 4, // Rough estimate
                    last_updated: s.published_at.unwrap_or_else(chrono::Utc::now),
                }
            })
            .collect();

        Ok(metadata)
    }

    async fn get_skill(
        &self,
        id: &str,
        version: Option<&str>,
    ) -> Result<Option<SkillMetadata>, RepositoryClientError> {
        let entries = self
            .registry_client
            .get_skill(id)
            .await
            .map_err(RepositoryClientError::Service)?;

        if entries.is_empty() {
            return Ok(None);
        }

        // Filter by version if specified
        let entry = if let Some(ver) = version {
            entries.into_iter().find(|e| e.vers == ver)
        } else {
            // Get latest version
            entries.into_iter().max_by_key(|e| e.vers.clone())
        };

        use crate::core::service::SkillId;
        use chrono::Utc;

        Ok(entry.map(|e| SkillMetadata {
            id: SkillId::from(e.name.clone().to_string()),
            name: e.name.clone(),
            description: e
                .metadata
                .as_ref()
                .and_then(|m| m.description.clone())
                .unwrap_or_else(|| "".to_string()),
            version: e.vers,
            author: e.metadata.as_ref().and_then(|m| m.author.clone()),
            enabled: true,
            token_estimate: 0,
            last_updated: Utc::now(),
        }))
    }

    async fn search(&self, query: &str) -> Result<Vec<SkillMetadata>, RepositoryClientError> {
        // Use RegistryClient's search method
        let results = self
            .registry_client
            .search(query)
            .await
            .map_err(RepositoryClientError::Service)?;

        Ok(results)
    }

    async fn download(&self, id: &str, version: &str) -> Result<Vec<u8>, RepositoryClientError> {
        let data = self
            .registry_client
            .download(id, version)
            .await
            .map_err(RepositoryClientError::Service)?;
        Ok(data)
    }

    async fn get_versions(&self, id: &str) -> Result<Vec<String>, RepositoryClientError> {
        let versions = self
            .registry_client
            .get_versions(id)
            .await
            .map_err(RepositoryClientError::Service)?;
        Ok(versions)
    }
}

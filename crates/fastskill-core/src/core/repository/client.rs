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
    /// Unique per-client temp dir backing `sources_manager`. Held so it is not
    /// cleaned up while the client is alive; dropped (and removed) with the client.
    _temp_dir: tempfile::TempDir,
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

        // Create a temporary sources manager with just this source.
        // Use a unique per-operation temp dir (kept alive for this client's
        // lifetime) instead of a predictable, world-readable shared path, so a
        // local user cannot pre-create/symlink it to influence source resolution.
        let temp_dir = tempfile::Builder::new()
            .prefix("fastskill-repo-")
            .tempdir()
            .map_err(|e| {
                ServiceError::Custom(format!("Failed to create temp dir for repository: {}", e))
            })?;
        let temp_path = temp_dir.path().join("sources.toml");
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
            _temp_dir: temp_dir,
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
            .filter_map(|info| {
                SkillId::new(info.id.clone()).ok().map(|id| SkillMetadata {
                    id,
                    name: info.name,
                    description: info.description,
                    version: info.version.unwrap_or_else(|| "1.0.0".to_string()),
                    author: None,
                    token_estimate: 0,
                    last_updated: Utc::now(),
                })
            })
            .collect())
    }

    async fn get_skill(
        &self,
        id: &str,
        version: Option<&str>,
    ) -> Result<Option<SkillMetadata>, RepositoryClientError> {
        use crate::core::service::SkillId;
        let skill_id = SkillId::new(id.to_string())
            .map_err(|e| RepositoryClientError::Client(format!("Invalid skill ID: {}", e)))?;
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
        let skill_id = SkillId::new(id.to_string())
            .map_err(|e| RepositoryClientError::Client(format!("Invalid skill ID: {}", e)))?;
        let skills = self.list_skills().await?;
        Ok(skills
            .into_iter()
            .filter(|s| s.id == skill_id)
            .map(|s| s.version)
            .collect())
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
            storage: repo
                .storage
                .clone()
                .map(|s| crate::core::registry::config::StorageConfig {
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
        let mut url = format!("{}/api/v1/registry/index/skills", base_url);

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
        let client = Client::builder()
            .user_agent("fastskill/0.8.6")
            .build()
            .map_err(|e| {
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
            .filter_map(|s| {
                SkillId::new(s.id.clone()).ok().map(|id| SkillMetadata {
                    id,
                    name: s.name.clone(),
                    description: s.description.clone(),
                    version: s.latest_version.clone(),
                    author: None, // Not available in SkillSummary
                    token_estimate: s.description.len() / 4, // Rough estimate
                    last_updated: s.published_at.unwrap_or_else(chrono::Utc::now),
                })
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

        Ok(entry.and_then(|e| {
            SkillId::new(e.name.clone().to_string())
                .ok()
                .map(|id| SkillMetadata {
                    id,
                    name: e.name.clone(),
                    description: e
                        .metadata
                        .as_ref()
                        .and_then(|m| m.description.clone())
                        .unwrap_or_else(|| "".to_string()),
                    version: e.vers,
                    author: e.metadata.as_ref().and_then(|m| m.author.clone()),
                    token_estimate: 0,
                    last_updated: Utc::now(),
                })
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::core::registry_index::SkillSummary;
    use crate::core::repository::{
        RepositoryAuth, RepositoryConfig, RepositoryDefinition, RepositoryType,
    };
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn http_registry(index_url: &str, auth: Option<RepositoryAuth>) -> RepositoryDefinition {
        RepositoryDefinition {
            name: "reg".to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: index_url.to_string(),
            },
            auth,
            storage: None,
        }
    }

    fn marketplace(config: RepositoryConfig, auth: Option<RepositoryAuth>) -> RepositoryDefinition {
        RepositoryDefinition {
            name: "mp".to_string(),
            repo_type: match &config {
                RepositoryConfig::ZipUrl { .. } => RepositoryType::ZipUrl,
                RepositoryConfig::Local { .. } => RepositoryType::Local,
                _ => RepositoryType::GitMarketplace,
            },
            priority: 0,
            config,
            auth,
            storage: None,
        }
    }

    // ── RepositoryClientError ─────────────────────────────────────────────────

    #[test]
    fn test_repository_client_error_display() {
        let e = RepositoryClientError::Client("boom".to_string());
        assert_eq!(e.to_string(), "Client error: boom");
        assert_eq!(
            RepositoryClientError::NotImplemented.to_string(),
            "Not implemented for this repository type"
        );
        let from_service: RepositoryClientError = ServiceError::Custom("svc".to_string()).into();
        assert!(from_service.to_string().contains("Repository error"));
    }

    // ── create_client ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_client_http_registry() {
        let client = create_client(&http_registry("http://example.com/index", None))
            .await
            .unwrap();
        // A default fetch against a non-existent host errors (not a panic).
        assert!(client.download("a", "1.0.0").await.is_err());
    }

    #[tokio::test]
    async fn test_create_client_marketplace_variants() {
        let tmp = tempfile::tempdir().unwrap();
        for cfg in [
            RepositoryConfig::GitMarketplace {
                url: "https://github.com/org/mp".to_string(),
                branch: None,
                tag: None,
            },
            RepositoryConfig::ZipUrl {
                base_url: "https://example.com/base".to_string(),
            },
            RepositoryConfig::Local {
                path: tmp.path().to_path_buf(),
            },
        ] {
            assert!(create_client(&marketplace(cfg, None)).await.is_ok());
        }
    }

    // ── MarketplaceRepositoryClient::new ──────────────────────────────────────

    #[test]
    fn test_marketplace_new_with_auth_variants() {
        // Git marketplace + PAT auth.
        assert!(MarketplaceRepositoryClient::new(&marketplace(
            RepositoryConfig::GitMarketplace {
                url: "https://github.com/org/mp".to_string(),
                branch: Some("main".to_string()),
                tag: None,
            },
            Some(RepositoryAuth::Pat {
                env_var: "TOK".to_string(),
            }),
        ))
        .is_ok());

        // ZIP URL + Basic auth.
        assert!(MarketplaceRepositoryClient::new(&marketplace(
            RepositoryConfig::ZipUrl {
                base_url: "https://example.com/base".to_string(),
            },
            Some(RepositoryAuth::Basic {
                username: "u".to_string(),
                password_env: "P".to_string(),
            }),
        ))
        .is_ok());

        // Git marketplace + SshKey auth.
        assert!(MarketplaceRepositoryClient::new(&marketplace(
            RepositoryConfig::GitMarketplace {
                url: "https://github.com/org/mp".to_string(),
                branch: None,
                tag: None,
            },
            Some(RepositoryAuth::SshKey {
                path: "/tmp/key".into(),
            }),
        ))
        .is_ok());
    }

    #[test]
    fn test_marketplace_new_rejects_http_registry_config() {
        let repo = RepositoryDefinition {
            name: "x".to_string(),
            repo_type: RepositoryType::GitMarketplace,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "http://example.com/index".to_string(),
            },
            auth: None,
            storage: None,
        };
        assert!(MarketplaceRepositoryClient::new(&repo).is_err());
    }

    #[tokio::test]
    async fn test_marketplace_download_not_implemented() {
        let tmp = tempfile::tempdir().unwrap();
        let client = MarketplaceRepositoryClient::new(&marketplace(
            RepositoryConfig::Local {
                path: tmp.path().to_path_buf(),
            },
            None,
        ))
        .unwrap();
        assert!(matches!(
            client.download("a", "1.0.0").await,
            Err(RepositoryClientError::NotImplemented)
        ));
    }

    #[tokio::test]
    async fn test_marketplace_get_skill_invalid_id() {
        let tmp = tempfile::tempdir().unwrap();
        let client = MarketplaceRepositoryClient::new(&marketplace(
            RepositoryConfig::Local {
                path: tmp.path().to_path_buf(),
            },
            None,
        ))
        .unwrap();
        // Empty id fails SkillId validation before any I/O.
        assert!(client.get_skill("", None).await.is_err());
        assert!(client.get_versions("").await.is_err());
    }

    #[tokio::test]
    async fn test_marketplace_local_list_get_search_versions() {
        // A Local source scans a directory tree for SKILL.md files.
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = tmp.path().join("my-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\nversion: \"1.0.0\"\ndescription: A local marketplace skill\n---\n",
        )
        .unwrap();

        let client = MarketplaceRepositoryClient::new(&marketplace(
            RepositoryConfig::Local {
                path: tmp.path().to_path_buf(),
            },
            None,
        ))
        .unwrap();

        let skills = client.list_skills().await.unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id.to_string(), "my-skill");

        let got = client.get_skill("my-skill", None).await.unwrap();
        assert!(got.is_some());
        // A non-matching version filters it out.
        assert!(client
            .get_skill("my-skill", Some("9.9.9"))
            .await
            .unwrap()
            .is_none());

        let found = client.search("local").await.unwrap();
        assert_eq!(found.len(), 1);
        let none = client.search("zzzz-nomatch").await.unwrap();
        assert!(none.is_empty());

        let versions = client.get_versions("my-skill").await.unwrap();
        assert_eq!(versions, vec!["1.0.0"]);
    }

    // ── CratesRegistryClient::new ─────────────────────────────────────────────

    #[test]
    fn test_crates_new_success_and_auth_variants() {
        for auth in [
            None,
            Some(RepositoryAuth::Pat {
                env_var: "T".to_string(),
            }),
            Some(RepositoryAuth::Ssh {
                key_path: "/tmp/k".into(),
            }),
            Some(RepositoryAuth::ApiKey {
                env_var: "K".to_string(),
            }),
            // Basic is unsupported here → falls back to a PAT default.
            Some(RepositoryAuth::Basic {
                username: "u".to_string(),
                password_env: "P".to_string(),
            }),
        ] {
            assert!(
                CratesRegistryClient::new(&http_registry("http://example.com/index", auth)).is_ok()
            );
        }
    }

    #[test]
    fn test_crates_new_empty_index_url() {
        assert!(CratesRegistryClient::new(&http_registry("   ", None)).is_err());
    }

    #[test]
    fn test_crates_new_invalid_index_url() {
        assert!(CratesRegistryClient::new(&http_registry("not a url", None)).is_err());
    }

    #[test]
    fn test_crates_new_requires_http_registry_config() {
        let repo = marketplace(
            RepositoryConfig::ZipUrl {
                base_url: "https://example.com".to_string(),
            },
            None,
        );
        assert!(CratesRegistryClient::new(&repo).is_err());
    }

    // ── CratesRegistryClient::fetch_skills ────────────────────────────────────

    const SKILLS_PATH: &str = "/api/v1/registry/index/skills";

    fn summary(id: &str) -> SkillSummary {
        SkillSummary {
            id: id.to_string(),
            scope: "scope".to_string(),
            name: id.to_string(),
            description: "a skill".to_string(),
            latest_version: "1.0.0".to_string(),
            published_at: None,
            versions: None,
        }
    }

    #[tokio::test]
    async fn test_fetch_skills_success_and_list_skills() {
        let server = MockServer::start().await;
        let body = serde_json::to_string(&vec![summary("alpha"), summary("beta")]).unwrap();
        Mock::given(method("GET"))
            .and(path(SKILLS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = CratesRegistryClient::new(&http_registry(&server.uri(), None)).unwrap();
        let summaries = client
            .fetch_skills(&ListSkillsOptions::default())
            .await
            .unwrap();
        assert_eq!(summaries.len(), 2);

        // list_skills converts summaries to SkillMetadata.
        let metadata = client.list_skills().await.unwrap();
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].version, "1.0.0");
    }

    #[tokio::test]
    async fn test_fetch_skills_with_query_params() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(SKILLS_PATH))
            .and(query_param("scope", "myscope"))
            .and(query_param("all_versions", "true"))
            .and(query_param("include_pre_release", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;

        let client = CratesRegistryClient::new(&http_registry(&server.uri(), None)).unwrap();
        let options = ListSkillsOptions {
            scope: Some("myscope".to_string()),
            all_versions: true,
            include_pre_release: true,
        };
        let result = client.fetch_skills(&options).await.unwrap();
        assert!(result.is_empty());
    }

    async fn assert_status_maps_to_err(status: u16) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(SKILLS_PATH))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let client = CratesRegistryClient::new(&http_registry(&server.uri(), None)).unwrap();
        assert!(
            client
                .fetch_skills(&ListSkillsOptions::default())
                .await
                .is_err(),
            "status {} should map to an error",
            status
        );
    }

    #[tokio::test]
    async fn test_fetch_skills_error_statuses() {
        for status in [400u16, 401, 403, 404, 500, 503, 418] {
            assert_status_maps_to_err(status).await;
        }
    }

    // ── CratesRegistryClient delegation (get_skill/get_versions/download) ──────

    #[tokio::test]
    async fn test_crates_get_skill_and_versions_and_download() {
        use crate::core::registry::client::IndexEntry;
        use sha2::Digest;

        let server = MockServer::start().await;
        let payload = b"pkg-bytes";
        let cksum = format!("sha256:{:x}", sha2::Sha256::digest(payload));
        let dl_url = format!("{}/dl", server.uri());

        let entry = IndexEntry {
            name: "myskill".to_string(),
            vers: "1.0.0".to_string(),
            deps: Vec::new(),
            cksum,
            features: std::collections::HashMap::new(),
            yanked: false,
            links: None,
            download_url: dl_url,
            metadata: None,
        };
        let body = serde_json::to_string(&entry).unwrap();

        Mock::given(method("GET"))
            .and(path("/myskill"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/dl"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&server)
            .await;

        let client = CratesRegistryClient::new(&http_registry(&server.uri(), None)).unwrap();

        let skill = client.get_skill("myskill", Some("1.0.0")).await.unwrap();
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().version, "1.0.0");

        // Latest-version path (version = None).
        assert!(client.get_skill("myskill", None).await.unwrap().is_some());

        let versions = client.get_versions("myskill").await.unwrap();
        assert_eq!(versions, vec!["1.0.0"]);

        let bytes = client.download("myskill", "1.0.0").await.unwrap();
        assert_eq!(bytes, payload);
    }

    #[tokio::test]
    async fn test_crates_get_skill_not_found_is_none() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/nope"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let client = CratesRegistryClient::new(&http_registry(&server.uri(), None)).unwrap();
        assert!(client.get_skill("nope", None).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_crates_search_delegates_empty() {
        // RegistryClient::search currently returns empty without HTTP.
        let client =
            CratesRegistryClient::new(&http_registry("http://example.com/index", None)).unwrap();
        assert!(client.search("q").await.unwrap().is_empty());
    }
}

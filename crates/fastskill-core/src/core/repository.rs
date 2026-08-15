//! Unified repository system for managing skill storage locations
//!
//! This module provides a unified repositories.toml configuration for all repository types.

pub mod client;

pub use client::{CratesRegistryClient, RepositoryClient, RepositoryClientError};

use crate::core::service::ServiceError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main repositories configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoriesConfig {
    #[serde(default)]
    pub repositories: Vec<RepositoryDefinition>,
}

/// Repository definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub repo_type: RepositoryType,
    #[serde(default = "default_priority")]
    pub priority: u32,
    #[serde(flatten)]
    pub config: RepositoryConfig,
    #[serde(default)]
    pub auth: Option<RepositoryAuth>,
    #[serde(default)]
    pub storage: Option<StorageConfig>,
}

/// Default priority value (0 = highest priority)
fn default_priority() -> u32 {
    0
}

/// Repository type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RepositoryType {
    /// Git repository with marketplace.json
    GitMarketplace,
    /// HTTP-based registry with flat index layout
    HttpRegistry,
    /// ZIP URL base with marketplace.json
    ZipUrl,
    /// Local directory
    Local,
}

/// Unified repository configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RepositoryConfig {
    /// Git marketplace configuration
    GitMarketplace {
        url: String,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        tag: Option<String>,
    },
    /// HTTP registry configuration
    HttpRegistry { index_url: String },
    /// ZIP URL configuration
    ZipUrl { base_url: String },
    /// Local path configuration
    Local { path: PathBuf },
}

/// Unified authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RepositoryAuth {
    #[serde(rename = "pat")]
    Pat { env_var: String },
    #[serde(rename = "ssh-key")]
    SshKey { path: PathBuf },
    #[serde(rename = "ssh")]
    Ssh { key_path: PathBuf },
    #[serde(rename = "basic")]
    Basic {
        username: String,
        password_env: String,
    },
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

/// Repository manager for handling multiple repositories
pub struct RepositoryManager {
    config_path: PathBuf,
    repositories: HashMap<String, RepositoryDefinition>,
    clients: Arc<RwLock<HashMap<String, Arc<dyn RepositoryClient + Send + Sync>>>>,
}

impl RepositoryManager {
    /// Create a new repository manager
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            repositories: HashMap::new(),
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a repository manager from a list of repository definitions
    /// Used when loading from skill-project.toml instead of repositories.toml
    pub fn from_definitions(definitions: Vec<RepositoryDefinition>) -> Self {
        let mut repo_map: HashMap<String, RepositoryDefinition> = HashMap::new();
        let mut sorted_repos = definitions;
        sorted_repos.sort_by_key(|r| r.priority);

        for repo in sorted_repos {
            repo_map.entry(repo.name.clone()).or_insert(repo);
        }

        // Determine config path: try to use skill-project.toml, otherwise use empty path
        let config_path = std::env::current_dir()
            .ok()
            .and_then(|dir| {
                let project_file = crate::core::project::resolve_project_file(&dir);
                if project_file.found {
                    Some(project_file.path)
                } else {
                    None
                }
            })
            .unwrap_or_default();

        Self {
            config_path,
            repositories: repo_map,
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load repositories from TOML file
    /// Loads from repositories.toml only
    pub fn load(&mut self) -> Result<(), ServiceError> {
        if self.config_path.exists() {
            // Load from unified repositories.toml
            let content = std::fs::read_to_string(&self.config_path).map_err(ServiceError::Io)?;

            let config: RepositoriesConfig = toml::from_str(&content).map_err(|e| {
                ServiceError::Custom(format!("Failed to parse repositories config: {}", e))
            })?;

            // T068: Repository priority conflict resolution (first occurrence wins)
            // Sort repositories by priority (lower number = higher priority)
            let mut sorted_repos: Vec<RepositoryDefinition> = config.repositories;
            sorted_repos.sort_by_key(|r| r.priority);

            // First occurrence wins - only insert if name doesn't already exist
            let mut repo_map: HashMap<String, RepositoryDefinition> = HashMap::new();
            for repo in sorted_repos {
                repo_map.entry(repo.name.clone()).or_insert(repo);
            }
            self.repositories = repo_map;

            return Ok(());
        }

        // Create default empty config if no configs exist
        let config = RepositoriesConfig {
            repositories: Vec::new(),
        };
        self.save_config(&config)?;
        self.repositories = HashMap::new();
        Ok(())
    }

    /// Save repositories to TOML file
    pub fn save(&self) -> Result<(), ServiceError> {
        // Check if config_path is a skill-project.toml file
        if self.config_path.file_name().and_then(|n| n.to_str()) == Some("skill-project.toml") {
            self.save_to_project_file()
        } else {
            // Old repositories.toml format
            let mut repos: Vec<RepositoryDefinition> =
                self.repositories.values().cloned().collect();
            repos.sort_by_key(|r| r.priority);
            let config = RepositoriesConfig {
                repositories: repos,
            };
            self.save_config(&config)
        }
    }

    /// Save repositories to skill-project.toml
    fn save_to_project_file(&self) -> Result<(), ServiceError> {
        use crate::core::manifest::SkillProjectToml;

        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(ServiceError::Io)?;
        }

        // Load existing project file or create new one
        let mut project = if self.config_path.exists() {
            SkillProjectToml::load_from_file(&self.config_path).map_err(|e| {
                ServiceError::Custom(format!("Failed to load skill-project.toml: {}", e))
            })?
        } else {
            // Create minimal project file
            SkillProjectToml {
                metadata: None,
                dependencies: None,
                tool: None,
            }
        };

        // Convert RepositoryDefinition back to manifest format
        let manifest_repos: Vec<crate::core::manifest::RepositoryDefinition> = self
            .repositories
            .values()
            .map(|repo| self.convert_to_manifest_repo(repo))
            .collect();

        // Update tool.fastskill.repositories
        if project.tool.is_none() {
            project.tool = Some(crate::core::manifest::ToolSection {
                fastskill: Some(crate::core::manifest::FastSkillToolConfig {
                    skills_directory: None,
                    embedding: None,
                    repositories: Some(manifest_repos),
                    server: None,
                    install_depth: 5,
                    skip_transitive: false,
                    eval: None,
                    auto_reindex: true,
                }),
            });
        } else if let Some(ref mut tool) = project.tool {
            if tool.fastskill.is_none() {
                tool.fastskill = Some(crate::core::manifest::FastSkillToolConfig {
                    skills_directory: None,
                    embedding: None,
                    repositories: Some(manifest_repos),
                    server: None,
                    install_depth: 5,
                    skip_transitive: false,
                    eval: None,
                    auto_reindex: true,
                });
            } else if let Some(ref mut fastskill) = tool.fastskill {
                fastskill.repositories = Some(manifest_repos);
            }
        }

        // Save the project file
        project.save_to_file(&self.config_path).map_err(|e| {
            ServiceError::Custom(format!("Failed to save skill-project.toml: {}", e))
        })?;

        Ok(())
    }

    /// Convert RepositoryDefinition to manifest format
    fn convert_to_manifest_repo(
        &self,
        repo: &RepositoryDefinition,
    ) -> crate::core::manifest::RepositoryDefinition {
        use crate::core::manifest::{
            AuthConfig, AuthType, RepositoryConnection, RepositoryType as ManifestType,
        };

        let repo_type = match repo.repo_type {
            RepositoryType::HttpRegistry => ManifestType::HttpRegistry,
            RepositoryType::GitMarketplace => ManifestType::GitMarketplace,
            RepositoryType::ZipUrl => ManifestType::ZipUrl,
            RepositoryType::Local => ManifestType::Local,
        };

        let connection = match &repo.config {
            RepositoryConfig::HttpRegistry { index_url } => RepositoryConnection::HttpRegistry {
                index_url: index_url.clone(),
            },
            RepositoryConfig::GitMarketplace {
                url,
                branch,
                tag: _,
            } => RepositoryConnection::GitMarketplace {
                url: url.clone(),
                branch: branch.clone(),
            },
            RepositoryConfig::ZipUrl { base_url } => RepositoryConnection::ZipUrl {
                zip_url: base_url.clone(),
            },
            RepositoryConfig::Local { path } => RepositoryConnection::Local {
                path: path.to_string_lossy().to_string(),
            },
        };

        // Convert auth - manifest format only supports PAT currently
        let auth = repo.auth.as_ref().and_then(|a| match a {
            RepositoryAuth::Pat { env_var } => Some(AuthConfig {
                r#type: AuthType::Pat,
                env_var: Some(env_var.clone()),
            }),
            // Other auth types not supported in manifest format yet
            _ => None,
        });

        crate::core::manifest::RepositoryDefinition {
            name: repo.name.clone(),
            r#type: repo_type,
            priority: repo.priority,
            connection,
            auth,
        }
    }

    /// Internal helper to save config (for old repositories.toml format)
    fn save_config(&self, config: &RepositoriesConfig) -> Result<(), ServiceError> {
        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(ServiceError::Io)?;
        }

        let content = toml::to_string_pretty(config).map_err(|e| {
            ServiceError::Custom(format!("Failed to serialize repositories config: {}", e))
        })?;

        crate::utils::atomic_write(&self.config_path, content.as_bytes())
            .map_err(ServiceError::Io)?;

        Ok(())
    }

    /// Add a new repository
    pub fn add_repository(
        &mut self,
        name: String,
        definition: RepositoryDefinition,
    ) -> Result<(), ServiceError> {
        if self.repositories.contains_key(&name) {
            return Err(ServiceError::Custom(format!(
                "Repository '{}' already exists",
                name
            )));
        }

        self.repositories.insert(name, definition);
        Ok(())
    }

    /// Remove a repository
    pub fn remove_repository(&mut self, name: &str) -> Result<(), ServiceError> {
        if self.repositories.remove(name).is_none() {
            return Err(ServiceError::Custom(format!(
                "Repository '{}' not found",
                name
            )));
        }
        // Also remove client if it exists
        if let Ok(mut clients) = self.clients.try_write() {
            clients.remove(name);
        }
        Ok(())
    }

    /// Get a repository by name
    pub fn get_repository(&self, name: &str) -> Option<&RepositoryDefinition> {
        self.repositories.get(name)
    }

    /// List all repositories (sorted by priority)
    pub fn list_repositories(&self) -> Vec<&RepositoryDefinition> {
        let mut repos: Vec<&RepositoryDefinition> = self.repositories.values().collect();
        repos.sort_by_key(|r| r.priority);
        repos
    }

    /// Get or create a repository client
    pub async fn get_client(
        &self,
        name: &str,
    ) -> Result<Arc<dyn RepositoryClient + Send + Sync>, ServiceError> {
        // Check cache first
        {
            let clients = self.clients.read().await;
            if let Some(client) = clients.get(name) {
                return Ok(Arc::clone(client));
            }
        }

        // Create new client
        let repo = self
            .repositories
            .get(name)
            .ok_or_else(|| ServiceError::Custom(format!("Repository '{}' not found", name)))?;

        let client_arc = client::create_client(repo).await?;

        // Cache it
        let mut clients = self.clients.write().await;
        clients.insert(name.to_string(), client_arc.clone());
        Ok(client_arc)
    }

    /// Get default repository (first by priority, or one named "default")
    pub fn get_default_repository(&self) -> Option<&RepositoryDefinition> {
        // First try to find one named "default"
        if let Some(repo) = self.repositories.get("default") {
            return Some(repo);
        }

        // Otherwise return first by priority
        let mut repos: Vec<&RepositoryDefinition> = self.repositories.values().collect();
        repos.sort_by_key(|r| r.priority);
        repos.first().copied()
    }

    /// Refresh the on-disk index cache for a single repository (PRD 006 /
    /// RFQ 004, US-005). Two independent steps:
    ///
    /// 1. For a `GitMarketplace` repository, resolve its configured branch/tag
    ///    to the remote's current commit SHA via [`crate::storage::git::ls_remote`]
    ///    and record it in the git-resolutions index — the same cache
    ///    `install::fetch_git` reads for offline resolution (US-002). An
    ///    unreachable remote fails the refresh here, before step 2.
    /// 2. Fetch the repository's current skill listing via [`Self::get_client`]
    ///    and persist it as a [`crate::core::cache::SourceIndex`].
    ///
    /// A step-2 listing failure (e.g. no reachable marketplace.json) does not
    /// unwind a step-1 resolution already recorded — the two are independently
    /// useful and the ref-resolution index should stay warm even if the
    /// listing itself is temporarily broken.
    ///
    /// Index only: this never fetches or caches skill *content* (RFQ 004's
    /// "does not pre-warm content" default).
    ///
    /// Returns the number of distinct skills recorded in the refreshed index.
    pub async fn refresh_index(
        &self,
        cache: &crate::core::cache::SkillCache,
        name: &str,
    ) -> Result<usize, ServiceError> {
        let repo = self
            .get_repository(name)
            .ok_or_else(|| ServiceError::Custom(format!("Repository '{}' not found", name)))?;

        // Ref resolution first and independent of the listing below: a
        // `GitMarketplace` repository's branch/tag resolves via `ls_remote`
        // against the git remote directly, not via whatever serves
        // marketplace.json, so recording it does not depend on the listing
        // step succeeding. An unreachable remote fails the whole refresh
        // fast, before attempting a listing that would fail anyway.
        if let RepositoryConfig::GitMarketplace { url, branch, tag } = &repo.config {
            let sha = crate::storage::git::ls_remote(url, branch.as_deref(), tag.as_deref())
                .await
                .map_err(|e| {
                    ServiceError::Custom(format!(
                        "failed to resolve current ref for '{}': {}",
                        name, e
                    ))
                })?;
            let ref_key = crate::core::cache::GitResolutions::branch_or_tag_key(
                branch.as_deref(),
                tag.as_deref(),
            );
            let mut resolutions = cache.read_git_resolutions()?;
            resolutions.insert(url, &ref_key, sha, chrono::Utc::now());
            cache.write_git_resolutions(&resolutions)?;
        }

        let client = self.get_client(name).await.map_err(|e| {
            ServiceError::Custom(format!("failed to connect to repository '{}': {}", name, e))
        })?;
        let skills = client.list_skills().await.map_err(|e| {
            ServiceError::Custom(format!("failed to list skills for '{}': {}", name, e))
        })?;

        // Group by skill id, deduping versions — `list_skills()` currently
        // advertises one (the current/latest) version per skill for every
        // repository type, but this stays correct if that ever changes.
        let mut by_id: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for skill in &skills {
            by_id
                .entry(skill.id.to_string())
                .or_default()
                .insert(skill.version.clone());
        }
        let entries: Vec<crate::core::cache::SourceIndexEntry> = by_id
            .into_iter()
            .map(|(skill, versions)| crate::core::cache::SourceIndexEntry {
                skill,
                versions: versions.into_iter().collect(),
            })
            .collect();
        let entry_count = entries.len();

        let idx = crate::core::cache::SourceIndex {
            fetched_at: chrono::Utc::now(),
            entries,
        };
        cache.write_source_index(name, &idx)?;

        Ok(entry_count)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod refresh_index_tests {
    //! `RepositoryManager::refresh_index` (PRD 006 / RFQ 004, US-005). Uses
    //! `Local` repositories exclusively — no network involved — so git-source
    //! ref resolution (which needs a real `ls_remote`) is covered separately
    //! by `tests/repo_index_refresh_git_test.rs` against a local git daemon.

    use super::*;
    use crate::core::cache::SkillCache;
    use std::path::Path;
    use tempfile::TempDir;

    fn local_repo(name: &str, path: PathBuf) -> RepositoryDefinition {
        RepositoryDefinition {
            name: name.to_string(),
            repo_type: RepositoryType::Local,
            priority: 0,
            config: RepositoryConfig::Local { path },
            auth: None,
            storage: None,
        }
    }

    fn write_skill(dir: &Path, id: &str, version: &str) {
        let skill_dir = dir.join(id);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {id}\nversion: \"{version}\"\ndescription: a skill\n---\nBody\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn refresh_index_writes_source_index_and_returns_skill_count() {
        let source_dir = TempDir::new().unwrap();
        write_skill(source_dir.path(), "alpha", "1.0.0");
        write_skill(source_dir.path(), "beta", "2.0.0");

        let mut manager = RepositoryManager::new(PathBuf::new());
        manager
            .add_repository(
                "local-src".to_string(),
                local_repo("local-src", source_dir.path().to_path_buf()),
            )
            .unwrap();

        let cache_root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(cache_root.path());

        let count = manager.refresh_index(&cache, "local-src").await.unwrap();
        assert_eq!(count, 2);

        let idx = cache
            .read_source_index("local-src")
            .unwrap()
            .expect("index file must be written");
        let mut skills: Vec<&str> = idx.entries.iter().map(|e| e.skill.as_str()).collect();
        skills.sort();
        assert_eq!(skills, vec!["alpha", "beta"]);
        let alpha = idx.entries.iter().find(|e| e.skill == "alpha").unwrap();
        assert_eq!(alpha.versions, vec!["1.0.0".to_string()]);
    }

    #[tokio::test]
    async fn refresh_index_unknown_repository_is_an_error() {
        let manager = RepositoryManager::new(PathBuf::new());
        let cache_root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(cache_root.path());

        let err = manager.refresh_index(&cache, "nope").await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn refresh_index_propagates_a_listing_failure_without_writing_an_index() {
        let mut manager = RepositoryManager::new(PathBuf::new());
        manager
            .add_repository(
                "missing-path".to_string(),
                local_repo(
                    "missing-path",
                    std::env::temp_dir().join("fastskill-does-not-exist-72f1"),
                ),
            )
            .unwrap();

        let cache_root = TempDir::new().unwrap();
        let cache = SkillCache::at_root(cache_root.path());

        let result = manager.refresh_index(&cache, "missing-path").await;
        assert!(result.is_err());
        assert!(cache.read_source_index("missing-path").unwrap().is_none());
    }
}

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
///
/// `Pat` is the only variant: it is the only auth method the on-disk manifest
/// format (`manifest::AuthType`) can represent, so it is the only one that
/// ever survives a save/load round-trip. See `convert_to_manifest_repo` below.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RepositoryAuth {
    #[serde(rename = "pat")]
    Pat { env_var: String },
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
        use crate::core::manifest::{SkillProjectToml, MANIFEST_SCHEMA_VERSION};

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
                schema_version: Some(MANIFEST_SCHEMA_VERSION.to_string()),
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
            RepositoryConfig::GitMarketplace { url, branch, tag } => {
                RepositoryConnection::GitMarketplace {
                    url: url.clone(),
                    branch: branch.clone(),
                    tag: tag.clone(),
                }
            }
            RepositoryConfig::ZipUrl { base_url } => RepositoryConnection::ZipUrl {
                zip_url: base_url.clone(),
            },
            RepositoryConfig::Local { path } => RepositoryConnection::Local {
                path: path.to_string_lossy().to_string(),
            },
        };

        // RepositoryAuth has exactly one variant (`Pat`), so this conversion
        // is total.
        let auth = repo.auth.as_ref().map(|a| {
            let RepositoryAuth::Pat { env_var } = a;
            AuthConfig {
                r#type: AuthType::Pat,
                env_var: Some(env_var.clone()),
            }
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
        //
        // `name`/`description` (spec 008) are recorded as a representative
        // snapshot for the id — the first `SkillMetadata` seen for it — the
        // same "one per id, not one per version" simplification `versions`
        // already applies; `list_skills()`'s current one-version-per-skill
        // behavior means this loses nothing in practice today.
        let mut by_id: std::collections::BTreeMap<
            String,
            (std::collections::BTreeSet<String>, String, String),
        > = std::collections::BTreeMap::new();
        for skill in &skills {
            let entry = by_id.entry(skill.id.to_string()).or_insert_with(|| {
                (
                    std::collections::BTreeSet::new(),
                    skill.name.clone(),
                    skill.description.clone(),
                )
            });
            entry.0.insert(skill.version.clone());
        }
        let entries: Vec<crate::core::cache::SourceIndexEntry> = by_id
            .into_iter()
            .map(
                |(skill, (versions, name, description))| crate::core::cache::SourceIndexEntry {
                    skill,
                    versions: versions.into_iter().collect(),
                    name,
                    description,
                },
            )
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

    fn repository(
        name: &str,
        priority: u32,
        repo_type: RepositoryType,
        config: RepositoryConfig,
    ) -> RepositoryDefinition {
        RepositoryDefinition {
            name: name.to_string(),
            repo_type,
            priority,
            config,
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

    #[test]
    fn project_manifest_round_trip_preserves_git_tag() {
        let project = TempDir::new().unwrap();
        let manifest_path = project.path().join("skill-project.toml");
        let mut manager = RepositoryManager::new(manifest_path.clone());
        manager
            .add_repository(
                "tagged".to_string(),
                RepositoryDefinition {
                    name: "tagged".to_string(),
                    repo_type: RepositoryType::GitMarketplace,
                    priority: 0,
                    config: RepositoryConfig::GitMarketplace {
                        url: "https://github.com/example/skills.git".to_string(),
                        branch: None,
                        tag: Some("v1.2.0".to_string()),
                    },
                    auth: None,
                    storage: None,
                },
            )
            .unwrap();

        manager.save().unwrap();

        let manifest =
            crate::core::manifest::SkillProjectToml::load_from_file(&manifest_path).unwrap();
        let repositories = manifest
            .tool
            .unwrap()
            .fastskill
            .unwrap()
            .repositories
            .unwrap();
        assert!(matches!(
            repositories[0].connection,
            crate::core::manifest::RepositoryConnection::GitMarketplace {
                branch: None,
                tag: Some(ref tag),
                ..
            } if tag == "v1.2.0"
        ));
        let runtime = RepositoryDefinition::from(&repositories[0]);
        assert!(matches!(
            runtime.config,
            RepositoryConfig::GitMarketplace {
                branch: None,
                tag: Some(ref tag),
                ..
            } if tag == "v1.2.0"
        ));
    }

    #[tokio::test]
    async fn legacy_config_lifecycle_preserves_priority_and_client_cache() {
        let root = TempDir::new().unwrap();
        let config_path = root.path().join("config/repositories.toml");
        let source = root.path().join("skills");
        std::fs::create_dir_all(&source).unwrap();
        let mut manager = RepositoryManager::new(config_path.clone());

        manager.load().unwrap();
        assert!(config_path.is_file());
        assert!(manager.list_repositories().is_empty());

        std::fs::write(
            &config_path,
            format!(
                "[[repositories]]\nname = \"duplicate\"\ntype = \"local\"\npath = {:?}\n\
                 [[repositories]]\nname = \"duplicate\"\ntype = \"local\"\npriority = 9\npath = {:?}\n",
                source,
                root.path().join("ignored")
            ),
        )
        .unwrap();
        manager.load().unwrap();
        assert_eq!(manager.list_repositories().len(), 1);
        assert_eq!(manager.get_repository("duplicate").unwrap().priority, 0);

        let default = local_repo("default", source.clone());
        manager
            .add_repository("default".to_string(), default)
            .unwrap();
        assert!(manager
            .add_repository("default".to_string(), local_repo("default", source.clone()))
            .unwrap_err()
            .to_string()
            .contains("already exists"));
        assert_eq!(manager.get_default_repository().unwrap().name, "default");
        manager.save().unwrap();

        let first = manager.get_client("default").await.unwrap();
        let second = manager.get_client("default").await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        manager.remove_repository("default").unwrap();
        assert!(manager.remove_repository("default").is_err());
        assert!(manager.get_client("default").await.is_err());

        let mut reloaded = RepositoryManager::new(config_path);
        reloaded.load().unwrap();
        assert_eq!(reloaded.get_repository("default").unwrap().name, "default");
        assert!(RepositoryManager::new(root.path().join("bad.toml"))
            .load()
            .is_ok());
        std::fs::write(root.path().join("bad.toml"), "not valid toml = [").unwrap();
        assert!(RepositoryManager::new(root.path().join("bad.toml"))
            .load()
            .unwrap_err()
            .to_string()
            .contains("Failed to parse"));
    }

    #[test]
    fn project_save_converts_every_connection_and_updates_existing_tool_shapes() {
        let root = TempDir::new().unwrap();
        let manifest_path = root.path().join("skill-project.toml");
        let definitions = vec![
            repository(
                "http",
                1,
                RepositoryType::HttpRegistry,
                RepositoryConfig::HttpRegistry {
                    index_url: "https://example.test/index".to_string(),
                },
            ),
            repository(
                "git",
                2,
                RepositoryType::GitMarketplace,
                RepositoryConfig::GitMarketplace {
                    url: "https://example.test/repo.git".to_string(),
                    branch: None,
                    tag: Some("v2".to_string()),
                },
            ),
            repository(
                "zip",
                3,
                RepositoryType::ZipUrl,
                RepositoryConfig::ZipUrl {
                    base_url: "https://example.test/catalog.zip".to_string(),
                },
            ),
            RepositoryDefinition {
                auth: Some(RepositoryAuth::Pat {
                    env_var: "PRIVATE_TOKEN".to_string(),
                }),
                ..repository(
                    "local",
                    4,
                    RepositoryType::Local,
                    RepositoryConfig::Local {
                        path: root.path().join("catalog"),
                    },
                )
            },
        ];
        let mut manager = RepositoryManager::new(manifest_path.clone());
        for definition in definitions {
            manager
                .add_repository(definition.name.clone(), definition)
                .unwrap();
        }

        manager.save().unwrap();
        let read = || {
            crate::core::manifest::SkillProjectToml::load_from_file(&manifest_path)
                .unwrap()
                .tool
                .unwrap()
                .fastskill
                .unwrap()
                .repositories
                .unwrap()
        };
        let repositories = read();
        assert_eq!(repositories.len(), 4);
        let git = repositories.iter().find(|repo| repo.name == "git").unwrap();
        assert!(matches!(
            &git.connection,
            crate::core::manifest::RepositoryConnection::GitMarketplace {
                branch: None,
                tag: Some(tag),
                ..
            } if tag == "v2"
        ));
        let local = repositories
            .iter()
            .find(|repo| repo.name == "local")
            .unwrap();
        assert!(local.auth.is_some());

        std::fs::write(&manifest_path, "[tool]\n").unwrap();
        manager.save().unwrap();
        assert_eq!(read().len(), 4);
        std::fs::write(
            &manifest_path,
            "[tool.fastskill]\nskills_directory = \"skills\"\n",
        )
        .unwrap();
        manager.save().unwrap();
        assert_eq!(read().len(), 4);

        std::fs::write(&manifest_path, "invalid = [").unwrap();
        assert!(manager
            .save()
            .unwrap_err()
            .to_string()
            .contains("Failed to load skill-project.toml"));
    }

    #[test]
    fn definition_loading_deduplicates_names_and_uses_priority_fallback() {
        let definitions = vec![
            repository(
                "low",
                8,
                RepositoryType::Local,
                RepositoryConfig::Local { path: "low".into() },
            ),
            repository(
                "preferred",
                1,
                RepositoryType::Local,
                RepositoryConfig::Local {
                    path: "preferred".into(),
                },
            ),
            repository(
                "preferred",
                9,
                RepositoryType::Local,
                RepositoryConfig::Local {
                    path: "ignored".into(),
                },
            ),
        ];
        let manager = RepositoryManager::from_definitions(definitions);
        assert_eq!(manager.list_repositories()[0].name, "preferred");
        assert_eq!(manager.get_default_repository().unwrap().name, "preferred");
        assert_eq!(manager.get_repository("preferred").unwrap().priority, 1);
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
        // spec 008: name/description are captured from the listing too, not
        // just id + versions.
        assert_eq!(alpha.name, "alpha");
        assert_eq!(alpha.description, "a skill");
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

//! Main FastSkill service implementation

use crate::execution::ExecutionConfig;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// HTTP server CORS configuration
#[derive(Debug, Clone, Default)]
pub struct HttpServerConfig {
    /// List of origins allowed for CORS (required when server is used)
    pub allowed_origins: Vec<String>,

    /// Optional: allow list of request headers
    /// Default: ["Content-Type", "Authorization"] if unset
    pub allowed_headers: Vec<String>,
}

/// Main service configuration
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Base directory for skill storage
    pub skill_storage_path: PathBuf,

    /// Execution configuration
    pub execution: ExecutionConfig,

    /// Hot reloading configuration
    pub hot_reload: HotReloadConfig,

    /// Cache configuration
    pub cache: CacheConfig,

    /// Embedding configuration
    pub embedding: Option<EmbeddingConfig>,

    /// Security configuration
    pub security: SecurityConfig,

    /// Registry index path
    pub registry_index_path: Option<PathBuf>,

    /// HTTP server configuration
    pub http_server: Option<HttpServerConfig>,

    /// Override for the on-disk skill content/index cache root (PRD 006 /
    /// RFQ 004). `None` resolves it via [`crate::core::cache::SkillCache::from_env`]
    /// (the `FASTSKILL_CACHE_DIR` env var, else the platform cache dir) — what
    /// production call sites want. Tests should always set this to a
    /// `tempfile::TempDir` path rather than rely on the env-var default: that
    /// default is process-global, so asserting on it races against any other
    /// test in the same process that also touches the cache.
    pub skill_cache_root: Option<PathBuf>,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            skill_storage_path: PathBuf::from("./skills"),
            execution: ExecutionConfig::default(),
            hot_reload: HotReloadConfig::default(),
            cache: CacheConfig::default(),
            embedding: None,
            security: SecurityConfig::default(),
            registry_index_path: None,
            http_server: None,
            skill_cache_root: None,
        }
    }
}

/// Hot reloading configuration
#[derive(Debug, Clone)]
pub struct HotReloadConfig {
    /// Enable hot reloading
    pub enabled: bool,

    /// Directories to watch for changes
    pub watch_paths: Vec<PathBuf>,

    /// Debounce duration for file changes (ms)
    pub debounce_ms: u64,

    /// Automatically reload on file changes
    pub auto_reload: bool,
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            watch_paths: vec![PathBuf::from("./skills")],
            debounce_ms: 1000,
            auto_reload: true,
        }
    }
}

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum cache size (number of skills)
    pub max_size: usize,

    /// Cache TTL for metadata (seconds)
    pub metadata_ttl: u64,

    /// Cache TTL for content (seconds)
    pub content_ttl: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size: 1000,
            metadata_ttl: 300, // 5 minutes
            content_ttl: 60,   // 1 minute
        }
    }
}

/// Embedding configuration
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// OpenAI API base URL
    pub openai_base_url: String,

    /// Embedding model name
    pub embedding_model: String,

    /// Custom path for vector index database
    pub index_path: Option<PathBuf>,
}

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Enable security sandboxing
    pub enable_sandbox: bool,

    /// Allowed file system paths for scripts
    pub allowed_paths: Vec<PathBuf>,

    /// Audit logging configuration
    pub audit_logging: bool,

    /// Maximum script execution time
    pub max_execution_time: std::time::Duration,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enable_sandbox: true,
            allowed_paths: vec![PathBuf::from("/tmp"), PathBuf::from("./temp")],
            audit_logging: true,
            max_execution_time: std::time::Duration::from_secs(60),
        }
    }
}

/// Unique identifier for a skill
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SkillId(String);

impl SkillId {
    /// Create a new SkillId with validation
    pub fn new(id: String) -> Result<Self, ServiceError> {
        if id.trim().is_empty() {
            return Err(ServiceError::Validation(
                "Skill ID cannot be empty".to_string(),
            ));
        }
        if id.len() > 255 {
            return Err(ServiceError::Validation(
                "Skill ID too long (max 255 characters)".to_string(),
            ));
        }
        // Reject forward slashes (scope should be handled separately)
        if id.contains('/') {
            return Err(ServiceError::Validation(
                "Skill ID cannot contain forward slashes. Scope should be handled separately during publishing.".to_string(),
            ));
        }
        // Basic validation for allowed characters (alphanumeric, dash, underscore)
        if !id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ServiceError::Validation("Skill ID contains invalid characters (only alphanumeric, dash, underscore allowed)".to_string()));
        }
        Ok(Self(id))
    }

    /// Get the string value
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to owned string
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for SkillId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<SkillId> for String {
    fn from(id: SkillId) -> String {
        id.0
    }
}

impl TryFrom<String> for SkillId {
    type Error = ServiceError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        SkillId::new(s)
    }
}

impl AsRef<str> for SkillId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl serde::Serialize for SkillId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for SkillId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        SkillId::new(s).map_err(serde::de::Error::custom)
    }
}

/// Main service error type
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Execution error: {0}")]
    Execution(#[from] crate::execution::ExecutionError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Event error: {0}")]
    Event(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Skill not found: {0}")]
    SkillNotFound(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Skill already indexed: {0}")]
    AlreadyIndexed(String),

    #[error("Custom error: {0}")]
    Custom(String),
}

/// Main FastSkill service
///
/// Note: This struct does not derive Debug because it contains Arc<dyn Trait> fields
/// which cannot implement Debug. This is acceptable for enterprise software.
pub struct FastSkillService {
    /// Service configuration
    config: ServiceConfig,

    /// Skill manager (shared instance)
    skill_manager: Arc<dyn crate::core::skill_manager::SkillManagementService>,

    /// Metadata service (depends on skill manager)
    metadata_service: Arc<dyn crate::core::metadata::MetadataService>,

    /// Vector index service (optional, for embedding search)
    vector_index_service: Option<Arc<dyn crate::core::vector_index::VectorIndexService>>,

    /// Embedding provider (optional), injected at the CLI/serve edge where the
    /// API key is loaded. `None` ⇒ reindex skips silently (ADR-0002/0005).
    embedding_service: Option<Arc<dyn crate::core::embedding::EmbeddingService>>,

    /// Repository access (optional), injected at the edge from the resolved
    /// `repos` config. Needed to fetch `Origin::Repository` skills; `None` ⇒
    /// a repository-origin install returns a clear "no repositories configured" error.
    repository_manager: Option<Arc<crate::core::repository::RepositoryManager>>,

    /// Project root the install seam writes the Manifest + Lock under, injected at
    /// the edge. The CLI leaves this `None` (it resolves the project from the
    /// process cwd, which is correct for a CLI); the `serve` path MUST inject the
    /// served project's root so `add_from_origin` doesn't write relative to the
    /// server's arbitrary working directory.
    project_root: Option<PathBuf>,

    /// Skill storage backend
    storage: Arc<dyn crate::storage::StorageBackend>,

    /// Hot reload manager
    hot_reload_manager: Option<Arc<crate::storage::hot_reload::HotReloadManager>>,

    /// On-disk skill content/index cache (PRD 006 / RFQ 004). Rooted per
    /// `config.skill_cache_root` (env-resolved when unset).
    skill_cache: crate::core::cache::SkillCache,

    /// Service state
    initialized: bool,
}

/// Directories always skipped when scanning the skill storage tree for SKILL.md files.
const SKIPPED_DIRS: &[&str] = &["node_modules", "target", "__pycache__"];

/// Returns `true` for directory names that should not be descended into during auto-indexing.
fn should_skip_directory(name: &str) -> bool {
    name.starts_with('.') || SKIPPED_DIRS.contains(&name)
}

impl FastSkillService {
    async fn build_storage_backend(
        config: &ServiceConfig,
    ) -> Result<Arc<dyn crate::storage::StorageBackend>, ServiceError> {
        Ok(Arc::new(
            crate::storage::FilesystemStorage::new(config.skill_storage_path.clone()).await?,
        ))
    }

    fn build_vector_index_service(
        config: &ServiceConfig,
    ) -> Option<Arc<dyn crate::core::vector_index::VectorIndexService>> {
        config.embedding.as_ref().map(|embedding_config| {
            Arc::new(
                crate::core::vector_index::VectorIndexServiceImpl::with_config(
                    embedding_config,
                    &config.skill_storage_path,
                ),
            ) as Arc<dyn crate::core::vector_index::VectorIndexService>
        })
    }

    /// Create a new service instance
    pub async fn new(config: ServiceConfig) -> Result<Self, ServiceError> {
        crate::init_logging();
        info!("Initializing FastSkill service v{}", crate::VERSION);

        let storage = Self::build_storage_backend(&config).await?;
        let event_bus = Arc::new(crate::events::EventBus::new());
        let skill_manager = Arc::new(crate::core::skill_manager::SkillManager::new());
        let metadata_service = Arc::new(crate::core::metadata::MetadataServiceImpl::new(
            skill_manager.clone(),
        ));
        let vector_index_service = Self::build_vector_index_service(&config);
        let hot_reload_manager = if config.hot_reload.enabled {
            Some(Arc::new(crate::storage::hot_reload::HotReloadManager::new(
                storage.clone(),
                event_bus.clone(),
            )?))
        } else {
            None
        };

        let skill_cache = match &config.skill_cache_root {
            Some(root) => crate::core::cache::SkillCache::at_root(root.clone()),
            None => crate::core::cache::SkillCache::from_env()?,
        };

        Ok(Self {
            config,
            skill_manager,
            metadata_service,
            vector_index_service,
            embedding_service: None,
            repository_manager: None,
            project_root: None,
            storage,
            hot_reload_manager,
            skill_cache,
            initialized: false,
        })
    }

    /// Inject an embedding provider (edge-constructed, holds the API key). Enables
    /// the core reindex seam; without it reindex skips silently.
    pub fn with_embedding_service(
        mut self,
        embedding: Arc<dyn crate::core::embedding::EmbeddingService>,
    ) -> Self {
        self.embedding_service = Some(embedding);
        self
    }

    /// Inject repository access resolved from the edge `repos` config. Enables
    /// fetching `Origin::Repository` skills.
    pub fn with_repository_manager(
        mut self,
        manager: Arc<crate::core::repository::RepositoryManager>,
    ) -> Self {
        self.repository_manager = Some(manager);
        self
    }

    /// The injected embedding provider, if any.
    pub fn embedding_service(&self) -> Option<&Arc<dyn crate::core::embedding::EmbeddingService>> {
        self.embedding_service.as_ref()
    }

    /// The injected repository manager, if any.
    pub fn repository_manager(&self) -> Option<&Arc<crate::core::repository::RepositoryManager>> {
        self.repository_manager.as_ref()
    }

    /// The on-disk skill content/index cache (PRD 006 / RFQ 004). `fetch_git`
    /// (US-002) goes through it before any clone; later stories (US-003
    /// registry, US-004 local) wire their own fetch paths through the same
    /// seam.
    pub fn skill_cache(&self) -> &crate::core::cache::SkillCache {
        &self.skill_cache
    }

    /// Inject the project root the install seam writes Manifest/Lock under (the
    /// served project's root for `serve`). When unset the seam falls back to the
    /// process cwd (correct for the CLI).
    pub fn with_project_root(mut self, root: PathBuf) -> Self {
        self.project_root = Some(root);
        self
    }

    /// The injected project root, if any.
    pub fn project_root(&self) -> Option<&PathBuf> {
        self.project_root.as_ref()
    }

    /// Initialize the service
    pub async fn initialize(&mut self) -> Result<(), ServiceError> {
        if self.initialized {
            return Ok(());
        }

        info!("Initializing service components...");

        // Initialize storage
        self.storage.initialize().await?;

        // Initialize hot reload if enabled
        if let Some(hot_reload) = &self.hot_reload_manager {
            hot_reload
                .enable_hot_reloading(self.config.hot_reload.watch_paths.clone())
                .await?;
        }

        // Auto-index skills from filesystem
        self.auto_index_skills_from_filesystem().await?;

        self.initialized = true;
        info!("Service initialization complete");

        Ok(())
    }

    /// Shutdown the service
    pub async fn shutdown(&mut self) -> Result<(), ServiceError> {
        info!("Shutting down service...");

        // Disable hot reloading
        if let Some(hot_reload) = &self.hot_reload_manager {
            hot_reload.disable_hot_reloading().await?;
        }

        // Clear any caches
        self.storage.clear_cache().await?;

        self.initialized = false;
        info!("Service shutdown complete");

        Ok(())
    }

    /// Get skill manager service
    pub fn skill_manager(&self) -> Arc<dyn crate::core::skill_manager::SkillManagementService> {
        self.skill_manager.clone()
    }

    /// Get metadata service
    pub fn metadata_service(&self) -> Arc<dyn crate::core::metadata::MetadataService> {
        self.metadata_service.clone()
    }

    /// Get vector index service (if available)
    pub fn vector_index_service(
        &self,
    ) -> Option<Arc<dyn crate::core::vector_index::VectorIndexService>> {
        self.vector_index_service.clone()
    }

    /// Get routing service
    pub fn routing_service(&self) -> Arc<dyn crate::core::routing::RoutingService> {
        Arc::new(crate::core::routing::RoutingServiceImpl::new(
            self.metadata_service.clone(),
        ))
    }

    /// Get service configuration
    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }

    /// Get context resolver for machine-first skill resolution
    pub fn context_resolver(&self) -> crate::core::context_resolver::ContextResolver {
        crate::core::context_resolver::ContextResolver::new(
            self.skill_manager.clone(),
            self.metadata_service.clone(),
            self.vector_index_service.clone(),
            self.config.embedding.clone(),
            self.config.skill_storage_path.clone(),
        )
    }

    /// Check if service is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Auto-index skills from the filesystem by scanning for SKILL.md files
    async fn auto_index_skills_from_filesystem(&self) -> Result<(), ServiceError> {
        use crate::core::skill_walk::walk_skill_storage;

        let mut indexed_count = 0;

        // Walk the skills directory. `walk_skill_storage` resolves exactly one
        // hop for a top-level entry that is itself a symlink to a directory
        // (the develop-in-place / editable workflow, spec 010), and never
        // follows a symlink encountered any deeper. A bad/looping top-level
        // symlink surfaces as a single `Err` for that entry — skip it with a
        // warning rather than failing the whole index, since one broken link
        // must not hide every other skill.
        for entry in walk_skill_storage(&self.config.skill_storage_path, should_skip_directory) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    tracing::warn!("Skipping unreadable skill storage entry: {}", e);
                    continue;
                }
            };

            // Look for SKILL.md or skill.md files
            if entry.file_type().is_file() {
                let fname = entry.file_name();
                if fname == "SKILL.md" || fname == "skill.md" {
                    let skill_file = entry.path();

                    match self.try_index_skill_from_file(skill_file).await {
                        Ok(_) => {
                            indexed_count += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to index skill at {}: {}",
                                skill_file.display(),
                                e
                            );
                        }
                    }
                }
            }
        }

        if indexed_count > 0 {
            info!("Auto-indexed {} skills from filesystem", indexed_count);
        }

        Ok(())
    }

    /// Try to index a single skill from its SKILL.md file
    async fn try_index_skill_from_file(
        &self,
        skill_file: &std::path::Path,
    ) -> Result<(), ServiceError> {
        // Read the SKILL.md file
        let content = tokio::fs::read_to_string(skill_file)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read SKILL.md: {}", e)))?;

        // Parse the frontmatter
        let frontmatter = crate::core::metadata::parse_yaml_frontmatter(&content)?;

        // Get the skill directory (parent of SKILL.md)
        let skill_dir = skill_file
            .parent()
            .ok_or_else(|| ServiceError::Custom("SKILL.md has no parent directory".to_string()))?;

        // Use directory name as skill ID
        let skill_id_str = skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ServiceError::Custom("Invalid skill directory name".to_string()))?
            .to_string();
        let skill_id = SkillId::new(skill_id_str)?;

        // Create skill definition from frontmatter. This is a directory-scan
        // registration path with no real provenance to record — the skill IS a
        // local directory on disk, so `Origin::Local` is the accurate (and
        // behavior-neutral) origin: previously all source_* fields were simply
        // left `None` for this path.
        let mut skill = crate::core::skill_manager::SkillDefinition::new(
            skill_id.clone(),
            frontmatter.name,
            frontmatter.description,
            frontmatter.version.unwrap_or_else(|| "1.0.0".to_string()),
            crate::core::origin::Origin::Local {
                path: skill_dir.to_path_buf(),
                editable: false,
            },
        );

        // Set additional fields
        skill.author = frontmatter.author;
        skill.skill_file = skill_file.to_path_buf();

        // Set timestamps
        skill.created_at = chrono::Utc::now();
        skill.updated_at = chrono::Utc::now();

        // Try to register the skill (ignore if it is already indexed)
        match self.skill_manager.register_skill(skill).await {
            Ok(_) => Ok(()),
            Err(ServiceError::AlreadyIndexed(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Non-hidden storage root under `tmp`. `TempDir`'s own directory name
    /// begins with `.tmp`, and the auto-indexer's `should_skip_directory`
    /// filter skips any directory starting with `.` — so the storage root
    /// used by these tests must be a non-hidden child of the temp dir.
    fn skills_root(tmp: &TempDir) -> PathBuf {
        let root = tmp.path().join("store");
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_skill_md(dir: &std::path::Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: a test skill\nversion: 1.0.0\n---\n# {name}\n"
            ),
        )
        .unwrap();
    }

    /// Spec 010: a skill directory installed as a symlink to a checkout
    /// elsewhere (the develop-in-place / editable workflow) must still be
    /// discovered by the filesystem auto-indexer that backs `list`/`search` —
    /// not silently skipped just because the top-level entry is a symlink.
    #[tokio::test]
    async fn test_auto_index_finds_symlinked_skill_dir() {
        let temp_dir = TempDir::new().unwrap();
        let storage = skills_root(&temp_dir);

        // A regular (non-symlink) skill, to confirm no regression alongside
        // the symlink fix.
        write_skill_md(&storage.join("regular-skill"), "Regular Skill");

        // The skill really lives elsewhere; only a symlink sits in storage.
        let real_target = temp_dir.path().join("dev-checkout");
        write_skill_md(&real_target, "Linked Skill");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_target, storage.join("linked-skill")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real_target, storage.join("linked-skill")).unwrap();

        let config = ServiceConfig {
            skill_storage_path: storage,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let skills = service.skill_manager().list_skills().await.unwrap();
        let ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();

        assert!(
            ids.contains(&"regular-skill"),
            "regular skill dir must still be indexed; found: {ids:?}"
        );
        assert!(
            ids.contains(&"linked-skill"),
            "symlinked skill dir must be discovered by the auto-indexer; found: {ids:?}"
        );
    }

    /// Spec 010: a self-referential (looping) symlink at the top level of
    /// `skill_storage_path` must be skipped, not turned into a hard failure
    /// of the whole auto-index pass — the rest of the skills must still be
    /// indexed.
    #[tokio::test]
    async fn test_auto_index_skips_cyclic_symlink_without_failing_whole_index() {
        let temp_dir = TempDir::new().unwrap();
        let storage = skills_root(&temp_dir);

        write_skill_md(&storage.join("good-skill"), "Good Skill");

        // Self-referential symlink: storage/looping-skill -> storage/looping-skill.
        #[cfg(unix)]
        std::os::unix::fs::symlink(storage.join("looping-skill"), storage.join("looping-skill"))
            .unwrap();

        let config = ServiceConfig {
            skill_storage_path: storage,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();

        // The looping symlink must not abort initialization.
        service
            .initialize()
            .await
            .expect("a cyclic symlink must be skipped, not fail the whole index");

        let skills = service.skill_manager().list_skills().await.unwrap();
        let ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();

        assert!(
            ids.contains(&"good-skill"),
            "unrelated skill must still be indexed despite a sibling cyclic symlink; found: {ids:?}"
        );
        assert!(
            !ids.contains(&"looping-skill"),
            "the cyclic symlink itself must not be indexed as a skill"
        );
    }

    #[tokio::test]
    async fn test_service_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        assert!(!service.is_initialized());

        service.initialize().await.unwrap();
        assert!(service.is_initialized());
    }

    #[tokio::test]
    async fn test_service_shutdown() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        service.shutdown().await.unwrap();
        assert!(!service.is_initialized());
    }

    #[test]
    fn test_skill_id_new_validates_input() {
        assert!(SkillId::new("valid-id".to_string()).is_ok());
        assert!(SkillId::new("valid_id_123".to_string()).is_ok());
        assert!(SkillId::new("".to_string()).is_err());
        assert!(SkillId::new("bad/id".to_string()).is_err());
        assert!(SkillId::new("id with spaces".to_string()).is_err());
    }

    #[test]
    fn test_skill_id_try_from_validates_input() {
        // TryFrom should validate input
        assert!(SkillId::try_from("valid-id".to_string()).is_ok());
        assert!(SkillId::try_from("".to_string()).is_err());
        assert!(SkillId::try_from("bad/id".to_string()).is_err());
    }

    #[test]
    fn test_skill_id_new_rejects_too_long() {
        let too_long = "a".repeat(256);
        assert!(SkillId::new(too_long).is_err());

        let exactly_max = "a".repeat(255);
        assert!(SkillId::new(exactly_max).is_ok());
    }

    #[test]
    fn test_skill_id_into_string_and_from_conversions() {
        let id = SkillId::new("my-skill".to_string()).unwrap();
        assert_eq!(id.clone().into_string(), "my-skill");

        let s: String = id.into();
        assert_eq!(s, "my-skill");
    }

    #[test]
    fn test_skill_id_as_ref() {
        let id = SkillId::new("my-skill".to_string()).unwrap();
        assert_eq!(id.as_ref(), "my-skill");
    }

    #[test]
    fn test_skill_id_serde_roundtrip() {
        let id = SkillId::new("my-skill".to_string()).unwrap();

        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"my-skill\"");

        let round_tripped: SkillId = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped.as_str(), "my-skill");
    }

    #[test]
    fn test_skill_id_deserialize_rejects_invalid_value() {
        // An id embedding a forward slash fails SkillId::new's validation,
        // and Deserialize must surface that as a deserialize error rather
        // than panicking or silently accepting it.
        let result: Result<SkillId, _> = serde_json::from_str("\"bad/id\"");
        assert!(result.is_err());
    }

    /// `initialize()` guards on `self.initialized` and must be a no-op the
    /// second time it is called (no re-running of storage init / auto-index).
    #[tokio::test]
    async fn test_service_initialize_is_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        assert!(service.is_initialized());

        // Calling it again must short-circuit on the `self.initialized` guard
        // and still return Ok, not re-run initialization.
        service.initialize().await.unwrap();
        assert!(service.is_initialized());
    }

    /// With hot reload enabled, `initialize()` constructs a `HotReloadManager`
    /// and enables it, and `shutdown()` disables it — exercising the
    /// otherwise-untested `Some(hot_reload)` branches.
    #[tokio::test]
    async fn test_service_hot_reload_enabled_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            hot_reload: HotReloadConfig {
                enabled: true,
                watch_paths: vec![],
                debounce_ms: 100,
                auto_reload: true,
            },
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        assert!(service.is_initialized());

        service.shutdown().await.unwrap();
        assert!(!service.is_initialized());
    }

    /// Re-running the filesystem auto-indexer over skills it already indexed
    /// must not error: `try_index_skill_from_file` maps
    /// `ServiceError::AlreadyIndexed` back to `Ok(())` rather than propagating
    /// it, so a rescan is idempotent.
    #[tokio::test]
    async fn test_auto_index_rerun_is_idempotent_for_already_indexed_skills() {
        let temp_dir = TempDir::new().unwrap();
        let storage = skills_root(&temp_dir);
        write_skill_md(&storage.join("my-skill"), "My Skill");

        let config = ServiceConfig {
            skill_storage_path: storage,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let skills = service.skill_manager().list_skills().await.unwrap();
        assert_eq!(skills.len(), 1);

        // Re-run the private auto-indexer directly (initialize() itself is
        // guarded against a second run) to exercise the AlreadyIndexed path.
        service
            .auto_index_skills_from_filesystem()
            .await
            .expect("re-indexing an already-indexed skill must not error");

        let skills = service.skill_manager().list_skills().await.unwrap();
        assert_eq!(skills.len(), 1, "rescanning must not duplicate the skill");
    }
}

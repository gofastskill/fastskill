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

    /// Skill storage backend
    storage: Arc<dyn crate::storage::StorageBackend>,

    /// Hot reload manager
    hot_reload_manager: Option<Arc<crate::storage::hot_reload::HotReloadManager>>,

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

        Ok(Self {
            config,
            skill_manager,
            metadata_service,
            vector_index_service,
            embedding_service: None,
            repository_manager: None,
            storage,
            hot_reload_manager,
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
        use walkdir::WalkDir;

        let mut indexed_count = 0;

        // Walk the skills directory recursively
        for entry in WalkDir::new(&self.config.skill_storage_path)
            .into_iter()
            .filter_entry(|e| {
                !e.file_name()
                    .to_str()
                    .map(should_skip_directory)
                    .unwrap_or(false)
            })
        {
            let entry = entry.map_err(|e| {
                ServiceError::Custom(format!("Failed to read directory entry: {}", e))
            })?;

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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
}

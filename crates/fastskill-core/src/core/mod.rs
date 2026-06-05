//! Core service layer modules

pub mod analysis;
pub mod blob_storage;
pub mod build_cache;
pub mod change_detection;
pub mod context_resolver;
pub mod dependencies;
pub mod dependency_resolver;
pub mod embedding;
pub mod frontmatter;
pub mod lock;
pub mod manifest;
pub mod metadata;
pub mod packaging;
pub mod project;
pub mod project_config;
pub mod reconciliation;
pub mod registry;
pub mod registry_index;
pub mod repository;
pub mod resolver;
pub mod routing;
pub mod service;
pub mod skill_manager;
pub mod sources;
pub mod tool_calling;
pub mod update;
pub mod validation;
pub mod vector_index;
pub mod version;
pub mod version_bump;

// Re-export main types for convenience
// Note: Selective re-exports to avoid conflicts
pub use blob_storage::{create_blob_storage, BlobStorage, BlobStorageConfig, LocalBlobStorage};
pub use build_cache::{BuildCache, SkillCacheEntry};
pub use change_detection::{
    calculate_skill_hash, detect_changed_skills_git, detect_changed_skills_hash,
};
// dependencies
pub use dependencies::{Dependency, DependencyError, DependencyGraph};
pub use dependency_resolver::{DependencyResolutionError, DependencyResolver, SkillInstallItem};

// embedding
pub use embedding::{EmbeddingService, OpenAIEmbeddingService};

// lock
pub use lock::{
    global_lock_path, project_lock_path, GlobalLockMetadata, GlobalLockedSkillEntry,
    GlobalSkillsLock, LockError, LockMismatch, ProjectLockMetadata, ProjectLockedSkillEntry,
    ProjectSkillsLock,
};

// manifest
pub use manifest::{
    AuthConfig, AuthType, DependenciesSection, DependencySource, DependencySpec,
    EmbeddingConfigToml, EvalConfigToml, FastSkillToolConfig, FileResolutionResult,
    HttpServerConfigToml, ManifestError, ManifestMetadata, MetadataSection, ProjectContext,
    RepositoryConnection, RepositoryDefinition, RepositoryType, SkillEntry, SkillProjectToml,
    SkillSource, SkillsManifest, SourceSpecificFields, ToolSection,
};

// metadata
pub use metadata::{
    parse_yaml_frontmatter, MetadataService, MetadataServiceImpl, SkillFrontmatter, SkillMetadata,
};

// packaging
pub use packaging::{
    calculate_checksum, create_build_metadata, package_skill, package_skill_with_id,
    BuildEnvironment, BuildMetadata,
};

// project_config
pub use project_config::{load_project_config, ProjectConfig};

// registry
pub use registry::{
    AuthConfig as RegistryAuthConfig, IndexEntry, RegistryClient, RegistryConfig,
    RegistryConfigManager, StorageConfig,
};

// registry_index
pub use registry_index::{
    create_registry_structure, get_skill_index_path, get_version_metadata, migrate_index_format,
    read_skill_versions, IndexMetadata, VersionEntry, VersionMetadata,
};

// repository
pub use repository::{
    RepositoriesConfig, RepositoryAuth, RepositoryConfig as RepoConfig,
    RepositoryDefinition as RepositoryDef, RepositoryManager, RepositoryType as RepoType,
};

// resolver
pub use resolver::{
    ConflictStrategy, PackageResolver, ResolutionResult, ResolverError, SkillCandidate,
};

// routing
pub use routing::{QueryContext, RoutedSkill, RoutingService, RoutingServiceImpl};

// service
pub use service::{
    CacheConfig, EmbeddingConfig, FastSkillService, HotReloadConfig, HttpServerConfig,
    SecurityConfig, ServiceConfig, ServiceError, SkillId,
};

// skill_manager
pub use skill_manager::{
    SkillDefinition, SkillFilters, SkillManagementService, SkillManager, SkillUpdate, SourceType,
};

// sources
pub use sources::{
    MarketplaceJson, MarketplaceSkill, SkillInfo, SourceAuth, SourceConfig, SourceDefinition,
    SourcesConfig, SourcesError, SourcesManager,
};

// tool_calling
pub use tool_calling::{AvailableTool, ToolCallingService, ToolCallingServiceImpl, ToolResult};

// update
pub use update::{UpdateError, UpdateInfo, UpdateService, UpdateStrategy};

// validation
pub use validation::{
    validate_identifier, validate_project_structure, validate_semver, validate_uniqueness,
    ValidationError,
};

// vector_index
pub use vector_index::{IndexedSkill, SkillMatch, VectorIndexService, VectorIndexServiceImpl};

// version
pub use version::{compare_versions, is_newer, VersionConstraint, VersionError};

// version_bump
pub use version_bump::{
    bump_version, get_current_version, parse_version, update_skill_version, BumpType,
};

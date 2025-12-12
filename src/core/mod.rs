//! Core service layer modules

pub mod blob_storage;
pub mod build_cache;
pub mod change_detection;
pub mod dependencies;
pub mod embedding;
pub mod loading;
pub mod lock;
pub mod manifest;
pub mod metadata;
pub mod packaging;
pub mod project;
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
pub use dependencies::*;
pub use embedding::*;
pub use loading::*;
pub use lock::*;
pub use manifest::*;
pub use metadata::*;
pub use packaging::{
    calculate_checksum, create_build_metadata, package_skill, package_skill_with_id,
    BuildEnvironment, BuildMetadata,
};
pub use registry::{
    AuthConfig, IndexEntry, RegistryClient, RegistryConfig, RegistryConfigManager, StorageConfig,
};
pub use registry_index::{
    create_registry_structure, get_skill_index_path, get_version_metadata, migrate_index_format,
    read_skill_versions, IndexMetadata, VersionEntry, VersionMetadata,
};
pub use repository::{
    RepositoriesConfig, RepositoryAuth, RepositoryConfig as RepoConfig, RepositoryDefinition,
    RepositoryManager, RepositoryType,
};
pub use resolver::*;
pub use routing::*;
pub use service::*;
pub use skill_manager::*;
pub use sources::*;
pub use tool_calling::*;
pub use update::*;
pub use validation::*;
pub use vector_index::*;
pub use version::*;
pub use version_bump::{
    bump_version, get_current_version, parse_version, update_skill_version, BumpType,
};

//! # FastSkill Service Layer (Rust Implementation)
//!
//! This crate provides a high-performance, memory-safe implementation of the FastSkill
//! service layer. It serves as the reference implementation for other language
//! bindings and can be used standalone or as a library.
//!
//! ## Architecture
//!
//! The service layer provides:
//! - Skill management (CRUD operations)
//! - Metadata and discovery services
//! - Progressive loading of skill content
//! - Tool calling and script execution
//! - Hot reloading capabilities
//! - Security sandboxing
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use fastskill_core::{FastSkillService, ServiceConfig};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ServiceConfig {
//!         skill_storage_path: PathBuf::from("./skills"),
//!         ..Default::default()
//!     };
//!
//!     let service = FastSkillService::new(config).await?;
//!
//!     // List available skills
//!     let skills = service.skill_manager().list_skills().await?;
//!     println!("Found {} skills", skills.len());
//!
//!     // Discover relevant skills
//!     let relevant_skills = service.metadata_service()
//!         .discover_skills("extract text from PDF")
//!         .await?;
//!
//!     println!("Found {} relevant skills", relevant_skills.len());
//!
//!     Ok(())
//! }
//! ```

pub mod core;
pub mod events;
pub mod execution;
pub mod http;
pub mod output;
pub mod search;
pub mod security;
pub mod storage;
pub mod utils;
pub mod validation;
pub mod write_ops;

pub use core::context_resolver::{
    ContentMode, ContextResolver, ResolveContextRequest, ResolveContextResponse, ResolveScope,
    ResolvedSkill,
};
pub use core::embedding::{EmbeddingService, OpenAIEmbeddingService};
pub use core::manifest::{SkillProjectToml, MANIFEST_SCHEMA_VERSION};
pub use core::metadata::{
    parse_yaml_frontmatter, MetadataService, SkillFrontmatter, SkillMetadata,
};
pub use core::routing::{RoutedSkill, RoutingService};
pub use core::service::SkillId;
pub use core::service::{EmbeddingConfig, FastSkillService, ServiceConfig, ServiceError};
pub use core::skill_manager::{SkillDefinition, SkillManagementService};
pub use core::vector_index::{
    IndexedSkill, SkillMatch, VectorIndexService, VectorIndexServiceImpl,
};

// Re-export search and output types
pub use output::OutputFormat;
pub use search::{execute, SearchError, SearchQuery, SearchResultItem, SearchScope};

/// Version of the service layer
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Initialize logging for the service layer (safe for testing)
pub fn init_logging() {
    init_logging_with_verbose(false)
}

/// Initialize logging for the service layer with optional verbose mode
pub fn init_logging_with_verbose(verbose: bool) {
    // Only initialize logging once
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        use tracing_subscriber::EnvFilter;

        let default_level = if verbose {
            "fastskill=info"
        } else {
            "fastskill=warn"
        };
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| default_level.into());

        let subscriber = tracing_subscriber::fmt().with_env_filter(filter).finish();

        // This will fail silently if already initialized
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

pub mod test_utils;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::execution::ExecutionConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_service_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let service = FastSkillService::new(config).await.unwrap();
        assert!(service.skill_manager().list_skills().await.is_ok());
    }

    // --- Relocated from tests/unit/service.rs (dead orphaned integration test
    // directory that used `crate::` paths only valid inside this crate, and an
    // outdated `ExecutionConfig` import path). Moved here as in-crate unit
    // tests so they actually compile and run. ---

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

    #[tokio::test]
    async fn test_service_configuration() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            execution: ExecutionConfig {
                default_timeout: std::time::Duration::from_secs(60),
                max_memory_mb: 1024,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut service = FastSkillService::new(config.clone()).await.unwrap();
        service.initialize().await.unwrap();

        assert_eq!(
            service.config().execution.default_timeout,
            std::time::Duration::from_secs(60)
        );
        assert_eq!(service.config().execution.max_memory_mb, 1024);
    }

    #[tokio::test]
    async fn test_skill_manager_access() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        // Test that we can access the skill manager
        let skill_manager = service.skill_manager();
        assert!(skill_manager.list_skills().await.is_ok());
    }

    #[tokio::test]
    async fn test_metadata_service_access() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        // Test that we can access the metadata service
        let metadata_service = service.metadata_service();
        assert!(metadata_service.discover_skills("test query").await.is_ok());
    }

    #[tokio::test]
    async fn test_routing_service_access() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        // Test that we can access the routing service
        let routing_service = service.routing_service();
        assert!(routing_service
            .find_relevant_skills("test query", None)
            .await
            .is_ok());
    }

    #[test]
    fn test_version_constant() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_init_logging() {
        // This should not panic
        init_logging();
    }
}

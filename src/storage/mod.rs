//! Storage backend implementations

use crate::core::service::ServiceError;
use async_trait::async_trait;

// Re-export storage types
pub use filesystem::{FilesystemStorage, StorageStats};

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn initialize(&self) -> Result<(), ServiceError>;
    async fn clear_cache(&self) -> Result<(), ServiceError>;
    // Add other methods as needed
}

pub mod filesystem;
#[cfg(feature = "git-support")]
pub mod git;
pub mod hot_reload;
pub mod vector_index;
pub mod zip;

// Re-export main types
pub use hot_reload::HotReloadManager;
pub use zip::ZipHandler;

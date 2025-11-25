//! Registry module for skill package registries

pub mod auth;
pub mod client;
pub mod config;
pub mod index_manager;
pub mod staging;
pub mod validation_worker;

pub use auth::{ApiKey, Auth, GitHubPat, SshKey};
pub use client::{IndexEntry, RegistryClient};
pub use config::{
    AuthConfig, DefaultRegistryConfig, RegistriesConfig, RegistryConfig, RegistryConfigManager,
    StorageConfig,
};
pub use staging::{StagingManager, StagingMetadata, StagingStatus};
pub use validation_worker::{ValidationWorker, ValidationWorkerConfig};

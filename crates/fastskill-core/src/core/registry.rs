//! Registry module for skill package registries

pub mod auth;
pub mod client;
pub mod config;

pub use auth::{ApiKey, Auth, GitHubPat, SshKey};
pub use client::{IndexEntry, RegistryClient};
pub use config::{
    AuthConfig, DefaultRegistryConfig, RegistriesConfig, RegistryConfig, RegistryConfigManager,
    StorageConfig,
};

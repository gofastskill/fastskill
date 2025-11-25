//! FastSkill Claude API Proxy Module
//!
//! This module provides proxy functionality that intercepts Anthropic API calls
//! and injects skills into messages.create() requests based on the container parameter.

pub mod config;
pub mod handler;
pub mod server;
pub mod skill_injection;
pub mod skill_storage;

pub use config::{ProxyCommandArgs, ProxyConfig};
pub use handler::ProxyHandler;
pub use server::{serve_proxy, ProxyServer};

/// Trait for service compatibility with the proxy
pub trait ProxyService: Send + Sync {
    fn skill_manager(
        &self,
    ) -> std::sync::Arc<dyn crate::core::skill_manager::SkillManagementService>;
    fn metadata_service(&self) -> std::sync::Arc<dyn crate::core::metadata::MetadataService>;
}

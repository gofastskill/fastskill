//! Marketplace fetch helpers.
//! The shared fetch logic lives in sources/mod.rs (SourcesManager::load_marketplace_from_url_with_branch
//! and get_marketplace_json). This module re-exports for external callers.
pub use super::{MarketplaceJson, MarketplaceSkill};

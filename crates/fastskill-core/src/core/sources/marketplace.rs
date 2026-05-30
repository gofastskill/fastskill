//! Marketplace JSON structures and caching.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Marketplace.json structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceJson {
    pub version: String,
    pub skills: Vec<MarketplaceSkill>,
}

/// Skill entry in marketplace.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
}

/// Claude Code marketplace.json format structures
/// This format is used by Claude Code standard repositories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeMarketplaceJson {
    pub name: String,
    #[serde(default)]
    pub owner: Option<ClaudeCodeOwner>,
    #[serde(default)]
    pub metadata: Option<ClaudeCodeMetadata>,
    pub plugins: Vec<ClaudeCodePlugin>,
}

/// Owner information in Claude Code format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeOwner {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
}

/// Metadata in Claude Code format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeMetadata {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

/// Plugin entry in Claude Code format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodePlugin {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub strict: Option<bool>,
    pub skills: Vec<String>, // Array of skill paths (relative to repository root)
}

// Marketplace.json format: Only Claude Code format is supported

/// Cached marketplace.json entry
#[derive(Debug, Clone)]
pub(super) struct CachedMarketplace {
    pub(super) data: MarketplaceJson,
    pub(super) fetched_at: DateTime<Utc>,
    pub(super) ttl_seconds: u64,
}

impl CachedMarketplace {
    pub(super) fn is_expired(&self) -> bool {
        let now = Utc::now();
        let elapsed = (now - self.fetched_at).num_seconds() as u64;
        elapsed > self.ttl_seconds
    }
}

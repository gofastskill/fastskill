//! Build cache management for tracking skill versions and hashes

use crate::core::service::ServiceError;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Build cache structure for tracking skill versions and hashes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildCache {
    pub version: String,
    #[serde(default)]
    pub last_build: Option<String>,
    #[serde(default)]
    pub skills: HashMap<String, SkillCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCacheEntry {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_packaged: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
}

impl Default for BuildCache {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            last_build: None,
            skills: HashMap::new(),
        }
    }
}

impl BuildCache {
    /// Load build cache from file
    pub fn load(cache_path: &Path) -> Result<Self, ServiceError> {
        if !cache_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(cache_path).map_err(ServiceError::Io)?;

        let cache: BuildCache = serde_json::from_str(&content)
            .map_err(|e| ServiceError::Custom(format!("Failed to parse build cache: {}", e)))?;

        Ok(cache)
    }

    /// Save build cache to file
    pub fn save(&self, cache_path: &Path) -> Result<(), ServiceError> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent).map_err(ServiceError::Io)?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ServiceError::Custom(format!("Failed to serialize build cache: {}", e)))?;

        std::fs::write(cache_path, content).map_err(ServiceError::Io)?;

        Ok(())
    }

    /// Update cache entry for a skill
    pub fn update_skill(
        &mut self,
        skill_id: &str,
        version: &str,
        hash: &str,
        artifact_path: &Path,
        git_commit: Option<&str>,
    ) {
        let previous_version = self.skills.get(skill_id).map(|e| e.version.clone());

        let entry = SkillCacheEntry {
            version: version.to_string(),
            previous_version,
            hash: Some(hash.to_string()),
            last_packaged: Some(Utc::now().to_rfc3339()),
            artifact_path: Some(artifact_path.to_string_lossy().to_string()),
            git_commit: git_commit.map(|s| s.to_string()),
        };

        self.skills.insert(skill_id.to_string(), entry);
        self.last_build = Some(Utc::now().to_rfc3339());
    }

    /// Get cached version for a skill
    pub fn get_cached_version(&self, skill_id: &str) -> Option<String> {
        self.skills.get(skill_id).map(|e| e.version.clone())
    }

    /// Get cached hash for a skill
    pub fn get_cached_hash(&self, skill_id: &str) -> Option<String> {
        self.skills.get(skill_id).and_then(|e| e.hash.clone())
    }
}

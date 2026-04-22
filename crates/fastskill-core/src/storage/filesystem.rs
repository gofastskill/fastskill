//! Filesystem storage backend

use crate::core::metadata::SkillMetadata;
use crate::core::service::ServiceError;
use async_trait::async_trait;
use serde_json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Filesystem storage backend for skills
pub struct FilesystemStorage {
    /// Base directory for skill storage
    base_path: PathBuf,

    /// Metadata cache for performance
    metadata_cache: Arc<RwLock<HashMap<String, SkillMetadata>>>,

    /// Cache statistics
    cache_hits: Arc<RwLock<usize>>,
    cache_misses: Arc<RwLock<usize>>,
}

impl FilesystemStorage {
    /// Create a new filesystem storage backend
    pub async fn new(base_path: PathBuf) -> Result<Self, ServiceError> {
        // Create base directory if it doesn't exist
        fs::create_dir_all(&base_path).await.map_err(|e| {
            ServiceError::Custom(format!("Failed to create storage directory: {}", e))
        })?;

        info!("Initialized filesystem storage at: {}", base_path.display());

        Ok(Self {
            base_path,
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_hits: Arc::new(RwLock::new(0)),
            cache_misses: Arc::new(RwLock::new(0)),
        })
    }

    /// Get the path for a skill's metadata file
    fn get_skill_metadata_path(&self, skill_id: &str) -> PathBuf {
        self.base_path.join(skill_id).join("metadata.json")
    }

    /// Get the path for a skill's SKILL.md file
    fn get_skill_content_path(&self, skill_id: &str) -> PathBuf {
        self.base_path.join(skill_id).join("SKILL.md")
    }

    /// Load skill metadata from disk or cache
    pub async fn load_skill_metadata(
        &self,
        skill_id: &str,
    ) -> Result<Option<SkillMetadata>, ServiceError> {
        // Check cache first
        {
            let cache = self.metadata_cache.read().await;
            if let Some(metadata) = cache.get(skill_id) {
                let mut hits = self.cache_hits.write().await;
                *hits += 1;
                return Ok(Some(metadata.clone()));
            }
        }

        // Cache miss - load from disk
        let mut misses = self.cache_misses.write().await;
        *misses += 1;

        let metadata_path = self.get_skill_metadata_path(skill_id);

        if !metadata_path.exists() {
            return Ok(None);
        }

        // Read and parse metadata file
        let content = fs::read_to_string(&metadata_path)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read metadata file: {}", e)))?;

        let metadata: SkillMetadata = serde_json::from_str(&content)
            .map_err(|e| ServiceError::Custom(format!("Failed to parse metadata JSON: {}", e)))?;

        // Cache the metadata
        {
            let mut cache = self.metadata_cache.write().await;
            cache.insert(skill_id.to_string(), metadata.clone());
        }

        Ok(Some(metadata))
    }

    /// Save skill metadata to disk and cache
    pub async fn save_skill_metadata(
        &self,
        skill_id: &str,
        metadata: &SkillMetadata,
    ) -> Result<(), ServiceError> {
        let metadata_path = self.get_skill_metadata_path(skill_id);

        // Create skill directory if it doesn't exist
        if let Some(parent) = metadata_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ServiceError::Custom(format!("Failed to create skill directory: {}", e))
            })?;
        }

        // Serialize metadata to JSON
        let content = serde_json::to_string_pretty(metadata)
            .map_err(|e| ServiceError::Custom(format!("Failed to serialize metadata: {}", e)))?;

        // Write to disk
        fs::write(&metadata_path, &content)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to write metadata file: {}", e)))?;

        // Update cache
        {
            let mut cache = self.metadata_cache.write().await;
            cache.insert(skill_id.to_string(), metadata.clone());
        }

        debug!("Saved metadata for skill: {}", skill_id);
        Ok(())
    }

    /// Load skill content (SKILL.md)
    pub async fn load_skill_content(&self, skill_id: &str) -> Result<Option<String>, ServiceError> {
        let content_path = self.get_skill_content_path(skill_id);

        if !content_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&content_path)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read skill content: {}", e)))?;

        Ok(Some(content))
    }

    /// Save skill content (SKILL.md)
    pub async fn save_skill_content(
        &self,
        skill_id: &str,
        content: &str,
    ) -> Result<(), ServiceError> {
        let content_path = self.get_skill_content_path(skill_id);

        // Create skill directory if it doesn't exist
        if let Some(parent) = content_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ServiceError::Custom(format!("Failed to create skill directory: {}", e))
            })?;
        }

        // Write content to disk
        fs::write(&content_path, content)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to write skill content: {}", e)))?;

        debug!("Saved content for skill: {}", skill_id);
        Ok(())
    }

    /// List all skill IDs in storage
    pub async fn list_skill_ids(&self) -> Result<Vec<String>, ServiceError> {
        let mut skill_ids = Vec::new();

        // Read base directory
        let mut read_dir = fs::read_dir(&self.base_path).await.map_err(|e| {
            ServiceError::Custom(format!("Failed to read storage directory: {}", e))
        })?;

        // Collect all subdirectories (skill directories)
        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read directory entry: {}", e)))?
        {
            let path = entry.path();

            if path.is_dir() {
                if let Some(skill_id) = path.file_name() {
                    // Check if this directory has a valid skill structure
                    let skill_file = path.join("SKILL.md");
                    if skill_file.exists() {
                        skill_ids.push(skill_id.to_string_lossy().to_string());
                    }
                }
            }
        }

        Ok(skill_ids)
    }

    /// Delete a skill from storage
    pub async fn delete_skill(&self, skill_id: &str) -> Result<(), ServiceError> {
        let skill_path = self.base_path.join(skill_id);

        if skill_path.exists() {
            fs::remove_dir_all(&skill_path).await.map_err(|e| {
                ServiceError::Custom(format!("Failed to delete skill directory: {}", e))
            })?;

            // Remove from cache
            {
                let mut cache = self.metadata_cache.write().await;
                cache.remove(skill_id);
            }

            debug!("Deleted skill: {}", skill_id);
        }

        Ok(())
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> (usize, usize, usize, usize) {
        let cache_size = self.metadata_cache.read().await.len();
        let hits = *self.cache_hits.read().await;
        let misses = *self.cache_misses.read().await;

        (cache_size, hits, misses, hits + misses)
    }

    /// Clear metadata cache
    pub async fn clear_cache(&self) {
        self.metadata_cache.write().await.clear();
        *self.cache_hits.write().await = 0;
        *self.cache_misses.write().await = 0;
    }

    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> Result<StorageStats, ServiceError> {
        let total_skills = self.list_skill_ids().await?.len();

        // Calculate total size (simplified)
        let mut total_size = 0u64;
        for skill_id in self.list_skill_ids().await? {
            let metadata_path = self.get_skill_metadata_path(&skill_id);
            let content_path = self.get_skill_content_path(&skill_id);

            if let Ok(metadata) = fs::metadata(&metadata_path).await {
                total_size += metadata.len();
            }

            if let Ok(metadata) = fs::metadata(&content_path).await {
                total_size += metadata.len();
            }
        }

        Ok(StorageStats {
            total_skills,
            total_size_bytes: total_size,
            base_path: self.base_path.clone(),
        })
    }
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_skills: usize,
    pub total_size_bytes: u64,
    pub base_path: PathBuf,
}

#[async_trait]
impl crate::storage::StorageBackend for FilesystemStorage {
    async fn initialize(&self) -> Result<(), ServiceError> {
        // Create base directory if it doesn't exist
        fs::create_dir_all(&self.base_path).await.map_err(|e| {
            ServiceError::Custom(format!("Failed to create storage directory: {}", e))
        })?;

        info!(
            "Filesystem storage initialized at: {}",
            self.base_path.display()
        );
        Ok(())
    }

    async fn clear_cache(&self) -> Result<(), ServiceError> {
        self.clear_cache().await;
        Ok(())
    }
}

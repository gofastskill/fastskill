//! Progressive loading service implementation

use crate::core::metadata::SkillMetadata;
use crate::core::service::ServiceError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::warn;

/// Progressive loading service trait
#[async_trait]
pub trait ProgressiveLoadingService: Send + Sync {
    /// Load metadata for multiple skills (in-memory)
    async fn load_metadata(&self, skill_ids: &[String])
        -> Result<Vec<SkillMetadata>, ServiceError>;

    /// Load complete skill content based on context
    async fn load_skill_content(
        &self,
        skill_ids: &[String],
        context: Option<LoadingContext>,
    ) -> Result<Vec<LoadedSkill>, ServiceError>;

    /// Load reference content for a specific file
    async fn load_reference_content(
        &self,
        skill_id: &str,
        reference_path: &str,
    ) -> Result<String, ServiceError>;

    /// Preload relevant skills based on context
    async fn preload_relevant_skills(
        &self,
        context: LoadingContext,
    ) -> Result<Vec<PreloadedSkill>, ServiceError>;

    /// Optimize loading strategy based on context
    async fn optimize_loading_strategy(
        &self,
        context: LoadingContext,
    ) -> Result<LoadingStrategy, ServiceError>;

    /// Clear all caches
    async fn clear_cache(&self) -> Result<(), ServiceError>;

    /// Get cache statistics
    async fn get_cache_stats(&self) -> Result<CacheStats, ServiceError>;
}

/// Loading context for optimization decisions
#[derive(Debug, Clone)]
pub struct LoadingContext {
    /// The user's query
    pub query: String,

    /// Available tokens for loading content
    pub available_tokens: usize,

    /// Urgency level for loading
    pub urgency: LoadingUrgency,

    /// Previous conversation messages (for context)
    pub conversation_history: Vec<String>,

    /// User preferences (optional)
    pub user_preferences: Option<HashMap<String, String>>,
}

/// Urgency levels for loading decisions
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadingUrgency {
    /// Low priority - can defer loading
    Low,
    /// Normal priority - standard loading
    Medium,
    /// High priority - load immediately
    High,
    /// Critical - load everything immediately
    Critical,
}

/// Loading strategy recommendation
#[derive(Debug, Clone)]
pub struct LoadingStrategy {
    /// Priority order for skill loading
    pub priority: Vec<String>,

    /// Recommended load level for each skill
    pub load_levels: HashMap<String, LoadLevel>,

    /// Estimated total tokens required
    pub estimated_tokens: usize,

    /// Reasoning for the strategy
    pub reasoning: Vec<String>,
}

/// Content loading levels
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadLevel {
    /// Load only metadata
    Metadata,
    /// Load SKILL.md content
    Content,
    /// Load reference files as well
    References,
}

/// Complete loaded skill with all content
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    /// Skill metadata (always loaded)
    pub metadata: SkillMetadata,

    /// SKILL.md content (loaded on demand)
    pub content: Option<String>,

    /// Reference file contents (loaded on demand)
    pub references: Option<HashMap<String, String>>,

    /// Script file contents (loaded from disk when needed)
    pub script_contents: Option<HashMap<String, String>>,

    /// Asset file paths (never loaded into memory)
    pub asset_paths: Vec<PathBuf>,

    /// Whether the skill is ready for execution
    pub execution_ready: bool,

    /// When this skill was loaded
    pub loaded_at: Instant,

    /// Load level achieved
    pub load_level: LoadLevel,
}

/// Preloaded skill with metadata only
#[derive(Debug, Clone)]
pub struct PreloadedSkill {
    pub metadata: SkillMetadata,
    pub relevance_score: f64,
    pub recommended_load_level: LoadLevel,
}

/// Cache statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of metadata entries in cache
    pub metadata_cache_size: usize,

    /// Number of content entries in cache
    pub content_cache_size: usize,

    /// Cache hit rate for metadata
    pub metadata_hit_rate: f64,

    /// Cache hit rate for content
    pub content_hit_rate: f64,

    /// Total memory usage (MB)
    pub memory_usage_mb: f64,

    /// Average load time for metadata
    pub avg_metadata_load_time_ms: f64,

    /// Average load time for content
    pub avg_content_load_time_ms: f64,
}

/// Progressive loading service implementation
pub struct ProgressiveLoadingServiceImpl {
    /// Metadata cache (in-memory)
    metadata_cache: Arc<RwLock<HashMap<String, (SkillMetadata, Instant)>>>,

    /// Content cache (in-memory, smaller)
    content_cache: Arc<RwLock<HashMap<String, (String, Instant)>>>,

    /// Base directory for skill storage
    skills_base_path: PathBuf,

    /// Cache TTL for metadata (default: 1 hour)
    metadata_cache_ttl: Duration,

    /// Cache TTL for content (default: 30 minutes)
    content_cache_ttl: Duration,

    /// Maximum cache sizes
    max_metadata_cache_size: usize,
    max_content_cache_size: usize,

    /// Cache hit counters
    metadata_cache_hits: Arc<RwLock<usize>>,
    metadata_cache_misses: Arc<RwLock<usize>>,
    content_cache_hits: Arc<RwLock<usize>>,
    content_cache_misses: Arc<RwLock<usize>>,
}

impl ProgressiveLoadingServiceImpl {
    /// Create a new progressive loading service
    pub fn new(skills_base_path: PathBuf) -> Self {
        Self {
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            content_cache: Arc::new(RwLock::new(HashMap::new())),
            skills_base_path,
            metadata_cache_ttl: Duration::from_secs(3600), // 1 hour
            content_cache_ttl: Duration::from_secs(1800),  // 30 minutes
            max_metadata_cache_size: 1000,
            max_content_cache_size: 100,
            metadata_cache_hits: Arc::new(RwLock::new(0)),
            metadata_cache_misses: Arc::new(RwLock::new(0)),
            content_cache_hits: Arc::new(RwLock::new(0)),
            content_cache_misses: Arc::new(RwLock::new(0)),
        }
    }

    /// Load metadata for skills (always in memory)
    async fn load_skill_metadata(&self, skill_id: &str) -> Result<SkillMetadata, ServiceError> {
        // Check cache first
        {
            let cache = self.metadata_cache.read().await;
            if let Some((metadata, loaded_at)) = cache.get(skill_id) {
                if loaded_at.elapsed() < self.metadata_cache_ttl {
                    let mut hits = self.metadata_cache_hits.write().await;
                    *hits += 1;
                    return Ok(metadata.clone());
                }
            }
        }

        // Cache miss - load from disk
        let mut misses = self.metadata_cache_misses.write().await;
        *misses += 1;

        // Load skill definition from filesystem
        let skill_path = self.skills_base_path.join(skill_id).join("SKILL.md");

        if !skill_path.exists() {
            return Err(ServiceError::Custom(format!(
                "Skill file not found: {}",
                skill_path.display()
            )));
        }

        let content = tokio::fs::read_to_string(&skill_path).await?;

        // Parse YAML frontmatter and create metadata
        let metadata = self.parse_skill_metadata(skill_id, &content)?;

        // Cache the metadata
        {
            let mut cache = self.metadata_cache.write().await;

            // Clean up expired entries if cache is full
            if cache.len() >= self.max_metadata_cache_size {
                self.cleanup_expired_metadata(&mut cache).await;
            }

            cache.insert(skill_id.to_string(), (metadata.clone(), Instant::now()));
        }

        Ok(metadata)
    }

    /// Parse skill content to extract metadata
    fn parse_skill_metadata(
        &self,
        skill_id: &str,
        content: &str,
    ) -> Result<SkillMetadata, ServiceError> {
        // Simple parsing for now - extract YAML frontmatter
        let lines = content.lines();

        // Look for YAML frontmatter (---)
        let mut in_frontmatter = false;
        let mut frontmatter_lines = Vec::new();

        for line in lines {
            if line.trim() == "---" {
                if in_frontmatter {
                    break; // End of frontmatter
                } else {
                    in_frontmatter = true;
                    continue;
                }
            }

            if in_frontmatter {
                frontmatter_lines.push(line);
            }
        }

        // For now, create basic metadata from filename and content
        // In a real implementation, this would parse YAML frontmatter
        let name = skill_id.replace("-", " ").to_string();
        let description = content.lines().next().unwrap_or("No description").to_string();

        Ok(SkillMetadata {
            id: crate::core::service::SkillId::new(skill_id.to_string())?,
            name,
            description,
            version: "1.0.0".to_string(),
            author: None,
            enabled: true,
            token_estimate: content.len() / 4, // Rough estimate
            last_updated: std::time::SystemTime::now().into(),
        })
    }

    /// Load complete skill content (on demand) - internal method for single skill
    async fn load_skill_content_internal(&self, skill_id: &str) -> Result<String, ServiceError> {
        // Check content cache first
        {
            let cache = self.content_cache.read().await;
            if let Some((content, loaded_at)) = cache.get(skill_id) {
                if loaded_at.elapsed() < self.content_cache_ttl {
                    let mut hits = self.content_cache_hits.write().await;
                    *hits += 1;
                    return Ok(content.clone());
                }
            }
        }

        // Cache miss - load from disk
        let mut misses = self.content_cache_misses.write().await;
        *misses += 1;

        let skill_path = self.skills_base_path.join(skill_id).join("SKILL.md");

        if !skill_path.exists() {
            return Err(ServiceError::Custom(format!(
                "Skill file not found: {}",
                skill_path.display()
            )));
        }

        let content = tokio::fs::read_to_string(&skill_path).await?;

        // Cache the content
        {
            let mut cache = self.content_cache.write().await;

            // Clean up expired entries if cache is full
            if cache.len() >= self.max_content_cache_size {
                self.cleanup_expired_content(&mut cache).await;
            }

            cache.insert(skill_id.to_string(), (content.clone(), Instant::now()));
        }

        Ok(content)
    }

    /// Clean up expired metadata cache entries
    async fn cleanup_expired_metadata(
        &self,
        cache: &mut HashMap<String, (SkillMetadata, Instant)>,
    ) {
        let now = Instant::now();
        cache.retain(|_, (_, loaded_at)| now.duration_since(*loaded_at) < self.metadata_cache_ttl);

        // If still too full, remove oldest entries
        if cache.len() >= self.max_metadata_cache_size {
            let entries: Vec<_> = cache.iter().map(|(k, (_, _))| k.clone()).collect();
            let to_remove = cache.len() - self.max_metadata_cache_size + 10; // Remove extra to avoid frequent cleanup

            for key in entries.iter().take(to_remove) {
                cache.remove(key);
            }
        }
    }

    /// Clean up expired content cache entries
    async fn cleanup_expired_content(&self, cache: &mut HashMap<String, (String, Instant)>) {
        let now = Instant::now();
        cache.retain(|_, (_, loaded_at)| now.duration_since(*loaded_at) < self.content_cache_ttl);

        // If still too full, remove oldest entries
        if cache.len() >= self.max_content_cache_size {
            let entries: Vec<_> = cache.iter().map(|(k, (_, _))| k.clone()).collect();
            let to_remove = cache.len() - self.max_content_cache_size + 5; // Remove extra to avoid frequent cleanup

            for key in entries.iter().take(to_remove) {
                cache.remove(key);
            }
        }
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> CacheStats {
        let metadata_cache_size = self.metadata_cache.read().await.len();
        let content_cache_size = self.content_cache.read().await.len();

        let metadata_hits = *self.metadata_cache_hits.read().await;
        let metadata_misses = *self.metadata_cache_misses.read().await;
        let content_hits = *self.content_cache_hits.read().await;
        let content_misses = *self.content_cache_misses.read().await;

        let metadata_total = metadata_hits + metadata_misses;
        let content_total = content_hits + content_misses;

        let metadata_hit_rate = if metadata_total > 0 {
            metadata_hits as f64 / metadata_total as f64
        } else {
            0.0
        };

        let content_hit_rate = if content_total > 0 {
            content_hits as f64 / content_total as f64
        } else {
            0.0
        };

        // Rough memory estimation (simplified)
        let memory_usage_mb =
            (metadata_cache_size * 1024 + content_cache_size * 2048) as f64 / (1024.0 * 1024.0);

        CacheStats {
            metadata_cache_size,
            content_cache_size,
            metadata_hit_rate,
            content_hit_rate,
            memory_usage_mb,
            avg_metadata_load_time_ms: 50.0, // Placeholder - would track actual times
            avg_content_load_time_ms: 100.0, // Placeholder - would track actual times
        }
    }

    /// Clear all caches
    pub async fn clear_cache(&self) {
        self.metadata_cache.write().await.clear();
        self.content_cache.write().await.clear();

        // Reset counters
        *self.metadata_cache_hits.write().await = 0;
        *self.metadata_cache_misses.write().await = 0;
        *self.content_cache_hits.write().await = 0;
        *self.content_cache_misses.write().await = 0;
    }
}

#[async_trait]
impl ProgressiveLoadingService for ProgressiveLoadingServiceImpl {
    /// Load metadata for multiple skills (in-memory)
    async fn load_metadata(
        &self,
        skill_ids: &[String],
    ) -> Result<Vec<SkillMetadata>, ServiceError> {
        let mut results = Vec::new();

        for skill_id in skill_ids {
            match self.load_skill_metadata(skill_id).await {
                Ok(metadata) => results.push(metadata),
                Err(e) => {
                    warn!("Failed to load metadata for skill {}: {}", skill_id, e);
                    // Continue with other skills instead of failing completely
                }
            }
        }

        Ok(results)
    }

    /// Load complete skill content based on context
    async fn load_skill_content(
        &self,
        skill_ids: &[String],
        context: Option<LoadingContext>,
    ) -> Result<Vec<LoadedSkill>, ServiceError> {
        let mut results = Vec::new();

        for skill_id in skill_ids {
            // Always load metadata first
            let metadata = self.load_skill_metadata(skill_id).await?;

            // Determine load level based on context
            let load_level = self.determine_load_level(&metadata, context.as_ref());

            // Load content based on determined level
            let (content, references, execution_ready) = match load_level {
                LoadLevel::Metadata => (None, None, false),
                LoadLevel::Content => {
                    // Load SKILL.md content
                    let skill_content = self.load_skill_content_internal(skill_id).await?;
                    (Some(skill_content), None, true)
                }
                LoadLevel::References => {
                    // Load SKILL.md content and reference files
                    let skill_content = self.load_skill_content_internal(skill_id).await?;
                    let skill_references = self.load_reference_files(skill_id).await?;

                    (Some(skill_content), Some(skill_references), true)
                }
            };

            results.push(LoadedSkill {
                metadata,
                content,
                references,
                script_contents: None, // Scripts loaded from disk when needed
                asset_paths: self.get_asset_paths(skill_id),
                execution_ready,
                loaded_at: Instant::now(),
                load_level,
            });
        }

        Ok(results)
    }

    /// Load reference content for a specific file
    async fn load_reference_content(
        &self,
        skill_id: &str,
        reference_path: &str,
    ) -> Result<String, ServiceError> {
        let ref_path = self.skills_base_path.join(skill_id).join("references").join(reference_path);

        if !ref_path.exists() {
            return Err(ServiceError::Custom(format!(
                "Reference file not found: {}",
                ref_path.display()
            )));
        }

        tokio::fs::read_to_string(&ref_path)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read reference file: {}", e)))
    }

    /// Preload relevant skills based on context
    async fn preload_relevant_skills(
        &self,
        _context: LoadingContext,
    ) -> Result<Vec<PreloadedSkill>, ServiceError> {
        // This would integrate with the routing service to find relevant skills
        // For now, return empty list as routing service isn't implemented yet
        Ok(Vec::new())
    }

    /// Optimize loading strategy based on context
    async fn optimize_loading_strategy(
        &self,
        context: LoadingContext,
    ) -> Result<LoadingStrategy, ServiceError> {
        // Simple strategy for now - prioritize by query relevance
        // In a real implementation, this would use the routing service

        let mut strategy = LoadingStrategy {
            priority: Vec::new(),
            load_levels: HashMap::new(),
            estimated_tokens: 0,
            reasoning: vec!["Basic strategy based on query analysis".to_string()],
        };

        // For now, just set default strategy
        strategy.load_levels.insert("default".to_string(), LoadLevel::Metadata);
        strategy.estimated_tokens = context.available_tokens / 2; // Use half available tokens

        Ok(strategy)
    }

    /// Clear all caches
    async fn clear_cache(&self) -> Result<(), ServiceError> {
        self.clear_cache().await;
        Ok(())
    }

    /// Get cache statistics
    async fn get_cache_stats(&self) -> Result<CacheStats, ServiceError> {
        Ok(self.get_cache_stats().await)
    }
}

/// Helper methods for loading service
impl ProgressiveLoadingServiceImpl {
    /// Determine appropriate load level for a skill based on context
    fn determine_load_level(
        &self,
        _metadata: &SkillMetadata,
        context: Option<&LoadingContext>,
    ) -> LoadLevel {
        match context {
            Some(ctx) => {
                match ctx.urgency {
                    LoadingUrgency::Critical => LoadLevel::References,
                    LoadingUrgency::High => LoadLevel::Content,
                    LoadingUrgency::Medium => {
                        // Check if query seems to need full content
                        if ctx.query.len() > 100 || ctx.available_tokens > 2000 {
                            LoadLevel::Content
                        } else {
                            LoadLevel::Metadata
                        }
                    }
                    LoadingUrgency::Low => LoadLevel::Metadata,
                }
            }
            None => LoadLevel::Metadata, // Default to metadata only
        }
    }

    /// Load reference files for a skill
    async fn load_reference_files(
        &self,
        skill_id: &str,
    ) -> Result<HashMap<String, String>, ServiceError> {
        let references_dir = self.skills_base_path.join(skill_id).join("references");

        if !references_dir.exists() {
            return Ok(HashMap::new());
        }

        let mut references = HashMap::new();
        let mut read_dir = tokio::fs::read_dir(&references_dir).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            if entry.path().is_file() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                let content = tokio::fs::read_to_string(entry.path()).await?;

                references.insert(file_name, content);
            }
        }

        Ok(references)
    }

    /// Get asset file paths for a skill (never load content)
    fn get_asset_paths(&self, skill_id: &str) -> Vec<PathBuf> {
        let assets_dir = self.skills_base_path.join(skill_id).join("assets");

        if !assets_dir.exists() {
            return Vec::new();
        }

        // Return paths without loading content
        // In a real implementation, you'd scan the directory
        Vec::new() // Placeholder
    }
}

/// Legacy alias for backward compatibility
pub struct LoadingService(ProgressiveLoadingServiceImpl);

impl Default for LoadingService {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadingService {
    pub fn new() -> Self {
        Self(ProgressiveLoadingServiceImpl::new(PathBuf::from(
            "./skills",
        )))
    }
}

#[async_trait]
impl ProgressiveLoadingService for LoadingService {
    async fn load_metadata(
        &self,
        skill_ids: &[String],
    ) -> Result<Vec<SkillMetadata>, ServiceError> {
        self.0.load_metadata(skill_ids).await
    }

    async fn load_skill_content(
        &self,
        skill_ids: &[String],
        context: Option<LoadingContext>,
    ) -> Result<Vec<LoadedSkill>, ServiceError> {
        self.0.load_skill_content(skill_ids, context).await
    }

    async fn load_reference_content(
        &self,
        skill_id: &str,
        reference_path: &str,
    ) -> Result<String, ServiceError> {
        self.0.load_reference_content(skill_id, reference_path).await
    }

    async fn preload_relevant_skills(
        &self,
        context: LoadingContext,
    ) -> Result<Vec<PreloadedSkill>, ServiceError> {
        self.0.preload_relevant_skills(context).await
    }

    async fn optimize_loading_strategy(
        &self,
        context: LoadingContext,
    ) -> Result<LoadingStrategy, ServiceError> {
        self.0.optimize_loading_strategy(context).await
    }

    async fn clear_cache(&self) -> Result<(), ServiceError> {
        self.0.clear_cache().await;
        Ok(())
    }

    async fn get_cache_stats(&self) -> Result<CacheStats, ServiceError> {
        Ok(self.0.get_cache_stats().await)
    }
}

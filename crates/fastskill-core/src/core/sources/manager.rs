//! SourcesManager implementation.

use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::local::scan_local_source;
use super::marketplace::{
    CachedMarketplace, ClaudeCodeMarketplaceJson, MarketplaceJson, MarketplaceSkill,
};
use super::model::{SkillInfo, SourceConfig, SourceDefinition, SourcesConfig};
use super::SourcesError;

/// Sources manager for handling multiple sources
pub struct SourcesManager {
    pub(crate) config_path: PathBuf,
    sources: HashMap<String, SourceDefinition>,
    marketplace_cache: Arc<RwLock<HashMap<String, CachedMarketplace>>>,
    cache_ttl_seconds: u64,
}

impl SourcesManager {
    /// Create a new sources manager
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            sources: HashMap::new(),
            marketplace_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl_seconds: 300, // 5 minutes default TTL
        }
    }

    /// Create a new sources manager with custom cache TTL
    pub fn with_cache_ttl(config_path: PathBuf, cache_ttl_seconds: u64) -> Self {
        Self {
            config_path,
            sources: HashMap::new(),
            marketplace_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl_seconds,
        }
    }

    /// Load sources from TOML file
    pub fn load(&mut self) -> Result<(), SourcesError> {
        if !self.config_path.exists() {
            // Create default empty config if file doesn't exist
            let config = SourcesConfig {
                sources: Vec::new(),
            };
            config.save_to_file(&self.config_path)?;
            self.sources = HashMap::new();
            return Ok(());
        }

        let config = SourcesConfig::load_from_file(&self.config_path)?;
        // Sort sources by priority (lower number = higher priority)
        let mut sorted_sources: Vec<SourceDefinition> = config.sources;
        sorted_sources.sort_by_key(|s| s.priority);

        self.sources = sorted_sources
            .into_iter()
            .map(|source| (source.name.clone(), source))
            .collect();

        Ok(())
    }

    /// Save sources to TOML file
    pub fn save(&self) -> Result<(), SourcesError> {
        let mut sources: Vec<SourceDefinition> = self.sources.values().cloned().collect();
        // Sort by priority before saving
        sources.sort_by_key(|s| s.priority);
        let config = SourcesConfig { sources };
        config.save_to_file(&self.config_path)?;
        Ok(())
    }

    /// Add a new source
    pub fn add_source(&mut self, name: String, config: SourceConfig) -> Result<(), SourcesError> {
        self.add_source_with_priority(name, config, 0)
    }

    /// Add a new source with priority
    pub fn add_source_with_priority(
        &mut self,
        name: String,
        config: SourceConfig,
        priority: u32,
    ) -> Result<(), SourcesError> {
        if self.sources.contains_key(&name) {
            return Err(SourcesError::AlreadyExists(name));
        }

        let definition = SourceDefinition {
            name: name.clone(),
            priority,
            source: config,
        };

        self.sources.insert(name, definition);
        Ok(())
    }

    /// Remove a source
    pub fn remove_source(&mut self, name: &str) -> Result<(), SourcesError> {
        if self.sources.remove(name).is_none() {
            return Err(SourcesError::SourceNotFound(name.to_string()));
        }
        Ok(())
    }

    /// Get a source by name
    pub fn get_source(&self, name: &str) -> Option<&SourceDefinition> {
        self.sources.get(name)
    }

    /// List all sources (sorted by priority)
    pub fn list_sources(&self) -> Vec<&SourceDefinition> {
        let mut sources: Vec<&SourceDefinition> = self.sources.values().collect();
        sources.sort_by_key(|s| s.priority);
        sources
    }

    /// Clear the marketplace cache
    pub async fn clear_cache(&self) {
        let mut cache = self.marketplace_cache.write().await;
        cache.clear();
    }

    /// Get available skills from all sources (checked in priority order)
    pub async fn get_available_skills(&self) -> Result<Vec<SkillInfo>, SourcesError> {
        let mut all_skills = Vec::new();

        // Get sources sorted by priority
        let mut sources: Vec<(&String, &SourceDefinition)> = self.sources.iter().collect();
        sources.sort_by_key(|(_, def)| def.priority);

        for (source_name, source_def) in sources {
            let skills = self.get_skills_from_source(source_name, source_def).await?;
            all_skills.extend(skills);
        }

        Ok(all_skills)
    }

    /// Get skills from a specific source
    pub async fn get_skills_from_source(
        &self,
        source_name: &str,
        source_def: &SourceDefinition,
    ) -> Result<Vec<SkillInfo>, SourcesError> {
        match &source_def.source {
            SourceConfig::Git { url, branch, .. } => {
                // Try to load marketplace.json from Git source
                // Pass branch info for proper URL construction
                self.load_marketplace_from_url_with_branch(url, branch.as_deref(), source_name)
                    .await
            }
            SourceConfig::ZipUrl { base_url, .. } => {
                // Load marketplace.json from ZipUrl source
                self.load_marketplace_from_url_with_branch(base_url, None, source_name)
                    .await
            }
            SourceConfig::Local { path } => {
                // Scan local path for skills
                scan_local_source(path, source_name).await
            }
        }
    }

    /// Convert Claude Code format to FastSkill internal format
    /// This extracts skills from plugins by resolving skill paths
    async fn convert_claude_to_fastskill_format(
        &self,
        claude_marketplace: ClaudeCodeMarketplaceJson,
        base_url: String,
        _source_name: &str,
    ) -> Result<MarketplaceJson, SourcesError> {
        let mut skills = Vec::new();
        let owner_name = claude_marketplace.owner.as_ref().map(|o| o.name.clone());
        let metadata_version = claude_marketplace
            .metadata
            .as_ref()
            .and_then(|m| m.version.clone());

        for plugin in claude_marketplace.plugins {
            let plugin_source = plugin.source.as_deref().unwrap_or("./");

            for skill_path in plugin.skills {
                // Resolve skill path relative to plugin source
                let resolved_path = if skill_path.starts_with("./") {
                    // Relative to plugin source
                    format!(
                        "{}{}",
                        plugin_source.trim_end_matches('/'),
                        &skill_path[1..]
                    )
                } else if skill_path.starts_with('/') {
                    // Absolute from repo root
                    skill_path.trim_start_matches('/').to_string()
                } else {
                    // Relative to plugin source
                    format!("{}/{}", plugin_source.trim_end_matches('/'), skill_path)
                };

                // Extract skill ID from path (use directory name or last component)
                let skill_id = resolved_path
                    .trim_end_matches('/')
                    .split('/')
                    .next_back()
                    .unwrap_or(&resolved_path)
                    .to_string();

                // Use plugin description as fallback, or metadata description
                let description = plugin
                    .description
                    .clone()
                    .or_else(|| {
                        claude_marketplace
                            .metadata
                            .as_ref()
                            .and_then(|m| m.description.clone())
                    })
                    .unwrap_or_else(|| format!("Skill from {}", plugin.name));

                // Construct download URL if base_url is provided
                let download_url = if base_url.contains("github.com")
                    && !base_url.contains("raw.githubusercontent.com")
                {
                    let repo_path = base_url
                        .trim_start_matches("https://github.com/")
                        .trim_start_matches("http://github.com/")
                        .trim_end_matches(".git")
                        .trim_end_matches('/');
                    Some(format!(
                        "https://github.com/{}/tree/main/{}",
                        repo_path, resolved_path
                    ))
                } else if !base_url.is_empty() {
                    let base = base_url.trim_end_matches('/');
                    Some(format!("{}/{}", base, resolved_path))
                } else {
                    None
                };

                skills.push(MarketplaceSkill {
                    id: skill_id.clone(),
                    name: skill_id.clone(), // Use ID as name if not available
                    description,
                    version: metadata_version
                        .clone()
                        .unwrap_or_else(|| "1.0.0".to_string()),
                    author: owner_name.clone(),
                    download_url,
                });
            }
        }

        Ok(MarketplaceJson {
            version: "1.0".to_string(),
            skills,
        })
    }

    /// Try to fetch marketplace.json from a URL
    /// Only Claude Code format is supported
    /// Tries Claude Code standard location first (.claude-plugin/marketplace.json), then root location
    async fn try_fetch_marketplace(
        &self,
        url: &str,
        base_repo_url: Option<&str>,
    ) -> Result<MarketplaceJson, SourcesError> {
        let client = reqwest::Client::new();
        let response = client.get(url).send().await.map_err(|e| {
            SourcesError::Network(format!("Failed to fetch marketplace.json: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(SourcesError::Network(format!(
                "Failed to fetch marketplace.json: HTTP {}",
                response.status()
            )));
        }

        // Parse as Claude Code format (only supported format)
        let claude_marketplace: ClaudeCodeMarketplaceJson = response.json().await.map_err(|e| {
            SourcesError::Parse(format!(
                "Failed to parse Claude Code marketplace.json: {}",
                e
            ))
        })?;

        // Extract base repository URL for path resolution
        // If base_repo_url is provided, use it; otherwise try to extract from raw URL
        let base_url = if let Some(repo_url) = base_repo_url {
            repo_url.to_string()
        } else if url.contains("raw.githubusercontent.com") {
            // Extract repo path from raw.githubusercontent.com URL
            // e.g., https://raw.githubusercontent.com/owner/repo/branch/.claude-plugin/marketplace.json
            // -> https://github.com/owner/repo
            let parts: Vec<&str> = url.split('/').collect();
            if parts.len() >= 5 {
                let owner = parts[3];
                let repo = parts[4];
                format!("https://github.com/{}/{}", owner, repo)
            } else {
                String::new()
            }
        } else {
            // For other URLs, extract base by removing filename
            if let Some(pos) = url.rfind('/') {
                url[..pos].to_string()
            } else {
                url.to_string()
            }
        };

        // Convert Claude Code format to FastSkill internal format
        let marketplace = self
            .convert_claude_to_fastskill_format(claude_marketplace, base_url, "")
            .await?;

        // Validate marketplace.json structure
        for skill in &marketplace.skills {
            if skill.id.is_empty() || skill.name.is_empty() || skill.description.is_empty() {
                return Err(SourcesError::Parse(
                    "Invalid marketplace.json: skills must have id, name, and description"
                        .to_string(),
                ));
            }
        }

        Ok(marketplace)
    }

    /// Convert a `github.com` repo URL to a `raw.githubusercontent.com` content URL.
    ///
    /// Both callers that construct marketplace.json URLs use this helper, eliminating
    /// the repeated `.contains("github.com") && !.contains("raw.githubusercontent.com")` idiom.
    fn to_github_raw_url(base_url: &str, branch: &str, path: &str) -> String {
        if base_url.contains("github.com") && !base_url.contains("raw.githubusercontent.com") {
            let repo_path = base_url
                .trim_start_matches("https://github.com/")
                .trim_start_matches("http://github.com/")
                .trim_end_matches(".git")
                .trim_end_matches('/');
            format!(
                "https://raw.githubusercontent.com/{}/{}/{}",
                repo_path, branch, path
            )
        } else {
            let base = if base_url.ends_with('/') {
                base_url.to_string()
            } else {
                format!("{}/", base_url)
            };
            format!("{}{}", base, path)
        }
    }

    /// Check cache, fetch from the network if stale, update cache, and return the marketplace.
    ///
    /// This is the single place where cache reads, TTL checks, HTTP calls, and cache writes live.
    async fn fetch_and_cache_marketplace(
        &self,
        claude_plugin_url: &str,
        root_url: &str,
        base_url: &str,
    ) -> Result<MarketplaceJson, SourcesError> {
        // Check cache first (try both URLs)
        {
            let cache = self.marketplace_cache.read().await;
            if let Some(cached) = cache.get(claude_plugin_url) {
                if !cached.is_expired() {
                    return Ok(cached.data.clone());
                }
            }
            if let Some(cached) = cache.get(root_url) {
                if !cached.is_expired() {
                    return Ok(cached.data.clone());
                }
            }
        }

        // Try Claude Code standard location first, fall back to root
        let (marketplace, successful_url) = match self
            .try_fetch_marketplace(claude_plugin_url, Some(base_url))
            .await
        {
            Ok(m) => {
                tracing::debug!(
                    "Loaded marketplace.json from Claude Code standard location: {}",
                    claude_plugin_url
                );
                (m, claude_plugin_url.to_string())
            }
            Err(e) => {
                tracing::debug!(
                    "Claude Code location failed ({}), trying root location: {}",
                    e,
                    root_url
                );
                match self.try_fetch_marketplace(root_url, Some(base_url)).await {
                    Ok(m) => {
                        tracing::debug!("Loaded marketplace.json from root location: {}", root_url);
                        (m, root_url.to_string())
                    }
                    Err(e2) => {
                        return Err(SourcesError::Network(format!(
                            "Failed to fetch marketplace.json from both locations. Claude Code location (.claude-plugin/marketplace.json): {}. Root location (marketplace.json): {}",
                            e, e2
                        )));
                    }
                }
            }
        };

        {
            let mut cache = self.marketplace_cache.write().await;
            cache.insert(
                successful_url,
                CachedMarketplace {
                    data: marketplace.clone(),
                    fetched_at: Utc::now(),
                    ttl_seconds: self.cache_ttl_seconds,
                },
            );
        }

        Ok(marketplace)
    }

    /// Load marketplace.json from a URL.
    /// Tries Claude Code standard location (.claude-plugin/marketplace.json) first, then root.
    async fn load_marketplace_from_url_with_branch(
        &self,
        base_url: &str,
        branch: Option<&str>,
        source_name: &str,
    ) -> Result<Vec<SkillInfo>, SourcesError> {
        let branch_name = branch.unwrap_or("main");
        let claude_plugin_url =
            Self::to_github_raw_url(base_url, branch_name, ".claude-plugin/marketplace.json");
        let root_url = Self::to_github_raw_url(base_url, branch_name, "marketplace.json");

        let marketplace = self
            .fetch_and_cache_marketplace(&claude_plugin_url, &root_url, base_url)
            .await?;

        Ok(marketplace
            .skills
            .iter()
            .map(|skill| SkillInfo {
                id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                version: Some(skill.version.clone()),
                source_name: source_name.to_string(),
            })
            .collect())
    }

    /// Get marketplace.json for a specific source.
    /// Tries Claude Code standard location (.claude-plugin/marketplace.json) first, then root.
    pub async fn get_marketplace_json(
        &self,
        source_name: &str,
    ) -> Result<MarketplaceJson, SourcesError> {
        let source_def = self
            .sources
            .get(source_name)
            .ok_or_else(|| SourcesError::SourceNotFound(source_name.to_string()))?;

        let (base_url, branch) = match &source_def.source {
            SourceConfig::Git { url, branch, .. } => {
                (url.as_str(), branch.as_deref().unwrap_or("main"))
            }
            SourceConfig::ZipUrl { base_url, .. } => (base_url.as_str(), ""),
            SourceConfig::Local { .. } => {
                return Err(SourcesError::Network(
                    "Local sources do not support marketplace.json".to_string(),
                ));
            }
        };

        let claude_plugin_url =
            Self::to_github_raw_url(base_url, branch, ".claude-plugin/marketplace.json");
        let root_url = Self::to_github_raw_url(base_url, branch, "marketplace.json");

        self.fetch_and_cache_marketplace(&claude_plugin_url, &root_url, base_url)
            .await
    }
}

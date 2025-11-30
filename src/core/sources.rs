//! Sources system for managing skill repositories

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main sources configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesConfig {
    #[serde(default)]
    pub sources: Vec<SourceDefinition>,
}

/// Source definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDefinition {
    pub name: String,
    #[serde(default = "default_priority")]
    pub priority: u32,
    pub source: SourceConfig,
}

impl SourceDefinition {
    /// Check if this source supports listing skills
    /// Returns true for git-marketplace, zip-url, and local sources
    /// Returns false for HTTP registries (http-registry)
    pub fn supports_listing(&self) -> bool {
        matches!(
            &self.source,
            SourceConfig::Git { .. } | SourceConfig::ZipUrl { .. } | SourceConfig::Local { .. }
        )
    }
}

/// Default priority value (0 = highest priority)
fn default_priority() -> u32 {
    0
}

/// Source authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SourceAuth {
    #[serde(rename = "pat")]
    Pat { env_var: String },
    #[serde(rename = "ssh-key")]
    SshKey { path: PathBuf },
    #[serde(rename = "basic")]
    Basic {
        username: String,
        password_env: String,
    },
}

/// Source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SourceConfig {
    #[serde(rename = "git")]
    Git {
        url: String,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        tag: Option<String>,
        #[serde(default)]
        auth: Option<SourceAuth>,
    },
    #[serde(rename = "zip-url")]
    ZipUrl {
        base_url: String,
        #[serde(default)]
        auth: Option<SourceAuth>,
    },
    #[serde(rename = "local")]
    Local { path: PathBuf },
}

/// Information about a skill available from a source
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub source_name: String,
}

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
    pub tags: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
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
struct CachedMarketplace {
    data: MarketplaceJson,
    fetched_at: DateTime<Utc>,
    ttl_seconds: u64,
}

impl CachedMarketplace {
    fn is_expired(&self) -> bool {
        let now = Utc::now();
        let elapsed = (now - self.fetched_at).num_seconds() as u64;
        elapsed > self.ttl_seconds
    }
}

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
                self.scan_local_source(path, source_name).await
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
                    tags: vec![plugin.name.clone()], // Use plugin name as tag
                    capabilities: Vec::new(),
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

    /// Load marketplace.json from a URL
    /// Tries Claude Code standard location (.claude-plugin/marketplace.json) first, then root (marketplace.json)
    async fn load_marketplace_from_url_with_branch(
        &self,
        base_url: &str,
        branch: Option<&str>,
        source_name: &str,
    ) -> Result<Vec<SkillInfo>, SourcesError> {
        // Construct marketplace.json URLs (try both locations)
        // Priority 1: Claude Code standard location (.claude-plugin/marketplace.json)
        // Priority 2: Root location (marketplace.json)
        let branch_name = branch.unwrap_or("main");
        let (claude_plugin_url, root_url) =
            if base_url.contains("github.com") && !base_url.contains("raw.githubusercontent.com") {
                // Convert GitHub URL to raw content URL
                let repo_path = base_url
                    .trim_start_matches("https://github.com/")
                    .trim_start_matches("http://github.com/")
                    .trim_end_matches(".git")
                    .trim_end_matches('/');
                (
                    format!(
                        "https://raw.githubusercontent.com/{}/{}/.claude-plugin/marketplace.json",
                        repo_path, branch_name
                    ),
                    format!(
                        "https://raw.githubusercontent.com/{}/{}/marketplace.json",
                        repo_path, branch_name
                    ),
                )
            } else {
                let base = if base_url.ends_with('/') {
                    base_url.to_string()
                } else {
                    format!("{}/", base_url)
                };
                (
                    format!("{}.claude-plugin/marketplace.json", base),
                    format!("{}marketplace.json", base),
                )
            };

        // Check cache first (try both URLs)
        let cache_key = claude_plugin_url.clone();
        {
            let cache = self.marketplace_cache.read().await;
            if let Some(cached) = cache.get(&cache_key) {
                if !cached.is_expired() {
                    return Ok(cached
                        .data
                        .skills
                        .iter()
                        .map(|skill| SkillInfo {
                            id: skill.id.clone(),
                            name: skill.name.clone(),
                            description: skill.description.clone(),
                            version: Some(skill.version.clone()),
                            source_name: source_name.to_string(),
                        })
                        .collect());
                }
            }
            // Also check root URL cache
            if let Some(cached) = cache.get(&root_url) {
                if !cached.is_expired() {
                    return Ok(cached
                        .data
                        .skills
                        .iter()
                        .map(|skill| SkillInfo {
                            id: skill.id.clone(),
                            name: skill.name.clone(),
                            description: skill.description.clone(),
                            version: Some(skill.version.clone()),
                            source_name: source_name.to_string(),
                        })
                        .collect());
                }
            }
        }

        // Try Claude Code standard location first
        let (marketplace, successful_url) = match self
            .try_fetch_marketplace(&claude_plugin_url, Some(base_url))
            .await
        {
            Ok(m) => {
                tracing::debug!(
                    "Loaded marketplace.json from Claude Code standard location: {}",
                    claude_plugin_url
                );
                (m, claude_plugin_url.clone())
            }
            Err(e) => {
                // Fall back to root location
                tracing::debug!(
                    "Claude Code location failed ({}), trying root location: {}",
                    e,
                    root_url
                );
                match self.try_fetch_marketplace(&root_url, Some(base_url)).await {
                    Ok(m) => {
                        tracing::debug!("Loaded marketplace.json from root location: {}", root_url);
                        (m, root_url.clone())
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
                successful_url.clone(),
                CachedMarketplace {
                    data: marketplace.clone(),
                    fetched_at: Utc::now(),
                    ttl_seconds: self.cache_ttl_seconds,
                },
            );
        }

        // Convert to SkillInfo
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

    /// Get marketplace.json for a specific source
    /// Tries Claude Code standard location (.claude-plugin/marketplace.json) first, then root (marketplace.json)
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

        // Construct marketplace.json URLs (try both locations)
        // Priority 1: Claude Code standard location (.claude-plugin/marketplace.json)
        // Priority 2: Root location (marketplace.json)
        let (claude_plugin_url, root_url) =
            if base_url.contains("github.com") && !base_url.contains("raw.githubusercontent.com") {
                // Convert GitHub URL to raw content URL
                let repo_path = base_url
                    .trim_start_matches("https://github.com/")
                    .trim_start_matches("http://github.com/")
                    .trim_end_matches(".git")
                    .trim_end_matches('/');
                (
                    format!(
                        "https://raw.githubusercontent.com/{}/{}/.claude-plugin/marketplace.json",
                        repo_path, branch
                    ),
                    format!(
                        "https://raw.githubusercontent.com/{}/{}/marketplace.json",
                        repo_path, branch
                    ),
                )
            } else {
                let base = if base_url.ends_with('/') {
                    base_url.to_string()
                } else {
                    format!("{}/", base_url)
                };
                (
                    format!("{}.claude-plugin/marketplace.json", base),
                    format!("{}marketplace.json", base),
                )
            };

        // Check cache first (try both URLs)
        {
            let cache = self.marketplace_cache.read().await;
            if let Some(cached) = cache.get(&claude_plugin_url) {
                if !cached.is_expired() {
                    return Ok(cached.data.clone());
                }
            }
            // Also check root URL cache
            if let Some(cached) = cache.get(&root_url) {
                if !cached.is_expired() {
                    return Ok(cached.data.clone());
                }
            }
        }

        // Try Claude Code standard location first
        let (marketplace, successful_url) = match self
            .try_fetch_marketplace(&claude_plugin_url, Some(base_url))
            .await
        {
            Ok(m) => {
                tracing::debug!(
                    "Loaded marketplace.json from Claude Code standard location: {}",
                    claude_plugin_url
                );
                (m, claude_plugin_url.clone())
            }
            Err(e) => {
                // Fall back to root location
                tracing::debug!(
                    "Claude Code location failed ({}), trying root location: {}",
                    e,
                    root_url
                );
                match self.try_fetch_marketplace(&root_url, Some(base_url)).await {
                    Ok(m) => {
                        tracing::debug!("Loaded marketplace.json from root location: {}", root_url);
                        (m, root_url.clone())
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

        // Cache the result (use the URL that succeeded)
        {
            let mut cache = self.marketplace_cache.write().await;
            cache.insert(
                successful_url.clone(),
                CachedMarketplace {
                    data: marketplace.clone(),
                    fetched_at: Utc::now(),
                    ttl_seconds: self.cache_ttl_seconds,
                },
            );
        }

        Ok(marketplace)
    }

    /// Scan a local directory for skills
    async fn scan_local_source(
        &self,
        path: &PathBuf,
        source_name: &str,
    ) -> Result<Vec<SkillInfo>, SourcesError> {
        use walkdir::WalkDir;

        let resolved_path = if path.is_absolute() {
            path.clone()
        } else {
            // Resolve relative to current directory
            std::env::current_dir()
                .map_err(SourcesError::Io)?
                .join(path)
        };

        if !resolved_path.exists() {
            return Err(SourcesError::NotFound(resolved_path));
        }

        if !resolved_path.is_dir() {
            return Err(SourcesError::Io(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                format!("Path is not a directory: {}", resolved_path.display()),
            )));
        }

        let mut skills = Vec::new();

        // Walk directory recursively
        for entry in WalkDir::new(&resolved_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let entry_path = entry.path();
            if entry_path.is_file()
                && entry_path.file_name() == Some(std::ffi::OsStr::new("SKILL.md"))
            {
                // Found a skill directory
                if let Some(skill_dir) = entry_path.parent() {
                    // Try to extract skill metadata
                    if let Ok(skill_info) =
                        self.extract_skill_info_from_path(skill_dir, source_name)
                    {
                        skills.push(skill_info);
                    }
                }
            }
        }

        Ok(skills)
    }

    /// Extract skill information from a local path
    fn extract_skill_info_from_path(
        &self,
        skill_path: &Path,
        source_name: &str,
    ) -> Result<SkillInfo, SourcesError> {
        use std::fs;

        let skill_file = skill_path.join("SKILL.md");
        if !skill_file.exists() {
            return Err(SourcesError::NotFound(skill_file));
        }

        // Read SKILL.md to extract metadata
        let content = fs::read_to_string(&skill_file).map_err(SourcesError::Io)?;

        // Extract frontmatter (simple YAML frontmatter parser)
        let (id, name, description, version) =
            self.parse_skill_frontmatter(&content, skill_path)?;

        Ok(SkillInfo {
            id,
            name,
            description,
            version: Some(version),
            source_name: source_name.to_string(),
        })
    }

    /// Parse YAML frontmatter from SKILL.md
    fn parse_skill_frontmatter(
        &self,
        content: &str,
        skill_path: &Path,
    ) -> Result<(String, String, String, String), SourcesError> {
        // Simple frontmatter parser - look for --- delimited YAML
        if !content.starts_with("---\n") {
            // No frontmatter, use directory name as fallback
            let id = skill_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            return Ok((
                id.clone(),
                id.clone(),
                "No description".to_string(),
                "1.0.0".to_string(),
            ));
        }

        // Find end of frontmatter
        let end_marker = content[4..]
            .find("---\n")
            .ok_or_else(|| SourcesError::Parse("Invalid frontmatter format".to_string()))?;

        let frontmatter = &content[4..end_marker + 4];

        // Simple YAML parsing - just extract key fields
        let mut id = None;
        let mut name = None;
        let mut description = None;
        let mut version = None;

        for line in frontmatter.lines() {
            let line = line.trim();
            if line.starts_with("id:") {
                id = line
                    .split(':')
                    .nth(1)
                    .map(|s| s.trim().trim_matches('"').to_string());
            } else if line.starts_with("name:") {
                name = line
                    .split(':')
                    .nth(1)
                    .map(|s| s.trim().trim_matches('"').to_string());
            } else if line.starts_with("description:") {
                description = line
                    .split(':')
                    .nth(1)
                    .map(|s| s.trim().trim_matches('"').to_string());
            } else if line.starts_with("version:") {
                version = line
                    .split(':')
                    .nth(1)
                    .map(|s| s.trim().trim_matches('"').to_string());
            }
        }

        // Use directory name as fallback for id
        let skill_id = id.unwrap_or_else(|| {
            skill_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        Ok((
            skill_id.clone(),
            name.unwrap_or(skill_id.clone()),
            description.unwrap_or_else(|| "No description".to_string()),
            version.unwrap_or_else(|| "1.0.0".to_string()),
        ))
    }
}

impl SourcesConfig {
    /// Load sources config from TOML file
    pub fn load_from_file(path: &Path) -> Result<Self, SourcesError> {
        if !path.exists() {
            return Err(SourcesError::NotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path).map_err(SourcesError::Io)?;

        let config: SourcesConfig =
            toml::from_str(&content).map_err(|e| SourcesError::Parse(e.to_string()))?;

        Ok(config)
    }

    /// Save sources config to TOML file
    pub fn save_to_file(&self, path: &Path) -> Result<(), SourcesError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| SourcesError::Serialize(e.to_string()))?;

        std::fs::write(path, content).map_err(SourcesError::Io)?;

        Ok(())
    }
}

/// Sources-related errors
#[derive(Debug, thiserror::Error)]
pub enum SourcesError {
    #[error("Sources config file not found: {0}")]
    NotFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Serialize error: {0}")]
    Serialize(String),

    #[error("Source already exists: {0}")]
    AlreadyExists(String),

    #[error("Source not found: {0}")]
    SourceNotFound(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Git error: {0}")]
    Git(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_sources_config_parsing() {
        let toml_content = r#"
            [[sources]]
            name = "team-tools"
            source = { type = "git", url = "https://github.com/org/team-plugins.git", branch = "main" }

            [[sources]]
            name = "official-skills"
            source = { type = "zip-url", base_url = "https://skills.example.com/" }

            [[sources]]
            name = "local"
            source = { type = "local", path = "./local-sources" }
        "#;

        let config: SourcesConfig = toml::from_str(toml_content).unwrap();

        assert_eq!(config.sources.len(), 3);
        assert_eq!(config.sources[0].name, "team-tools");
        assert_eq!(config.sources[1].name, "official-skills");
        assert_eq!(config.sources[2].name, "local");
    }

    #[test]
    fn test_source_config_variants() {
        // Test Git source
        let git_config = SourceConfig::Git {
            url: "https://github.com/org/repo.git".to_string(),
            branch: Some("main".to_string()),
            tag: None,
            auth: None,
        };

        // Test ZipUrl source
        let zip_config = SourceConfig::ZipUrl {
            base_url: "https://skills.example.com/".to_string(),
            auth: None,
        };

        // Test Local source
        let local_config = SourceConfig::Local {
            path: PathBuf::from("./local-sources"),
        };

        // Verify they serialize correctly
        let git_toml = toml::to_string(&git_config).unwrap();
        assert!(git_toml.contains("type = \"git\""));

        let zip_toml = toml::to_string(&zip_config).unwrap();
        assert!(zip_toml.contains("type = \"zip-url\""));

        let local_toml = toml::to_string(&local_config).unwrap();
        assert!(local_toml.contains("type = \"local\""));
    }

    #[tokio::test]
    async fn test_sources_manager() {
        let temp_file = NamedTempFile::new().unwrap();
        let config_path = temp_file.path().to_path_buf();

        let mut manager = SourcesManager::new(config_path.clone());

        // Load (should create empty config)
        manager.load().unwrap();
        assert_eq!(manager.list_sources().len(), 0);

        // Add sources
        manager
            .add_source(
                "team-tools".to_string(),
                SourceConfig::Git {
                    url: "https://github.com/org/team-plugins.git".to_string(),
                    branch: Some("main".to_string()),
                    tag: None,
                    auth: None,
                },
            )
            .unwrap();

        manager
            .add_source(
                "official".to_string(),
                SourceConfig::ZipUrl {
                    base_url: "https://skills.example.com/".to_string(),
                    auth: None,
                },
            )
            .unwrap();

        assert_eq!(manager.list_sources().len(), 2);

        // Save and reload
        manager.save().unwrap();

        let mut new_manager = SourcesManager::new(config_path);
        new_manager.load().unwrap();
        assert_eq!(new_manager.list_sources().len(), 2);
    }
}

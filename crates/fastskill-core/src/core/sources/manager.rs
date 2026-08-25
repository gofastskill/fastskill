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
use super::model::{SkillInfo, SourceAuth, SourceConfig, SourceDefinition, SourcesConfig};
use super::SourcesError;
use crate::core::cache::SkillCache;

/// Reject a configured `auth` on a git source loudly rather than silently
/// ignoring it. Git sources authenticate via the system git credential
/// helper or SSH agent -- fastskill has no PAT/basic credential-injection
/// machinery for git operations. Before this check, a user who configured
/// `auth` for a git source believed their private repo was authenticated
/// when it was not: the source "worked" only when ambient git credentials
/// happened to exist, and otherwise failed with a confusing, unrelated
/// error and no hint that the `auth` block was never applied.
fn reject_configured_git_auth(
    source_name: &str,
    auth: &Option<SourceAuth>,
) -> Result<(), SourcesError> {
    if auth.is_some() {
        return Err(SourcesError::Git(format!(
            "Source '{source_name}' has `auth` configured, but git sources authenticate via \
             the system git credential helper or SSH agent, not via an `auth` block -- \
             fastskill does not inject PAT/basic credentials into git operations. Remove \
             `auth` from this source and either: (1) configure a git credential helper (e.g. \
             `git config credential.helper store`, or `gh auth login`), or (2) use an SSH \
             remote (e.g. `git@github.com:org/repo.git`) with a key loaded in your SSH agent."
        )));
    }
    Ok(())
}

/// Reject a configured `auth` on a zip-url source loudly rather than silently
/// ignoring it. Zip-url sources fetch via a plain, unauthenticated HTTP GET --
/// fastskill has no PAT/basic credential-injection machinery for that request,
/// so any `auth` block on a zip-url source is never consulted. Before this
/// check, a user who configured `auth` for a zip-url source believed their
/// private artifact was authenticated when it was not: the source "worked"
/// only when the URL happened to be publicly reachable, and otherwise failed
/// with a confusing, unrelated error and no hint that the `auth` block was
/// never applied.
fn reject_configured_zip_url_auth(
    source_name: &str,
    auth: &Option<SourceAuth>,
) -> Result<(), SourcesError> {
    if auth.is_some() {
        return Err(SourcesError::ZipUrl(format!(
            "Source '{source_name}' has `auth` configured, but zip-url sources fetch via a \
             plain HTTP GET and do not support an `auth` block -- fastskill does not inject \
             PAT/basic credentials into zip-url requests. Remove `auth` from this source and \
             use a pre-signed URL instead (e.g. an S3 or GCS presigned URL), which embeds the \
             credential in the URL itself and needs no separate `auth` configuration."
        )));
    }
    Ok(())
}

/// Number of times [`SourcesManager::try_fetch_marketplace`] actually issued
/// an HTTP request for a `marketplace.json` (either candidate location).
/// Instrumentation-only, kept for the same reason as
/// `storage::git::CLONE_INVOCATIONS` / `registry::client::DOWNLOAD_INVOCATIONS`
/// / `install::LOCAL_COPY_INVOCATIONS`: proving spec 008's "zero HTTP calls
/// on a disk-index hit" claim is otherwise only observable by timing.
/// Deliberately not gated behind `#[cfg(test)]` — integration tests are a
/// separate compilation unit and cannot see items scoped that way.
#[doc(hidden)] // test instrumentation, not supported public API
pub static MARKETPLACE_FETCH_INVOCATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Sources manager for handling multiple sources
pub struct SourcesManager {
    pub(crate) config_path: PathBuf,
    sources: HashMap<String, SourceDefinition>,
    marketplace_cache: Arc<RwLock<HashMap<String, CachedMarketplace>>>,
    cache_ttl_seconds: u64,
    /// The on-disk index cache (spec 008 FR-1/FR-2), consulted on an
    /// in-memory miss and refreshed on a successful live fetch.
    ///
    /// `None` by default (`new`/`with_cache_ttl`/`from_repositories` do not
    /// set it) so every construction site keeps today's memory-only
    /// behavior unless it opts in via [`Self::with_skill_cache`] — notably
    /// `MarketplaceRepositoryClient::new` (`repository/client.rs`), whose
    /// `SourcesManager` backs `RepositoryManager::refresh_index`'s listing
    /// call and must stay a real, unconditional network fetch (FR-4: `repos
    /// refresh`'s contract must not change). Giving that one a disk-first
    /// read-through would let a stale on-disk index silently answer what is
    /// supposed to be an explicit refresh.
    skill_cache: Option<SkillCache>,
}

impl SourcesManager {
    /// Create a new sources manager
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            sources: HashMap::new(),
            marketplace_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl_seconds: 300, // 5 minutes default TTL
            skill_cache: None,
        }
    }

    /// Create a new sources manager with custom cache TTL
    pub fn with_cache_ttl(config_path: PathBuf, cache_ttl_seconds: u64) -> Self {
        Self {
            config_path,
            sources: HashMap::new(),
            marketplace_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl_seconds,
            skill_cache: None,
        }
    }

    /// Opt this manager into the on-disk index cache (spec 008 FR-1/FR-2):
    /// `fetch_and_cache_marketplace` will consult it on an in-memory miss
    /// before going to the network, and refresh it after a successful live
    /// fetch. Without this, `SourcesManager` behaves exactly as it did
    /// before spec 008 (memory-only, always live on a miss) — see the
    /// `skill_cache` field's docs for why some construction sites must not
    /// opt in.
    pub fn with_skill_cache(mut self, skill_cache: SkillCache) -> Self {
        self.skill_cache = Some(skill_cache);
        self
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
            SourceConfig::Git {
                url, branch, auth, ..
            } => {
                reject_configured_git_auth(source_name, auth)?;

                // Try to load marketplace.json from Git source
                // Pass branch info for proper URL construction
                self.load_marketplace_from_url_with_branch(url, branch.as_deref(), source_name)
                    .await
            }
            SourceConfig::ZipUrl { base_url, auth } => {
                reject_configured_zip_url_auth(source_name, auth)?;

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
        MARKETPLACE_FETCH_INVOCATIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    ///
    /// spec 008: on an in-memory miss, the on-disk index (keyed by
    /// `source_name`) is consulted *before* the network (FR-1) — a genuine
    /// bypass, not just a fallback, so a cold process with a previously
    /// `repos refresh`-ed (or previously live-fetched) source resolves a
    /// listing with zero HTTP calls. A successful live fetch still populates
    /// the in-memory map as before, and additionally refreshes the on-disk
    /// index for `source_name` so the two layers do not drift (FR-2). If
    /// *neither* layer has anything yet, the network is attempted as a last
    /// resort; if that fails too but the on-disk index gained a usable entry
    /// in the meantime (e.g. a concurrent `repos refresh`), it is used with a
    /// warning naming when it was recorded (FR-3) — mirroring
    /// `install::resolve_git_sha` / `install::fetch_zip_url_cached`'s
    /// offline-fallback shape. `self.skill_cache` is `None` for managers that
    /// must never read the disk index this way (see its field docs), so all
    /// of the above degrades to today's memory-only behavior for them.
    async fn fetch_and_cache_marketplace(
        &self,
        source_name: &str,
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

        // FR-1: consult the on-disk index before the network.
        if let Some((marketplace, fetched_at)) = self.disk_index_marketplace(source_name).await {
            tracing::warn!(
                "using the on-disk index for source '{source_name}' (recorded {fetched_at}) \
                 instead of a live marketplace fetch; run `fastskill repos refresh {source_name}` \
                 for the latest listing"
            );
            self.cache_marketplace_in_memory(claude_plugin_url, &marketplace)
                .await;
            return Ok(marketplace);
        }

        // Try Claude Code standard location first, fall back to root
        let live_fetch = match self
            .try_fetch_marketplace(claude_plugin_url, Some(base_url))
            .await
        {
            Ok(m) => {
                tracing::debug!(
                    "Loaded marketplace.json from Claude Code standard location: {}",
                    claude_plugin_url
                );
                Ok((m, claude_plugin_url.to_string()))
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
                        Ok((m, root_url.to_string()))
                    }
                    Err(e2) => Err(SourcesError::Network(format!(
                        "Failed to fetch marketplace.json from both locations. Claude Code location (.claude-plugin/marketplace.json): {}. Root location (marketplace.json): {}",
                        e, e2
                    ))),
                }
            }
        };

        let (marketplace, successful_url) = match live_fetch {
            Ok(ok) => ok,
            Err(err) => {
                // FR-3: both live locations failed. A last-resort re-check of
                // the on-disk index (it found nothing above, but this guards
                // a concurrent `repos refresh`/live-fetch landing in
                // between) proceeds with a warning naming when it was
                // recorded, rather than surfacing the network error.
                if let Some((marketplace, fetched_at)) =
                    self.disk_index_marketplace(source_name).await
                {
                    tracing::warn!(
                        "could not fetch a live marketplace listing for source '{source_name}' \
                         ({err}); using the on-disk index recorded {fetched_at} instead"
                    );
                    self.cache_marketplace_in_memory(claude_plugin_url, &marketplace)
                        .await;
                    return Ok(marketplace);
                }
                return Err(err);
            }
        };

        self.cache_marketplace_in_memory(&successful_url, &marketplace)
            .await;

        // FR-2: refresh the on-disk index so it does not drift behind what
        // was just fetched live. Best-effort: a write failure must not fail
        // a fetch that has already succeeded.
        if let Some(skill_cache) = &self.skill_cache {
            if let Err(e) = skill_cache
                .write_source_index(source_name, &marketplace_to_source_index(&marketplace))
            {
                tracing::warn!(
                    "failed to refresh the on-disk index for source '{source_name}': {}",
                    e
                );
            }
        }

        Ok(marketplace)
    }

    /// FR-1/FR-3's shared disk-index lookup: `None` when this manager has no
    /// [`SkillCache`] configured, the index has never been written for
    /// `source_name`, or it was written with zero entries. Otherwise a best-
    /// effort [`MarketplaceJson`] reconstructed from it (see
    /// [`source_index_to_marketplace`] for exactly what is and is not
    /// recoverable from the on-disk shape), paired with the index's recorded
    /// `fetched_at` so callers can name it in their warning (FR-3).
    async fn disk_index_marketplace(
        &self,
        source_name: &str,
    ) -> Option<(MarketplaceJson, chrono::DateTime<Utc>)> {
        let skill_cache = self.skill_cache.as_ref()?;
        let idx = skill_cache.read_source_index(source_name).ok()??;
        if idx.entries.is_empty() {
            return None;
        }
        let fetched_at = idx.fetched_at;
        Some((source_index_to_marketplace(&idx), fetched_at))
    }

    /// Insert `marketplace` into the in-memory map under `key` (FR-5: this is
    /// the seam the within-operation dedup relies on — every return point in
    /// `fetch_and_cache_marketplace`, disk or network, goes through it, so a
    /// second call in the same operation always hits the in-memory check at
    /// the top rather than repeating a disk read or a fetch).
    async fn cache_marketplace_in_memory(&self, key: &str, marketplace: &MarketplaceJson) {
        let mut cache = self.marketplace_cache.write().await;
        cache.insert(
            key.to_string(),
            CachedMarketplace {
                data: marketplace.clone(),
                fetched_at: Utc::now(),
                ttl_seconds: self.cache_ttl_seconds,
            },
        );
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
            .fetch_and_cache_marketplace(source_name, &claude_plugin_url, &root_url, base_url)
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

    /// Build a SourcesManager from a RepositoryManager, converting marketplace-compatible
    /// repository definitions to source entries. Returns `None` if no eligible repos exist.
    pub fn from_repositories(
        repo_manager: &crate::core::repository::RepositoryManager,
    ) -> Result<Option<Self>, SourcesError> {
        use crate::core::repository::{RepositoryConfig, RepositoryType};

        let repos = repo_manager.list_repositories();

        let source_defs: Vec<SourceDefinition> = repos
            .into_iter()
            .filter_map(|repo| {
                let source_config = match &repo.repo_type {
                    RepositoryType::GitMarketplace => {
                        if let RepositoryConfig::GitMarketplace { url, branch, tag } = &repo.config
                        {
                            let auth = repo.auth.as_ref().map(repo_auth_to_source_auth);
                            Some(SourceConfig::Git {
                                url: url.clone(),
                                branch: branch.clone(),
                                tag: tag.clone(),
                                auth,
                            })
                        } else {
                            None
                        }
                    }
                    RepositoryType::ZipUrl => {
                        if let RepositoryConfig::ZipUrl { base_url } = &repo.config {
                            let auth = repo.auth.as_ref().map(repo_auth_to_source_auth);
                            Some(SourceConfig::ZipUrl {
                                base_url: base_url.clone(),
                                auth,
                            })
                        } else {
                            None
                        }
                    }
                    RepositoryType::Local => {
                        if let RepositoryConfig::Local { path } = &repo.config {
                            Some(SourceConfig::Local { path: path.clone() })
                        } else {
                            None
                        }
                    }
                    RepositoryType::HttpRegistry => None,
                };
                source_config.map(|source| SourceDefinition {
                    name: repo.name.clone(),
                    priority: repo.priority,
                    source,
                })
            })
            .collect();

        if source_defs.is_empty() {
            return Ok(None);
        }

        let temp_path = std::env::temp_dir().join("fastskill-sources-temp.toml");
        let mut manager = Self::new(temp_path);
        manager.sources = source_defs
            .into_iter()
            .map(|def| (def.name.clone(), def))
            .collect();

        Ok(Some(manager))
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
            SourceConfig::Git {
                url, branch, auth, ..
            } => {
                reject_configured_git_auth(source_name, auth)?;
                (url.as_str(), branch.as_deref().unwrap_or("main"))
            }
            SourceConfig::ZipUrl { base_url, auth } => {
                reject_configured_zip_url_auth(source_name, auth)?;
                (base_url.as_str(), "")
            }
            SourceConfig::Local { .. } => {
                return Err(SourcesError::Network(
                    "Local sources do not support marketplace.json".to_string(),
                ));
            }
        };

        let claude_plugin_url =
            Self::to_github_raw_url(base_url, branch, ".claude-plugin/marketplace.json");
        let root_url = Self::to_github_raw_url(base_url, branch, "marketplace.json");

        self.fetch_and_cache_marketplace(source_name, &claude_plugin_url, &root_url, base_url)
            .await
    }
}

/// FR-2: fold a freshly-fetched [`MarketplaceJson`] into the on-disk
/// [`crate::core::cache::SourceIndex`] shape — group by skill id, dedup
/// versions, keep a representative `name`/`description` per id (the first
/// skill entry seen for it). Mirrors
/// `RepositoryManager::refresh_index`'s grouping (`repository.rs`) applied to
/// a [`MarketplaceSkill`] list instead of a `SkillMetadata` list.
fn marketplace_to_source_index(marketplace: &MarketplaceJson) -> crate::core::cache::SourceIndex {
    use std::collections::{BTreeMap, BTreeSet};

    let mut by_id: BTreeMap<String, (BTreeSet<String>, String, String)> = BTreeMap::new();
    for skill in &marketplace.skills {
        let entry = by_id.entry(skill.id.clone()).or_insert_with(|| {
            (
                BTreeSet::new(),
                skill.name.clone(),
                skill.description.clone(),
            )
        });
        entry.0.insert(skill.version.clone());
    }

    let entries = by_id
        .into_iter()
        .map(
            |(skill, (versions, name, description))| crate::core::cache::SourceIndexEntry {
                skill,
                versions: versions.into_iter().collect(),
                name,
                description,
            },
        )
        .collect();

    crate::core::cache::SourceIndex {
        fetched_at: Utc::now(),
        entries,
    }
}

/// FR-1/FR-3: the read-side inverse of [`marketplace_to_source_index`] —
/// best-effort reconstruct a [`MarketplaceJson`] from a
/// [`crate::core::cache::SourceIndex`] so a disk hit can stand in for a live
/// fetch.
///
/// **Lossy**: `SourceIndexEntry` (owned by the on-disk index schema, shared
/// with `RepositoryManager::refresh_index` and `install::resolve_repository_version`,
/// which never needed more than `skill` + `versions`) has no `author` or
/// `download_url` field — a skill reconstructed this way always gets
/// `author: None, download_url: None`. In this codebase that is a narrow,
/// display-only gap: the real skill-content fetch (`install_from_resolved_source`,
/// `install.rs`) resolves bytes from a `SourceConfig` (git/zip-url/local),
/// never from `MarketplaceSkill::download_url` — so it is never even read on
/// the path that matters for *installing*. It does reach two read-only HTTP
/// registry endpoints (`GET /api/v1/registry/skills`,
/// `.../sources/:name/skills`) that surface `author`/`download_url` for
/// display; those fields render blank when a listing came from the disk
/// fallback instead of a live fetch, until the next live fetch or `repos
/// refresh` repopulates them with real values.
///
/// One [`MarketplaceSkill`] is emitted per recorded version, all sharing the
/// entry's representative `name`/`description` (the same "one per id, not
/// one per version" approximation `marketplace_to_source_index` already made
/// when writing the index).
fn source_index_to_marketplace(idx: &crate::core::cache::SourceIndex) -> MarketplaceJson {
    let skills = idx
        .entries
        .iter()
        .flat_map(|entry| {
            let id = entry.skill.clone();
            let name = entry.name.clone();
            let description = entry.description.clone();
            entry.versions.iter().map(move |version| MarketplaceSkill {
                id: id.clone(),
                name: name.clone(),
                description: description.clone(),
                version: version.clone(),
                author: None,
                download_url: None,
            })
        })
        .collect();

    MarketplaceJson {
        version: "1.0".to_string(),
        skills,
    }
}

/// Total: `RepositoryAuth` has exactly one variant, so every configured
/// repository auth maps to a `SourceAuth`. This previously returned `Option`
/// and answered `None` for `ApiKey`, which is how a configured credential
/// could vanish without a word.
fn repo_auth_to_source_auth(
    auth: &crate::core::repository::RepositoryAuth,
) -> super::model::SourceAuth {
    use super::model::SourceAuth;
    use crate::core::repository::RepositoryAuth;
    let RepositoryAuth::Pat { env_var } = auth;
    SourceAuth::Pat {
        env_var: env_var.clone(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn pat_auth() -> Option<SourceAuth> {
        Some(SourceAuth::Pat {
            env_var: "EXAMPLE_TOKEN".to_string(),
        })
    }

    fn zip_source_with_auth() -> SourceConfig {
        SourceConfig::ZipUrl {
            base_url: "http://127.0.0.1:1/skills.zip".to_string(),
            auth: pat_auth(),
        }
    }

    fn zip_source_without_auth() -> SourceConfig {
        SourceConfig::ZipUrl {
            base_url: "http://127.0.0.1:1/skills.zip".to_string(),
            auth: None,
        }
    }

    #[test]
    fn reject_configured_zip_url_auth_rejects_when_configured() {
        let err = reject_configured_zip_url_auth("my-zip-source", &pat_auth())
            .expect_err("configured auth on a zip-url source must be rejected");
        assert_eq!(
            err.to_string(),
            "Zip URL error: Source 'my-zip-source' has `auth` configured, but zip-url sources \
             fetch via a plain HTTP GET and do not support an `auth` block -- fastskill does \
             not inject PAT/basic credentials into zip-url requests. Remove `auth` from this \
             source and use a pre-signed URL instead (e.g. an S3 or GCS presigned URL), which \
             embeds the credential in the URL itself and needs no separate `auth` configuration."
        );
    }

    #[test]
    fn reject_configured_zip_url_auth_allows_when_absent() {
        assert!(reject_configured_zip_url_auth("my-zip-source", &None).is_ok());
    }

    /// Regression guard: this fix (the zip-url half of #273's git-auth fix)
    /// must not change #273's git rejection message by even one character --
    /// callers and tests elsewhere may assert on its exact text.
    #[test]
    fn reject_configured_git_auth_message_is_unchanged() {
        let err = reject_configured_git_auth("my-git-source", &pat_auth())
            .expect_err("configured auth on a git source must be rejected");
        assert_eq!(
            err.to_string(),
            "Git error: Source 'my-git-source' has `auth` configured, but git sources \
             authenticate via the system git credential helper or SSH agent, not via an \
             `auth` block -- fastskill does not inject PAT/basic credentials into git \
             operations. Remove `auth` from this source and either: (1) configure a git \
             credential helper (e.g. `git config credential.helper store`, or `gh auth \
             login`), or (2) use an SSH remote (e.g. `git@github.com:org/repo.git`) with a \
             key loaded in your SSH agent."
        );
    }

    #[test]
    fn reject_configured_git_auth_allows_when_absent() {
        assert!(reject_configured_git_auth("my-git-source", &None).is_ok());
    }

    /// Call site 1: `get_skills_from_source`. Configured `auth` on a
    /// zip-url source must be rejected before any network call is made --
    /// the loopback base_url would otherwise fail with a connection-refused
    /// `Network` error, not a `ZipUrl` one, so reaching the network at all
    /// here would itself be a test failure.
    #[tokio::test]
    async fn get_skills_from_source_rejects_zip_url_auth() {
        let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
        manager
            .add_source("zip-src".to_string(), zip_source_with_auth())
            .unwrap();
        let source_def = manager.get_source("zip-src").unwrap().clone();

        let err = manager
            .get_skills_from_source("zip-src", &source_def)
            .await
            .expect_err("configured auth must be rejected before any network call");
        assert!(matches!(err, SourcesError::ZipUrl(_)));
        assert!(err.to_string().contains("pre-signed URL"));
    }

    /// Call site 2: `get_marketplace_json`. Same guarantee as above, for
    /// the other function that destructures `SourceConfig::ZipUrl` -- the
    /// exact failure mode of the original bug was fixing only one of these.
    #[tokio::test]
    async fn get_marketplace_json_rejects_zip_url_auth() {
        let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
        manager
            .add_source("zip-src".to_string(), zip_source_with_auth())
            .unwrap();

        let err = manager
            .get_marketplace_json("zip-src")
            .await
            .expect_err("configured auth must be rejected before any network call");
        assert!(matches!(err, SourcesError::ZipUrl(_)));
        assert!(err.to_string().contains("pre-signed URL"));
    }

    /// A zip-url source with no `auth` configured must proceed unaffected:
    /// it should reach the network stage (and fail there, against a
    /// deliberately unreachable loopback port) rather than being rejected
    /// by the new auth gate.
    #[tokio::test]
    async fn get_skills_from_source_zip_url_without_auth_is_unaffected() {
        let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
        manager
            .add_source("zip-src".to_string(), zip_source_without_auth())
            .unwrap();
        let source_def = manager.get_source("zip-src").unwrap().clone();

        let err = manager
            .get_skills_from_source("zip-src", &source_def)
            .await
            .expect_err("unreachable loopback URL should fail at the network stage");
        assert!(
            !matches!(err, SourcesError::ZipUrl(_)),
            "unexpected auth rejection for a source with no `auth` configured: {err}"
        );
    }

    /// Same as above for the `get_marketplace_json` call site.
    #[tokio::test]
    async fn get_marketplace_json_zip_url_without_auth_is_unaffected() {
        let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
        manager
            .add_source("zip-src".to_string(), zip_source_without_auth())
            .unwrap();

        let err = manager
            .get_marketplace_json("zip-src")
            .await
            .expect_err("unreachable loopback URL should fail at the network stage");
        assert!(
            !matches!(err, SourcesError::ZipUrl(_)),
            "unexpected auth rejection for a source with no `auth` configured: {err}"
        );
    }
}

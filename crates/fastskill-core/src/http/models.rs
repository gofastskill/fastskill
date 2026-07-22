//! Request and response models for the HTTP API

use serde::{Deserialize, Serialize};
use validator::Validate;

/// Generic API response wrapper
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ErrorResponse>,
    pub meta: Option<ResponseMeta>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: None,
        }
    }

    pub fn success_with_meta(data: T, meta: ResponseMeta) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            meta: Some(meta),
        }
    }

    pub fn error(error: ErrorResponse) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
            meta: None,
        }
    }
}

/// Response metadata
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMeta {
    pub total_count: Option<i64>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub has_next: Option<bool>,
}

/// Error response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

/// Skill definition for API responses
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub metadata: serde_json::Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Skill list response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillsListResponse {
    pub skills: Vec<SkillResponse>,
    pub count: usize,
    pub total: usize,
}

/// Skill creation/update request
#[derive(Debug, Deserialize, Validate, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,

    #[validate(length(min = 1, max = 1000))]
    pub description: String,

    pub metadata: serde_json::Value,
}

/// Search request
#[derive(Debug, Deserialize, Validate, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    #[validate(length(min = 1, max = 1000))]
    pub query: String,

    #[validate(range(min = 1, max = 50))]
    pub limit: Option<i32>,

    pub semantic: Option<bool>,
}

/// Search response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub skills: Vec<SkillMatchResponse>,
    pub count: usize,
    pub query: String,
}

/// Skill match in search results
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillMatchResponse {
    pub skill: SkillResponse,
    pub score: f32,
    pub relevance: String,
}

/// Reindex request
#[derive(Debug, Deserialize, Validate, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReindexRequest {
    pub force: Option<bool>,
    pub skill_ids: Option<Vec<String>>,
}

/// Reindex response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReindexResponse {
    pub success_count: usize,
    pub error_count: usize,
    pub total_processed: usize,
    pub duration_ms: u64,
}

/// Outcome of a call into the core reindex seam (ADR-0002/0005). `reindexed:
/// false` + a `reason` means the reindex was skipped (e.g. no embedding
/// provider configured) — a success, not a failure.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReindexOutcomeResponse {
    pub reindexed: bool,
    pub count: usize,
    pub reason: Option<String>,
}

/// Status response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
    pub skills_count: usize,
    pub storage_path: String,
    pub hot_reload_enabled: bool,
    pub uptime_seconds: u64,
    /// Whether write (mutating) endpoints are enabled on this server instance.
    pub writable: bool,
    /// Whether an embedding provider is injected (reindex/semantic search
    /// available rather than skipping silently / falling back to keyword search).
    pub embedding_provider: bool,
}

/// Source response for registry
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceResponse {
    pub name: String,
    pub source_type: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub supports_marketplace: bool,
}

/// Marketplace skill response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSkillResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub download_url: Option<String>,
    pub source_name: String,
    pub installed: bool,
}

/// Source skills response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceSkillsResponse {
    pub source_name: String,
    pub skills: Vec<MarketplaceSkillResponse>,
    pub count: usize,
}

/// Registry skills response (all sources)
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySkillsResponse {
    pub sources: Vec<SourceSkillsResponse>,
    pub total_skills: usize,
    pub total_sources: usize,
}

/// Manifest skill response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSkillResponse {
    pub id: String,
    pub version: Option<String>,
    pub groups: Vec<String>,
    pub editable: bool,
    pub source_type: String,
}

/// Add skill to manifest request
#[derive(Debug, Deserialize, Validate, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AddSkillRequest {
    #[validate(length(min = 1))]
    pub skill_id: String,

    #[validate(length(min = 1))]
    pub source_name: String,

    pub groups: Option<Vec<String>>,
    pub editable: Option<bool>,
}

/// Update skill in manifest request
#[derive(Debug, Deserialize, Validate, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSkillRequest {
    pub groups: Option<Vec<String>>,
    pub editable: Option<bool>,
    pub version: Option<String>,
}

/// POST /api/v1/skills/install request body: a fresh install from an **Origin
/// ref** — a raw string (git URL, `.zip` URL, local path, or
/// `scope/skill[@version]` id) that the server classifies via the core
/// `infer_origin` seam (ADR-0005 / spec 003 Phase 3) — the UI performs no
/// detection of its own; it just sends what the user typed.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstallSkillRequest {
    pub origin: String,
    #[serde(default)]
    pub groups: Vec<String>,
}

/// POST /api/v1/skills/install response (201) / success shape.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstallSkillResponse {
    pub id: String,
    pub resolved_version: String,
    pub reindexed: bool,
}

/// POST /api/v1/skills/update request body. `skill_id` omitted (or `"all"`)
/// updates every skill recorded in the project; `check` reports the preflight
/// verdict for each without applying anything. `version` (v2, spec 003 Phase 4)
/// pins a single `repository`-origin skill to an exact version: only valid
/// together with `skill_id` and only when that skill's recorded `Origin` is
/// `Repository` (see `handlers::skills::update_skills`).
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSkillsRequest {
    pub skill_id: Option<String>,
    #[serde(default)]
    pub check: bool,
    pub version: Option<String>,
}

/// Query parameters for `GET /api/v1/skills/{id}/content`.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentQuery {
    pub format: Option<ContentFormat>,
}

/// `?format=` value for `GET /api/v1/skills/{id}/content` (spec 003 v2 / Phase
/// 4). `Raw` (the default) is today's behavior; `Html` renders the Markdown
/// server-side (`comrak`) and allowlist-sanitizes the result (`ammonia`) before
/// returning it — safe to assign to `innerHTML` (no `<script>`, no `on*`
/// handlers, no `javascript:`/`data:` URLs).
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContentFormat {
    #[default]
    Raw,
    Html,
}

impl ContentFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentFormat::Raw => "raw",
            ContentFormat::Html => "html",
        }
    }
}

/// GET /api/v1/skills/{id}/content response: the installed skill's `SKILL.md`,
/// path-confined to the skills directory (see
/// `handlers::skills::get_skill_content`). `format` echoes the resolved
/// `?format=` value (`"raw"` or `"html"`); `content` is the raw file text for
/// `raw`, or sanitized HTML for `html` (spec 003 §5 / SEC-7 — the UI still
/// HTML-escapes/renders `raw` content itself; `html` content is already safe to
/// insert directly).
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillContentResponse {
    pub path: String,
    pub format: String,
    pub content: String,
}

/// A single version available for a skill in the registry (spec 003 v2 /
/// Phase 4 version picker). `repo` is the concrete Repository/source name that
/// offers this version, when known.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

/// GET /api/v1/registry/skills/{id}/versions response. `versions` is sorted
/// descending (newest first); empty (not 404) when the registry has no
/// candidates for `id` (no registry configured, or the id is unknown there) —
/// see `handlers::registry::list_skill_versions`.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersionsResponse {
    pub id: String,
    pub versions: Vec<VersionInfo>,
}

/// Per-skill outcome of a `POST /api/v1/skills/update` call.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateResult {
    pub id: String,
    /// One of: `"updated"`, `"would_update"`, `"up_to_date"`, `"immutable"`, `"error"`.
    pub outcome: String,
    pub reason: Option<String>,
    pub resolved_version: Option<String>,
}

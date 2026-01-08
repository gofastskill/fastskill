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
}

/// JWT token request (for local development)
#[derive(Debug, Deserialize, Validate, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenRequest {
    #[validate(length(min = 1, max = 50))]
    pub role: String,

    pub username: Option<String>,
}

/// JWT token response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub role: String,
}

/// JWT claims structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,  // subject (user id)
    pub role: String, // user role
    pub exp: usize,   // expiration time
    pub iat: usize,   // issued at
    pub iss: String,  // issuer
}

/// Verify token response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub valid: bool,
    pub role: Option<String>,
    pub expires_at: Option<String>,
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

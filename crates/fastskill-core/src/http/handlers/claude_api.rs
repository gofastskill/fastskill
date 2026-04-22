//! Claude Code v1 API endpoints implementation
//!
//! This module implements the Claude Code v1 API endpoints for skill management,
//! matching Anthropic's API specification for skill creation, versioning, and management.

use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::{skill_storage::SkillStorage, AppState};
use crate::http::models::*;
use axum::{
    extract::{Multipart, Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

/// Create skill version request (multipart/form-data) - not used directly
/// We handle multipart parsing manually
/// Multipart file representation
#[derive(Debug)]
pub struct MultipartFile {
    pub filename: String,
    pub content: Vec<u8>,
    pub content_type: Option<String>,
}

/// Create skill version response
#[derive(Debug, Serialize)]
pub struct CreateSkillVersionResponse {
    pub created_at: String,
    pub description: String,
    pub directory: String,
    pub id: String,
    pub name: String,
    pub skill_id: String,
    pub r#type: String, // Always "skill_version"
    pub version: String,
}

/// List skills response
#[derive(Debug, Serialize)]
pub struct ListSkillsResponse {
    pub skills: Vec<SkillInfo>,
    pub count: usize,
    pub total: usize,
}

/// Skill information
#[derive(Debug, Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub latest_version: Option<String>,
    pub source: String, // "anthropic" or "custom"
}

/// Get skill response
#[derive(Debug, Serialize)]
pub struct GetSkillResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub latest_version: Option<String>,
    pub source: String,
}

/// Create skill response
#[derive(Debug, Serialize)]
pub struct CreateSkillResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub source: String,
}

/// Query parameters for listing skills
#[derive(Debug, Deserialize)]
pub struct ListSkillsQuery {
    pub source: Option<String>, // Filter by source: "anthropic" or "custom"
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// POST /v1/skills - Create a new skill
pub async fn create_skill(
    State(state): State<AppState>,
    Json(request): Json<CreateSkillRequest>,
) -> HttpResult<Json<ApiResponse<CreateSkillResponse>>> {
    // Validate request
    request.validate().map_err(|e| {
        HttpError::ValidationError(
            e.field_errors()
                .into_iter()
                .map(|(field, errors)| {
                    (
                        field.to_string(),
                        errors
                            .iter()
                            .map(|e| e.message.clone().unwrap_or_default().to_string())
                            .collect(),
                    )
                })
                .collect(),
        )
    })?;

    // Generate skill ID
    let skill_id = SkillStorage::generate_skill_id();

    // Create skill metadata
    let skills_dir = state.service.config().skill_storage_path.clone();
    let _skill_storage = SkillStorage::new(state.service.clone(), skills_dir);
    let created_at = chrono::Utc::now();

    // For now, create a basic skill entry
    // In a full implementation, this would create the skill in the registry
    let response = CreateSkillResponse {
        id: skill_id,
        name: request.display_title.clone(),
        description: request.description.unwrap_or_default(),
        created_at: created_at.to_rfc3339(),
        source: "custom".to_string(),
    };

    Ok(Json(ApiResponse::success(response)))
}

/// GET /v1/skills - List all skills
pub async fn list_skills(
    State(state): State<AppState>,
    Query(query): Query<ListSkillsQuery>,
) -> HttpResult<Json<ApiResponse<ListSkillsResponse>>> {
    let skills_dir = state.service.config().skill_storage_path.clone();
    let skill_storage = SkillStorage::new(state.service.clone(), skills_dir);

    // Get skills from storage
    let mut skills = skill_storage.list_skills().await?;

    // Apply source filter
    if let Some(source_filter) = &query.source {
        skills.retain(|skill| {
            // Determine source based on skill ID format or metadata
            // Anthropic skills have known IDs, custom skills have generated IDs
            let is_anthropic = matches!(skill.id.as_str(), "pptx" | "xlsx" | "docx" | "pdf");
            match source_filter.as_str() {
                "anthropic" => is_anthropic,
                "custom" => !is_anthropic,
                _ => true,
            }
        });
    }

    // Apply pagination
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(50);
    let total = skills.len();
    let paginated_skills = skills
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let count = paginated_skills.len();

    // Convert to response format
    let skill_infos: Vec<SkillInfo> = paginated_skills
        .into_iter()
        .map(|skill| {
            let is_anthropic = matches!(skill.id.as_str(), "pptx" | "xlsx" | "docx" | "pdf");
            SkillInfo {
                id: skill.id,
                name: skill.name,
                description: skill.description,
                created_at: Some(skill.created_at.to_rfc3339()),
                updated_at: Some(skill.updated_at.to_rfc3339()),
                latest_version: Some(skill.latest_version),
                source: if is_anthropic {
                    "anthropic".to_string()
                } else {
                    "custom".to_string()
                },
            }
        })
        .collect();

    let response = ListSkillsResponse {
        skills: skill_infos,
        count,
        total,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// GET /v1/skills/{skill_id} - Get skill details
pub async fn get_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> HttpResult<Json<ApiResponse<GetSkillResponse>>> {
    let skills_dir = state.service.config().skill_storage_path.clone();
    let skill_storage = SkillStorage::new(state.service.clone(), skills_dir);

    let skill = skill_storage
        .get_skill(&skill_id)
        .await?
        .ok_or_else(|| HttpError::NotFound(format!("Skill not found: {}", skill_id)))?;

    let is_anthropic = matches!(skill.id.as_str(), "pptx" | "xlsx" | "docx" | "pdf");

    let response = GetSkillResponse {
        id: skill.id,
        name: skill.name,
        description: skill.description,
        created_at: Some(skill.created_at.to_rfc3339()),
        updated_at: Some(skill.updated_at.to_rfc3339()),
        latest_version: Some(skill.latest_version),
        source: if is_anthropic {
            "anthropic".to_string()
        } else {
            "custom".to_string()
        },
    };

    Ok(Json(ApiResponse::success(response)))
}

/// DELETE /v1/skills/{skill_id} - Delete a skill
pub async fn delete_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> HttpResult<Json<ApiResponse<serde_json::Value>>> {
    let skills_dir = state.service.config().skill_storage_path.clone();
    let skill_storage = SkillStorage::new(state.service.clone(), skills_dir);

    skill_storage.delete_skill(&skill_id).await?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "message": "Skill deleted successfully"
    }))))
}

/// POST /v1/skills/{skill_id}/versions - Create skill version
pub async fn create_skill_version(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
    mut multipart: Multipart,
) -> HttpResult<Json<ApiResponse<CreateSkillVersionResponse>>> {
    let skills_dir = state.service.config().skill_storage_path.clone();
    let skill_storage = SkillStorage::new(state.service.clone(), skills_dir);

    // Parse multipart form data
    let mut files = HashMap::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| HttpError::BadRequest(format!("Failed to read multipart field: {}", e)))?
    {
        let name = field.name().unwrap_or("unnamed").to_string();
        if name == "files" || name == "files[]" {
            let filename = field.file_name().unwrap_or("unnamed").to_string();
            let content = field.bytes().await.map_err(|e| {
                HttpError::BadRequest(format!("Failed to read file content: {}", e))
            })?;
            files.insert(filename, content.to_vec());
        }
    }

    if files.is_empty() {
        return Err(HttpError::BadRequest("No files provided".to_string()));
    }

    // Store the skill version
    let version = skill_storage.store_skill_version(&skill_id, files).await?;

    let response = CreateSkillVersionResponse {
        created_at: version.created_at.to_rfc3339(),
        description: version.description,
        directory: version.directory,
        id: version.id,
        name: version.name,
        skill_id: version.skill_id,
        r#type: "skill_version".to_string(),
        version: version.version,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// GET /v1/skills/{skill_id}/versions - List skill versions
pub async fn list_skill_versions(
    State(_state): State<AppState>,
    Path(_skill_id): Path<String>,
) -> HttpResult<Json<ApiResponse<SkillVersionsListResponse>>> {
    // For now, return empty list - full implementation would track versions
    let response = SkillVersionsListResponse {
        versions: vec![],
        count: 0,
        total: 0,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// GET /v1/skills/{skill_id}/versions/{version} - Get skill version
pub async fn get_skill_version(
    State(_state): State<AppState>,
    Path((_skill_id, _version)): Path<(String, String)>,
) -> HttpResult<Json<ApiResponse<SkillVersionResponse>>> {
    // This would need to be implemented to retrieve specific versions
    Err(HttpError::NotFound(format!(
        "Skill version not found: {}@{}",
        _skill_id, _version
    )))
}

/// DELETE /v1/skills/{skill_id}/versions/{version} - Delete skill version
pub async fn delete_skill_version(
    State(_state): State<AppState>,
    Path((_skill_id, _version)): Path<(String, String)>,
) -> HttpResult<Json<ApiResponse<serde_json::Value>>> {
    // This would need to be implemented to delete specific versions
    Err(HttpError::NotFound(format!(
        "Skill version not found: {}@{}",
        _skill_id, _version
    )))
}

// Additional response types
#[derive(Debug, Serialize)]
pub struct SkillVersionsListResponse {
    pub versions: Vec<SkillVersionInfo>,
    pub count: usize,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct SkillVersionInfo {
    pub id: String,
    pub version: String,
    pub created_at: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct SkillVersionResponse {
    pub id: String,
    pub skill_id: String,
    pub version: String,
    pub created_at: String,
    pub name: String,
    pub description: String,
    pub directory: String,
    pub r#type: String,
}

// Request models
#[derive(Debug, Deserialize, validator::Validate)]
pub struct CreateSkillRequest {
    #[validate(length(min = 1, max = 64))]
    pub display_title: String,
    #[validate(length(max = 1024))]
    pub description: Option<String>,
}

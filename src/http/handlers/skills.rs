//! Skills CRUD endpoint handlers

use crate::http::auth::roles::EndpointPermissions;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    Json,
};
use validator::Validate;

/// GET /api/skills - List all skills
pub async fn list_skills(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<SkillsListResponse>>> {
    // Check permissions (read access required)
    let _check = EndpointPermissions::SKILLS_LIST.check(None); // TODO: Add auth context

    let skills = state.service.skill_manager().list_skills(None).await?;

    let skill_responses: Vec<SkillResponse> = skills
        .clone()
        .into_iter()
        .map(|skill| {
            // Convert SkillDefinition to the expected JSON format
            let metadata = serde_json::json!({
                "id": skill.id,
                "name": skill.name,
                "description": skill.description,
                "version": skill.version,
                "author": skill.author,
                "enabled": skill.enabled,
                "created_at": skill.created_at.to_rfc3339(),
                "updated_at": skill.updated_at.to_rfc3339(),
                "skill_file": skill.skill_file,
                "reference_files": skill.reference_files,
                "script_files": skill.script_files,
                "asset_files": skill.asset_files
            });

            SkillResponse {
                id: skill.id.to_string(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                metadata,
                created_at: Some(skill.created_at.to_rfc3339()),
                updated_at: Some(skill.updated_at.to_rfc3339()),
            }
        })
        .collect();

    let response = SkillsListResponse {
        skills: skill_responses,
        count: skills.len(),
        total: skills.len(),
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

/// GET /api/skills/{id} - Get skill details
pub async fn get_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> HttpResult<axum::Json<ApiResponse<SkillResponse>>> {
    // Check permissions
    let _check = EndpointPermissions::SKILLS_GET.check(None);

    let skills = state.service.skill_manager().list_skills(None).await?;
    let skill_id_parsed = crate::core::service::SkillId::new(skill_id.clone())
        .map_err(|_| HttpError::BadRequest("Invalid skill ID format".to_string()))?;
    let skill = skills
        .into_iter()
        .find(|s| s.id == skill_id_parsed)
        .ok_or_else(|| HttpError::NotFound(format!("Skill not found: {}", skill_id)))?;

    // Convert SkillDefinition to the expected JSON format
    let metadata = serde_json::json!({
        "id": skill.id,
        "name": skill.name,
        "description": skill.description,
        "version": skill.version,
        "author": skill.author,
        "enabled": skill.enabled,
        "created_at": skill.created_at.to_rfc3339(),
        "updated_at": skill.updated_at.to_rfc3339(),
        "skill_file": skill.skill_file,
        "reference_files": skill.reference_files,
        "script_files": skill.script_files,
        "asset_files": skill.asset_files
    });

    let response = SkillResponse {
        id: skill.id.to_string(),
        name: skill.name.clone(),
        description: skill.description.clone(),
        metadata,
        created_at: Some(skill.created_at.to_rfc3339()),
        updated_at: Some(skill.updated_at.to_rfc3339()),
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

/// POST /api/skills - Create new skill
pub async fn create_skill(
    State(_state): State<AppState>,
    Json(_request): Json<SkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<SkillResponse>>> {
    // Check permissions (write access required)
    let _check = EndpointPermissions::SKILLS_CREATE.check(None);

    // Validate request
    _request.validate().map_err(|e| {
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

    // Create skill definition
    let _skill_def = serde_json::json!({
        "id": format!("skill_{}", chrono::Utc::now().timestamp()),
        "name": _request.name,
        "description": _request.description,
    });

    // Register skill (this would need to be implemented in the service)
    // For now, return not implemented
    Err(HttpError::InternalServerError(
        "Skill creation not yet implemented".to_string(),
    ))
}

/// PUT /api/skills/{id} - Update skill
pub async fn update_skill(
    State(_state): State<AppState>,
    Path(_skill_id): Path<String>,
    Json(request): Json<SkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<SkillResponse>>> {
    // Check permissions
    let _check = EndpointPermissions::SKILLS_UPDATE.check(None);

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

    // Update skill (not implemented yet)
    Err(HttpError::InternalServerError(
        "Skill update not yet implemented".to_string(),
    ))
}

/// DELETE /api/skills/{id} - Delete skill
pub async fn delete_skill(
    State(_state): State<AppState>,
    Path(_skill_id): Path<String>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
    // Check permissions
    let _check = EndpointPermissions::SKILLS_DELETE.check(None);

    // Delete skill (not implemented yet)
    Err(HttpError::InternalServerError(
        "Skill deletion not yet implemented".to_string(),
    ))
}

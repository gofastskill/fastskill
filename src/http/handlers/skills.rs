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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeRequest {
    skill_id: Option<String>,
}

fn skill_metadata_json(skill: &crate::core::skill_manager::SkillDefinition) -> serde_json::Value {
    let source_type = skill
        .source_type
        .as_ref()
        .map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null));
    serde_json::json!({
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
        "asset_files": skill.asset_files,
        "source_url": skill.source_url,
        "source_type": source_type
    })
}

/// GET /api/skills - List all skills
pub async fn list_skills(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<SkillsListResponse>>> {
    let _check = EndpointPermissions::SKILLS_LIST.check(None);

    let skills = state.service.skill_manager().list_skills(None).await?;

    let skill_responses: Vec<SkillResponse> = skills
        .clone()
        .into_iter()
        .map(|skill| SkillResponse {
            id: skill.id.to_string(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            metadata: skill_metadata_json(&skill),
            created_at: Some(skill.created_at.to_rfc3339()),
            updated_at: Some(skill.updated_at.to_rfc3339()),
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

    let metadata = skill_metadata_json(&skill);

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

/// DELETE /api/skills/{id} - Delete skill (remove from manifest and storage, unregister)
pub async fn delete_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
    let _check = EndpointPermissions::SKILLS_DELETE.check(None);

    let skill_id_parsed = crate::core::service::SkillId::new(skill_id.clone())
        .map_err(|_| HttpError::BadRequest("Invalid skill ID format".to_string()))?;

    let skills = state.service.skill_manager().list_skills(None).await?;
    let skill = skills
        .into_iter()
        .find(|s| s.id == skill_id_parsed)
        .ok_or_else(|| HttpError::NotFound(format!("Skill not found: {}", skill_id)))?;

    let project_path = &state.project_file_path;
    let lock_path = if let Some(parent) = project_path.parent() {
        let safe_parent = if parent.exists() {
            parent.canonicalize().map_err(|e| {
                HttpError::InternalServerError(format!("Failed to resolve parent path: {}", e))
            })?
        } else {
            parent.to_path_buf()
        };
        safe_parent.join("skills.lock")
    } else {
        std::path::PathBuf::from("skills.lock")
    };

    if project_path.exists() {
        let mut project = crate::core::manifest::SkillProjectToml::load_from_file(project_path)
            .map_err(|e| {
                HttpError::InternalServerError(format!("Failed to load project: {}", e))
            })?;
        if let Some(ref mut deps) = project.dependencies {
            deps.dependencies.remove(&skill_id);
        }
        project.save_to_file(project_path).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to save project: {}", e))
        })?;
        if lock_path.exists() {
            let mut lock =
                crate::core::lock::SkillsLock::load_from_file(&lock_path).map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to load lock: {}", e))
                })?;
            lock.remove_skill(&skill_id);
            lock.save_to_file(&lock_path).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to save lock: {}", e))
            })?;
        }
    }

    let skill_dir = skill.skill_file.parent().ok_or_else(|| {
        HttpError::InternalServerError("Skill file has no parent dir".to_string())
    })?;
    if skill_dir.exists() {
        tokio::fs::remove_dir_all(skill_dir).await.map_err(|e| {
            HttpError::InternalServerError(format!("Failed to remove skill dir: {}", e))
        })?;
    }

    state
        .service
        .skill_manager()
        .unregister_skill(&skill_id_parsed)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    Ok(axum::Json(ApiResponse::success(serde_json::json!({
        "message": "Skill removed"
    }))))
}

/// POST /api/skills/upgrade - Upgrade one or all skills from manifest (shells out to fastskill update)
pub async fn upgrade_skills(
    State(state): State<AppState>,
    Json(payload): Json<Option<UpgradeRequest>>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
    let _check = EndpointPermissions::SKILLS_UPDATE.check(None);

    let project_path = state.project_file_path.clone();
    let filter_id = payload
        .and_then(|p| p.skill_id)
        .filter(|s| !s.is_empty() && s != "all");

    let output = tokio::task::spawn_blocking(move || {
        let exe = std::env::current_exe().map_err(|e| {
            HttpError::InternalServerError(format!("Failed to get executable path: {}", e))
        })?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("update");
        if let Some(ref id) = filter_id {
            cmd.arg(id);
        }
        if let Some(parent) = project_path.parent() {
            cmd.current_dir(parent);
        }
        let output = cmd
            .output()
            .map_err(|e| HttpError::InternalServerError(format!("Failed to run update: {}", e)))?;
        Ok::<_, HttpError>(output)
    })
    .await
    .map_err(|e| HttpError::InternalServerError(format!("Upgrade task failed: {}", e)))??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(HttpError::InternalServerError(format!(
            "Upgrade failed: {}",
            stderr.trim()
        )));
    }

    Ok(axum::Json(ApiResponse::success(serde_json::json!({
        "message": "Upgrade completed",
        "upgraded": true
    }))))
}

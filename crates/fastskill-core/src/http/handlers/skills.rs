//! Skills CRUD endpoint handlers

use crate::core::install::{AddMode, UpdatePreflight};
use crate::core::manifest::SkillProjectToml;
use crate::core::service::ServiceError;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

fn skill_metadata_json(skill: &crate::core::skill_manager::SkillDefinition) -> serde_json::Value {
    let origin = serde_json::to_value(&skill.origin).unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "id": skill.id,
        "name": skill.name,
        "description": skill.description,
        "version": skill.version,
        "author": skill.author,
        "created_at": skill.created_at.to_rfc3339(),
        "updated_at": skill.updated_at.to_rfc3339(),
        "skill_file": skill.skill_file,
        "reference_files": skill.reference_files,
        "script_files": skill.script_files,
        "asset_files": skill.asset_files,
        "origin": origin
    })
}

/// GET /api/skills - List all skills
pub async fn list_skills(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<SkillsListResponse>>> {
    let skills = state.service.skill_manager().list_skills().await?;

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
    let skills = state.service.skill_manager().list_skills().await?;
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

/// DELETE /api/skills/{id} - Delete skill (remove from manifest and storage, unregister)
pub async fn delete_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
    let skill_id_parsed = crate::core::service::SkillId::new(skill_id.clone())
        .map_err(|_| HttpError::BadRequest("Invalid skill ID format".to_string()))?;

    let skills = state.service.skill_manager().list_skills().await?;
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
            let mut lock = crate::core::lock::ProjectSkillsLock::load_from_file(&lock_path)
                .map_err(|e| {
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

/// POST /api/v1/skills/install - Fresh-install a skill from an [`Origin`]
/// (core install seam, ADR-0005 / spec 003 §2). `AddMode::Fresh` fails with a
/// 409 if the resolved id is already installed; other seam errors map to
/// 400/500 via the blanket `ServiceError` → `HttpError` conversion.
pub async fn install_skill(
    State(state): State<AppState>,
    Json(request): Json<InstallSkillRequest>,
) -> HttpResult<(StatusCode, axum::Json<ApiResponse<InstallSkillResponse>>)> {
    match state
        .service
        .add_from_origin(request.origin, AddMode::Fresh, request.groups)
        .await
    {
        Ok(outcome) => {
            let response = InstallSkillResponse {
                id: outcome.id,
                resolved_version: outcome.resolved.version,
                reindexed: outcome.reindexed,
            };
            Ok((StatusCode::CREATED, Json(ApiResponse::success(response))))
        }
        // ADR-0005 §Q6 / spec 003 §2: a Fresh conflict on an already-installed
        // id is a 409, not a generic 400 (which the blanket ServiceError→HttpError
        // mapping would otherwise give it).
        Err(ServiceError::AlreadyIndexed(id)) => Err(HttpError::Conflict(format!(
            "Skill '{}' is already installed",
            id
        ))),
        Err(e) => Err(e.into()),
    }
}

/// POST /api/v1/skills/update (and its back-compat alias `/skills/upgrade`) -
/// update one or all skills recorded in the project's `skill-project.toml` by
/// routing each dependency's recorded `Origin` through the core install seam
/// (ADR-0005), mirroring `fastskill-cli`'s `update` command: `preflight(&origin)`
/// decides Updatable/UpToDate/Immutable, and only `Updatable` entries are
/// re-fetched via `add_from_origin(origin, AddMode::Update, groups)`.
/// `check: true` reports the preflight verdict without applying anything.
/// Always 200 with a per-skill result list — a per-skill failure does not fail
/// the whole request.
pub async fn update_skills(
    State(state): State<AppState>,
    Json(payload): Json<Option<UpdateSkillsRequest>>,
) -> HttpResult<axum::Json<ApiResponse<Vec<SkillUpdateResult>>>> {
    let payload = payload.unwrap_or_default();
    let project_path = &state.project_file_path;

    if !project_path.exists() {
        return Err(HttpError::NotFound(
            "skill-project.toml not found".to_string(),
        ));
    }

    let project = SkillProjectToml::load_from_file(project_path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
    })?;

    let mut entries = project
        .to_skill_entries()
        .map_err(HttpError::InternalServerError)?;
    entries.sort_by(|a, b| a.id.cmp(&b.id));

    let filter_id = payload
        .skill_id
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "all");
    if let Some(id) = filter_id {
        entries.retain(|e| e.id == id);
        if entries.is_empty() {
            return Err(HttpError::NotFound(format!("Unknown skill: {}", id)));
        }
    }

    let mut results = Vec::with_capacity(entries.len());
    for entry in entries {
        match state.service.preflight(&entry.origin).await {
            Ok(UpdatePreflight::Updatable) if payload.check => {
                results.push(SkillUpdateResult {
                    id: entry.id,
                    outcome: "would_update".to_string(),
                    reason: None,
                    resolved_version: None,
                });
            }
            Ok(UpdatePreflight::Updatable) => {
                match state
                    .service
                    .add_from_origin(entry.origin.clone(), AddMode::Update, entry.groups.clone())
                    .await
                {
                    Ok(outcome) => results.push(SkillUpdateResult {
                        id: entry.id,
                        outcome: "updated".to_string(),
                        reason: None,
                        resolved_version: Some(outcome.resolved.version),
                    }),
                    Err(e) => results.push(SkillUpdateResult {
                        id: entry.id,
                        outcome: "error".to_string(),
                        reason: Some(e.to_string()),
                        resolved_version: None,
                    }),
                }
            }
            Ok(UpdatePreflight::UpToDate) => results.push(SkillUpdateResult {
                id: entry.id,
                outcome: "up_to_date".to_string(),
                reason: None,
                resolved_version: None,
            }),
            Ok(UpdatePreflight::Immutable { reason }) => results.push(SkillUpdateResult {
                id: entry.id,
                outcome: "immutable".to_string(),
                reason: Some(reason),
                resolved_version: None,
            }),
            Err(e) => results.push(SkillUpdateResult {
                id: entry.id,
                outcome: "error".to_string(),
                reason: Some(e.to_string()),
                resolved_version: None,
            }),
        }
    }

    Ok(axum::Json(ApiResponse::success(results)))
}

//! Skills CRUD endpoint handlers

use crate::core::lock::ProjectSkillsLock;
use crate::core::manifest::{SkillEntry, SkillProjectToml};
use crate::core::origin::Origin;
use crate::core::project_apply::{ProjectApplyCandidate, ProjectApplyPlan};
use crate::core::project_removal::ProjectRemovalService;
use crate::core::resolution::{prepare_resolution, prepare_resolution_preview, ResolutionRoot};
use crate::core::service::ServiceError;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, Query, State},
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

fn lifecycle_http_error(error: ServiceError) -> HttpError {
    match error {
        ServiceError::InvalidOperation(message)
            if message.contains("required by")
                || message.contains("owned by")
                || message.contains("managed by")
                || message.contains("locally modified")
                || message.contains("digest")
                || message.contains("recovery")
                || message.contains("state changed") =>
        {
            HttpError::Conflict(message)
        }
        other => other.into(),
    }
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

/// GET /api/v1/skills/{id}/content - Read-only view of the installed skill's
/// `SKILL.md` (spec 003 §5 / Phase 3 §Q4). Always mounted on the read router
/// (not write-gated). PATH-CONFINEMENT is a hard requirement here: the
/// resolved file must canonicalize to somewhere inside the canonicalized
/// skills directory, or the request is rejected — this endpoint must never
/// become a directory-traversal primitive, even though `serve` itself is not a
/// security boundary (ADR-0003).
pub async fn get_skill_content(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
    Query(query): Query<ContentQuery>,
) -> HttpResult<axum::Json<ApiResponse<SkillContentResponse>>> {
    let skill_id_parsed = crate::core::service::SkillId::new(skill_id.clone())
        .map_err(|_| HttpError::BadRequest("Invalid skill ID format".to_string()))?;

    let skills = state.service.skill_manager().list_skills().await?;
    let skill = skills
        .into_iter()
        .find(|s| s.id == skill_id_parsed)
        .ok_or_else(|| HttpError::NotFound(format!("Skill not found: {}", skill_id)))?;

    // `skill_file` may be stored relative (e.g. `./skills/{id}/SKILL.md`) or
    // absolute; resolve a relative one against the skills directory before
    // touching the filesystem.
    let candidate = if skill.skill_file.is_absolute() {
        skill.skill_file.clone()
    } else {
        state.skills_directory.join(&skill.skill_file)
    };

    let confined =
        crate::security::path::validate_path_within_root(&candidate, &state.skills_directory)
            .map_err(|e| match e {
                crate::security::path::PathSecurityError::EscapesRoot(msg)
                | crate::security::path::PathSecurityError::TraversalAttempt(msg)
                | crate::security::path::PathSecurityError::InvalidComponent(msg) => {
                    HttpError::BadRequest(msg)
                }
                crate::security::path::PathSecurityError::CanonicalizationFailed(_) => {
                    HttpError::NotFound(format!("Skill file not found on disk: {}", skill_id))
                }
            })?;

    let content = tokio::fs::read_to_string(&confined)
        .await
        .map_err(|_| HttpError::NotFound(format!("Skill file not found on disk: {}", skill_id)))?;

    // Report the skills-dir-relative path, not the absolute server path — the UI
    // only needs the logical location, and leaking the server's directory layout
    // is needless disclosure if `serve` is ever exposed. Fall back to the bare
    // file name if the relative strip fails.
    let display_path = state
        .skills_directory
        .canonicalize()
        .ok()
        .and_then(|root| {
            confined
                .strip_prefix(&root)
                .ok()
                .map(std::path::Path::to_path_buf)
        })
        .or_else(|| confined.file_name().map(std::path::PathBuf::from))
        .unwrap_or_else(|| confined.clone());

    let format = query.format.unwrap_or_default();
    let rendered_content = match format {
        ContentFormat::Raw => content,
        ContentFormat::Html => render_skill_markdown_html(&content),
    };

    Ok(axum::Json(ApiResponse::success(SkillContentResponse {
        path: display_path.to_string_lossy().to_string(),
        format: format.as_str().to_string(),
        content: rendered_content,
    })))
}

/// Render `SKILL.md` Markdown to sanitized HTML (spec 003 v2 / Phase 4 §5
/// sanitized preview). Server-side rendering via `comrak` followed by
/// allowlist sanitization via `ammonia::clean` — the result is safe to assign
/// to `innerHTML`: no `<script>`, no `on*` handlers, no `javascript:`/`data:`
/// URLs (ammonia's default strict allowlist strips all three).
fn render_skill_markdown_html(markdown: &str) -> String {
    let unsafe_html = comrak::markdown_to_html(markdown, &comrak::Options::default());
    ammonia::clean(&unsafe_html)
}

/// DELETE /api/skills/{id} - Delete skill (remove from manifest and storage, unregister)
pub async fn delete_skill(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
    let result = remove_project_skill(&state, skill_id).await?;
    Ok(axum::Json(ApiResponse::success(result)))
}

pub(crate) async fn remove_project_skill(
    state: &AppState,
    skill_id: String,
) -> HttpResult<serde_json::Value> {
    state.require_project_scope()?;
    crate::core::service::SkillId::new(skill_id.clone())
        .map_err(|_| HttpError::BadRequest("Invalid skill ID format".to_string()))?;
    if !state.project_file_path.exists() {
        return Err(HttpError::NotFound(
            "skill-project.toml not found for the served project".to_string(),
        ));
    }
    let plan = ProjectRemovalService::new(&state.project_root, &state.skills_directory)
        .remove(std::slice::from_ref(&skill_id))
        .map_err(lifecycle_http_error)?;
    if plan.unchanged.iter().any(|id| id == &skill_id) {
        return Err(HttpError::NotFound(format!("Skill not found: {skill_id}")));
    }
    let mut diagnostics = Vec::new();
    for removed in &plan.delete_files {
        let id = crate::core::service::SkillId::new(removed.clone()).map_err(HttpError::from)?;
        match state.service.skill_manager().unregister_skill(&id).await {
            Ok(()) | Err(ServiceError::SkillNotFound(_)) => {}
            Err(error) => {
                diagnostics.push(format!(
                    "Managed state changed, but registry cleanup failed for '{removed}': {error}"
                ));
            }
        }
    }

    Ok(serde_json::json!({
        "outcome": "changed",
        "removed": plan.remove_manifest_dependencies,
        "deleted": plan.delete_files,
        "retained": plan.retained_files,
        "diagnostics": diagnostics
    }))
}

/// POST /api/v1/skills/install - Fresh-install a skill from an **Origin ref**
/// string (core install seam, ADR-0005 / spec 003 §2 + Phase 3). The server
/// classifies `request.origin` via `infer_origin` — the UI performs no
/// detection of its own — then `AddMode::Fresh` fails with a 409 if the
/// resolved id is already installed; other seam errors map to 400/500 via the
/// blanket `ServiceError` → `HttpError` conversion.
pub async fn install_skill(
    State(state): State<AppState>,
    Json(request): Json<InstallSkillRequest>,
) -> HttpResult<(StatusCode, axum::Json<ApiResponse<InstallSkillResponse>>)> {
    state.require_project_scope()?;
    // A bad ref / no-default-repo is a client error (400), not a 500 — surface
    // it distinctly from the blanket `ServiceError` → `HttpError` conversion
    // below (which maps `Config` to 500, appropriate for the *fetch* path but
    // not for classifying the ref itself).
    let mut origin = state
        .service
        .infer_origin(&request.origin)
        .await
        .map_err(|e| HttpError::BadRequest(e.to_string()))?;
    if let Some(repository) = request.repository.as_deref() {
        let manager = state
            .service
            .repository_manager()
            .ok_or_else(|| HttpError::BadRequest("No repositories are configured".to_string()))?;
        if manager.get_repository(repository).is_none() {
            return Err(HttpError::NotFound(format!(
                "Repository not found: {repository}"
            )));
        }
        match &mut origin {
            Origin::Repository { repo, .. } => *repo = repository.to_string(),
            _ => {
                return Err(HttpError::BadRequest(
                    "repository applies only to a repository skill ID reference".to_string(),
                ));
            }
        }
    }

    let expected_id = match &origin {
        Origin::Repository { skill, .. } => Some(skill.clone()),
        _ => None,
    };
    let lock_path = state.project_root.join("skills.lock");
    let existing = ProjectSkillsLock::load_from_file(&lock_path).ok();
    let project = SkillProjectToml::load_from_file(&state.project_file_path)
        .map_err(|error| HttpError::InternalServerError(error.to_string()))?;
    let manifest_entries = project
        .to_skill_entries(&state.project_root)
        .map_err(HttpError::InternalServerError)?;

    if let Some(id) = expected_id.as_deref() {
        reject_existing_install(existing.as_ref(), &manifest_entries, id)?;
    }

    let roots = || {
        vec![ResolutionRoot {
            origin: origin.clone(),
            expected_id: expected_id.clone(),
            groups: request.groups.clone(),
            locked: None,
        }]
    };
    // A Git/ZIP/local source reveals its canonical ID only after acquisition.
    // Resolve it in disposable state first so a duplicate request cannot alter
    // the real repository catalog or content cache before returning 409.
    if expected_id.is_none() {
        let preview = prepare_resolution_preview(
            &state.service,
            roots(),
            &std::collections::HashMap::new(),
            5,
        )
        .await
        .map_err(lifecycle_http_error)?;
        let id = preview
            .root_ids
            .first()
            .ok_or_else(|| HttpError::BadRequest("install resolved no skill".to_string()))?;
        reject_existing_install(existing.as_ref(), &manifest_entries, id)?;
    }

    let resolution = prepare_resolution(
        &state.service,
        roots(),
        &std::collections::HashMap::new(),
        5,
        false,
    )
    .await
    .map_err(lifecycle_http_error)?;
    let root_id = resolution
        .root_ids
        .first()
        .cloned()
        .ok_or_else(|| HttpError::BadRequest("install resolved no skill".to_string()))?;
    reject_existing_install(existing.as_ref(), &manifest_entries, &root_id)?;
    let version = resolution
        .candidates
        .iter()
        .find(|candidate| candidate.prepared.id() == root_id)
        .map(|candidate| candidate.prepared.resolved().version.clone())
        .ok_or_else(|| HttpError::BadRequest("install root was not prepared".to_string()))?;
    let plan = ProjectApplyPlan {
        candidates: resolution
            .candidates
            .into_iter()
            .map(project_candidate)
            .collect(),
        expected_skills: existing
            .as_ref()
            .map(|lock| lock.skills.clone())
            .unwrap_or_default(),
        expected_manifest: vec![crate::core::project_apply::ExpectedManifestSkill {
            id: root_id.clone(),
            entry: manifest_entries
                .iter()
                .find(|entry| entry.id == root_id)
                .cloned(),
        }],
        base_lock: existing,
        covered_roots: vec![root_id.clone()],
        manifest_updates: vec![crate::core::manifest::SkillEntry {
            id: root_id.clone(),
            origin,
            groups: request.groups,
        }],
        removals: Vec::new(),
        write_lock: true,
    };
    state
        .service
        .apply_project_plan(&state.project_root, plan)
        .await
        .map_err(lifecycle_http_error)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(InstallSkillResponse {
            id: root_id,
            resolved_version: version,
            reindexed: false,
        })),
    ))
}

fn reject_existing_install(
    lock: Option<&ProjectSkillsLock>,
    manifest: &[SkillEntry],
    id: &str,
) -> HttpResult<()> {
    if lock.is_some_and(|lock| lock.covered_roots.iter().any(|root| root == id))
        || manifest.iter().any(|entry| entry.id == id)
    {
        Err(HttpError::Conflict(format!(
            "Skill '{id}' is already installed"
        )))
    } else {
        Ok(())
    }
}

fn project_candidate(
    candidate: crate::core::resolution::ResolutionCandidate,
) -> ProjectApplyCandidate {
    ProjectApplyCandidate {
        prepared: candidate.prepared,
        origin: candidate.origin,
        groups: candidate.groups,
        depth: candidate.depth,
        required_by: candidate.required_by,
        dependencies: candidate.dependencies,
    }
}

#[path = "skills_update.rs"]
mod update;
pub use update::update_skills;

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::service::{FastSkillService, SkillId};
    use crate::core::skill_manager::SkillDefinition;
    use crate::http::handlers::ServedScope;
    use std::sync::Arc;

    #[tokio::test]
    async fn content_and_remove_handlers_report_relative_escape_missing_and_absent_paths() {
        let root = tempfile::tempdir().unwrap();
        let skills = root.path().join("skills");
        std::fs::create_dir_all(skills.join("demo")).unwrap();
        std::fs::write(skills.join("demo/SKILL.md"), "# demo").unwrap();
        let service = Arc::new(
            FastSkillService::new(crate::ServiceConfig {
                skill_storage_path: skills.clone(),
                ..Default::default()
            })
            .await
            .unwrap(),
        );
        let id = SkillId::new("demo".to_string()).unwrap();
        let mut definition = SkillDefinition::new(
            id.clone(),
            "demo".to_string(),
            "demo".to_string(),
            "1.0.0".to_string(),
            Origin::Local {
                path: root.path().join("source"),
                editable: false,
            },
        );
        definition.skill_file = std::path::PathBuf::from("demo/SKILL.md");
        service
            .skill_manager()
            .force_register_skill(definition.clone())
            .await
            .unwrap();
        let project_file = root.path().join("skill-project.toml");
        std::fs::write(&project_file, "[dependencies]\n").unwrap();
        let state = AppState {
            service: service.clone(),
            start_time: std::time::SystemTime::now(),
            project_file_path: project_file,
            project_root: root.path().to_path_buf(),
            skills_directory: skills,
            served_scope: ServedScope::Project,
            enable_write: true,
        };

        assert!(get_skill_content(
            State(state.clone()),
            Path("demo".to_string()),
            Query(ContentQuery::default()),
        )
        .await
        .is_ok());

        let outside = root.path().join("outside.md");
        std::fs::write(&outside, "outside").unwrap();
        definition.skill_file = outside;
        service
            .skill_manager()
            .force_register_skill(definition.clone())
            .await
            .unwrap();
        assert!(matches!(
            get_skill_content(
                State(state.clone()),
                Path("demo".to_string()),
                Query(ContentQuery::default()),
            )
            .await,
            Err(HttpError::BadRequest(_))
        ));

        definition.skill_file = std::path::PathBuf::from("missing/SKILL.md");
        service
            .skill_manager()
            .force_register_skill(definition)
            .await
            .unwrap();
        assert!(matches!(
            get_skill_content(
                State(state.clone()),
                Path("demo".to_string()),
                Query(ContentQuery::default()),
            )
            .await,
            Err(HttpError::NotFound(_))
        ));
        assert!(matches!(
            remove_project_skill(&state, "demo".to_string()).await,
            Err(HttpError::NotFound(_))
        ));
    }
}

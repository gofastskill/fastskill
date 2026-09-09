//! Skills CRUD endpoint handlers

use crate::core::lock::ProjectSkillsLock;
use crate::core::manifest::SkillProjectToml;
use crate::core::origin::Origin;
use crate::core::project_apply::{ProjectApplyCandidate, ProjectApplyPlan};
use crate::core::project_removal::ProjectRemovalService;
use crate::core::resolution::{prepare_resolution, prepare_resolution_preview, ResolutionRoot};
use crate::core::service::ServiceError;
use crate::core::version::VersionConstraint;
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
    let resolution = prepare_resolution(
        &state.service,
        vec![ResolutionRoot {
            origin: origin.clone(),
            expected_id,
            groups: request.groups.clone(),
            locked: None,
        }],
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
    let lock_path = state.project_root.join("skills.lock");
    let existing = ProjectSkillsLock::load_from_file(&lock_path).ok();
    let project = SkillProjectToml::load_from_file(&state.project_file_path)
        .map_err(|error| HttpError::InternalServerError(error.to_string()))?;
    let manifest_entries = project
        .to_skill_entries(&state.project_root)
        .map_err(HttpError::InternalServerError)?;
    if existing
        .as_ref()
        .is_some_and(|lock| lock.covered_roots.contains(&root_id))
        || project
            .to_skill_entries(&state.project_root)
            .map_err(HttpError::InternalServerError)?
            .iter()
            .any(|entry| entry.id == root_id)
    {
        return Err(HttpError::Conflict(format!(
            "Skill '{root_id}' is already installed"
        )));
    }
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

/// POST /api/v1/skills/update (and its back-compat alias `/skills/upgrade`) -
/// update one or all skills recorded in the project's `skill-project.toml` by
/// routing each dependency's recorded `Origin` through the core install seam
/// (ADR-0005), mirroring `fastskill-cli`'s `update` command: `preflight(&origin)`
/// decides Updatable/UpToDate/Immutable, and only `Updatable` entries are
/// re-fetched via `add_from_origin(origin, AddMode::Update, groups)`.
/// `check: true` validates and reports the selected target without applying it.
pub async fn update_skills(
    State(state): State<AppState>,
    Json(payload): Json<Option<UpdateSkillsRequest>>,
) -> HttpResult<(StatusCode, axum::Json<ApiResponse<UpdateSkillsResponse>>)> {
    state.require_project_scope()?;
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
        .to_skill_entries(project_path.parent().unwrap_or(std::path::Path::new(".")))
        .map_err(HttpError::InternalServerError)?;
    entries.sort_by(|a, b| a.id.cmp(&b.id));

    let filter_id = payload
        .skill_id
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "all");

    if let Some(version) = payload.version.as_deref() {
        let Some(id) = filter_id else {
            return Err(HttpError::BadRequest(
                "`version` requires one named `skillId`".to_string(),
            ));
        };
        let entry = entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| HttpError::NotFound(format!("Unknown skill: {id}")))?;
        let Origin::Repository { repo, skill, .. } = &entry.origin else {
            return Err(HttpError::BadRequest(format!(
                "version pin only applies to repository-origin skills; '{id}' is not repository-backed"
            )));
        };
        let constraint = VersionConstraint::parse(version)
            .map_err(|e| HttpError::BadRequest(format!("Invalid version '{version}': {e}")))?;
        if constraint.as_exact().is_none() {
            return Err(HttpError::BadRequest(
                "version must be an exact semantic version".to_string(),
            ));
        }
        let pinned_origin = Origin::Repository {
            repo: repo.clone(),
            skill: skill.clone(),
            version: Some(constraint),
        };
        let result = update_project_root(&state, entry, pinned_origin, payload.check).await;
        return Ok(update_batch_response(vec![result], payload.check));
    }

    if let Some(id) = filter_id {
        entries.retain(|e| e.id == id);
        if entries.is_empty() {
            return Err(HttpError::NotFound(format!("Unknown skill: {}", id)));
        }
    }

    let mut results = Vec::with_capacity(entries.len());
    for entry in entries {
        results
            .push(update_project_root(&state, &entry, entry.origin.clone(), payload.check).await);
    }

    Ok(update_batch_response(results, payload.check))
}

async fn update_project_root(
    state: &AppState,
    entry: &crate::core::manifest::SkillEntry,
    origin: Origin,
    check: bool,
) -> SkillUpdateResult {
    let lock_path = state.project_root.join("skills.lock");
    let existing = ProjectSkillsLock::load_from_file(&lock_path).ok();
    let locked = existing
        .as_ref()
        .map(|lock| {
            lock.skills
                .iter()
                .map(|item| (item.id.clone(), item.resolved.clone()))
                .collect()
        })
        .unwrap_or_default();
    let root = ResolutionRoot {
        origin: origin.clone(),
        expected_id: Some(entry.id.clone()),
        groups: entry.groups.clone(),
        locked: None,
    };
    let resolution = if check {
        prepare_resolution_preview(&state.service, vec![root], &locked, 5).await
    } else {
        prepare_resolution(&state.service, vec![root], &locked, 5, false).await
    };
    let resolution = match resolution {
        Ok(resolution) => resolution,
        Err(error) => {
            return SkillUpdateResult {
                id: entry.id.clone(),
                outcome: "error".to_string(),
                reason: Some(error.to_string()),
                resolved_version: None,
            };
        }
    };
    let version = resolution
        .candidates
        .iter()
        .find(|candidate| candidate.prepared.id() == entry.id)
        .map(|candidate| candidate.prepared.resolved().version.clone());
    let positions: std::collections::BTreeSet<_> = resolution
        .candidates
        .iter()
        .map(|candidate| candidate.prepared.id().to_string())
        .collect();
    let expected_skills = existing
        .as_ref()
        .map(|lock| lock.skills.clone())
        .unwrap_or_default();
    let (base_lock, mut removals) = existing
        .map(|lock| {
            let (lock, removals) = crate::core::project_apply::retain_unaffected_roots(
                lock,
                std::slice::from_ref(&entry.id),
            );
            (Some(lock), removals)
        })
        .unwrap_or((None, Vec::new()));
    removals.retain(|id| !positions.contains(id));
    let plan = ProjectApplyPlan {
        candidates: resolution
            .candidates
            .into_iter()
            .map(project_candidate)
            .collect(),
        base_lock,
        covered_roots: vec![entry.id.clone()],
        manifest_updates: vec![crate::core::manifest::SkillEntry {
            id: entry.id.clone(),
            origin,
            groups: entry.groups.clone(),
        }],
        removals,
        write_lock: true,
        expected_skills,
        expected_manifest: vec![crate::core::project_apply::ExpectedManifestSkill {
            id: entry.id.clone(),
            entry: Some(entry.clone()),
        }],
    };
    if let Err(error) = state
        .service
        .validate_project_plan(&state.project_root, &plan)
    {
        return SkillUpdateResult {
            id: entry.id.clone(),
            outcome: "error".to_string(),
            reason: Some(error.to_string()),
            resolved_version: version,
        };
    }
    if !check {
        if let Err(error) = state
            .service
            .apply_project_plan(&state.project_root, plan)
            .await
        {
            return SkillUpdateResult {
                id: entry.id.clone(),
                outcome: "error".to_string(),
                reason: Some(error.to_string()),
                resolved_version: version,
            };
        }
    }
    SkillUpdateResult {
        id: entry.id.clone(),
        outcome: if check { "would_update" } else { "updated" }.to_string(),
        reason: None,
        resolved_version: version,
    }
}

fn update_batch_response(
    results: Vec<SkillUpdateResult>,
    check: bool,
) -> (StatusCode, axum::Json<ApiResponse<UpdateSkillsResponse>>) {
    let failed = results
        .iter()
        .filter(|result| result.outcome == "error")
        .count();
    let changed = results
        .iter()
        .filter(|result| matches!(result.outcome.as_str(), "updated" | "would_update"))
        .count();
    let outcome = if failed == results.len() && failed > 0 {
        "failed"
    } else if failed > 0 {
        "partial"
    } else if changed == 0 {
        "unchanged"
    } else if check {
        "would_change"
    } else {
        "changed"
    };
    let status = if outcome == "failed" {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::OK
    };
    (
        status,
        axum::Json(ApiResponse {
            success: failed == 0,
            data: Some(UpdateSkillsResponse {
                outcome: outcome.to_string(),
                results,
            }),
            error: None,
            meta: None,
        }),
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn result(id: &str, outcome: &str) -> SkillUpdateResult {
        SkillUpdateResult {
            id: id.to_string(),
            outcome: outcome.to_string(),
            reason: None,
            resolved_version: None,
        }
    }

    #[test]
    fn batch_status_distinguishes_failed_partial_and_unchanged_results() {
        let (status, Json(response)) = update_batch_response(vec![result("one", "error")], false);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.data.expect("data").outcome, "failed");

        let (status, Json(response)) = update_batch_response(
            vec![result("one", "updated"), result("two", "error")],
            false,
        );
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.data.expect("data").outcome, "partial");

        let (status, Json(response)) = update_batch_response(Vec::new(), false);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response.data.expect("data").outcome, "unchanged");

        let (_, Json(response)) = update_batch_response(vec![result("one", "would_update")], true);
        assert_eq!(response.data.expect("data").outcome, "would_change");

        let (_, Json(response)) = update_batch_response(vec![result("one", "updated")], false);
        assert_eq!(response.data.expect("data").outcome, "changed");
    }
}

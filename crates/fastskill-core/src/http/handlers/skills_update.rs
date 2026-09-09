//! Coordinated HTTP project-update planning and application.

use super::project_candidate;
use crate::core::lock::ProjectSkillsLock;
use crate::core::manifest::{ManifestError, SkillEntry, SkillProjectToml};
use crate::core::origin::{GitRef, Origin, Resolved};
use crate::core::project_apply::ProjectApplyPlan;
use crate::core::resolution::{
    origins_accept_same_resolution, prepare_resolution, prepare_resolution_preview, ResolutionPlan,
    ResolutionRoot,
};
use crate::core::service::ServiceError;
use crate::core::version::VersionConstraint;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::{
    ApiResponse, SkillUpdateResult, UpdateSkillsRequest, UpdateSkillsResponse,
};
use axum::{extract::State, http::StatusCode, Json};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone)]
struct UpdateRoot {
    entry: SkillEntry,
    origin: Origin,
}

/// POST /api/v1/skills/update (and its back-compat alias `/skills/upgrade`).
/// Every selected mutable root is resolved as one graph so shared dependency
/// constraints and ownership have the same meaning as a CLI batch update.
pub async fn update_skills(
    State(state): State<AppState>,
    Json(payload): Json<Option<UpdateSkillsRequest>>,
) -> HttpResult<(StatusCode, axum::Json<ApiResponse<UpdateSkillsResponse>>)> {
    state.require_project_scope()?;
    let payload = payload.unwrap_or_default();
    let project_path = &state.project_file_path;

    let project = SkillProjectToml::load_from_file(project_path).map_err(|error| match error {
        ManifestError::NotFound(_) => {
            HttpError::NotFound("skill-project.toml not found".to_string())
        }
        error => {
            HttpError::InternalServerError(format!("Failed to load skill-project.toml: {error}"))
        }
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
        return Ok(update_project_roots(
            &state,
            vec![UpdateRoot {
                entry: entry.clone(),
                origin: pinned_origin,
            }],
            payload.check,
        )
        .await);
    }

    if let Some(id) = filter_id {
        entries.retain(|e| e.id == id);
        if entries.is_empty() {
            return Err(HttpError::NotFound(format!("Unknown skill: {}", id)));
        }
    }

    let roots = entries
        .into_iter()
        .map(|entry| UpdateRoot {
            origin: entry.origin.clone(),
            entry,
        })
        .collect();
    Ok(update_project_roots(&state, roots, payload.check).await)
}

async fn update_project_roots(
    state: &AppState,
    roots: Vec<UpdateRoot>,
    check: bool,
) -> (StatusCode, axum::Json<ApiResponse<UpdateSkillsResponse>>) {
    let lock_path = state.project_root.join("skills.lock");
    let existing = ProjectSkillsLock::load_from_file(&lock_path).ok();
    let locked: HashMap<String, Resolved> = existing
        .as_ref()
        .map(|lock| {
            lock.skills
                .iter()
                .map(|item| (item.id.clone(), item.resolved.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut results = Vec::new();
    let mut mutable = Vec::new();
    for root in roots {
        if let Some(reason) = immutable_reason(&root.origin) {
            let resolved_version = locked
                .get(&root.entry.id)
                .map(|resolved| resolved.version.clone());
            results.push(SkillUpdateResult {
                id: root.entry.id,
                outcome: "immutable".to_string(),
                reason: Some(reason),
                resolved_version,
            });
        } else {
            mutable.push(root);
        }
    }
    if mutable.is_empty() {
        results.sort_by(|left, right| left.id.cmp(&right.id));
        return update_batch_response(results, check);
    }

    // Validate the whole graph in disposable state first. If one root is bad,
    // isolate it with disposable per-root previews, then coordinate all viable
    // roots in one graph. No failed requested target refreshes persistent state.
    let preview =
        prepare_resolution_preview(&state.service, resolution_roots(&mutable), &locked, 5).await;
    let (viable, preview) = match preview {
        Ok(preview) => (mutable, Some(preview)),
        Err(_) => isolate_viable_roots(state, mutable, &locked, &mut results).await,
    };
    if viable.is_empty() {
        results.sort_by(|left, right| left.id.cmp(&right.id));
        return update_batch_response(results, check);
    }
    let preview = match preview {
        Some(preview) => preview,
        None => {
            match prepare_resolution_preview(&state.service, resolution_roots(&viable), &locked, 5)
                .await
            {
                Ok(preview) => preview,
                Err(error) => {
                    append_update_errors(&mut results, &viable, &error);
                    results.sort_by(|left, right| left.id.cmp(&right.id));
                    return update_batch_response(results, check);
                }
            }
        }
    };
    let resolution = if check {
        Ok(preview)
    } else {
        prepare_resolution(&state.service, resolution_roots(&viable), &locked, 5, false).await
    };
    let resolution = match resolution {
        Ok(resolution) => resolution,
        Err(error) => {
            append_update_errors(&mut results, &viable, &error);
            results.sort_by(|left, right| left.id.cmp(&right.id));
            return update_batch_response(results, check);
        }
    };
    let changed_roots = changed_update_roots(state, &viable, &resolution, existing.as_ref());
    let versions: HashMap<_, _> = resolution
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.prepared.id().to_string(),
                candidate.prepared.resolved().version.clone(),
            )
        })
        .collect();
    if changed_roots.is_empty() {
        append_update_outcomes(&mut results, &viable, &changed_roots, &versions, check);
        results.sort_by(|left, right| left.id.cmp(&right.id));
        return update_batch_response(results, check);
    }

    let affected = graph_closure(
        changed_roots.iter().cloned(),
        &resolution
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.prepared.id().to_string(),
                    candidate.dependencies.clone(),
                )
            })
            .collect(),
    );
    let expected_skills = existing
        .as_ref()
        .map(|lock| lock.skills.clone())
        .unwrap_or_default();
    let (base_lock, mut removals) = existing
        .map(|lock| {
            let (lock, removals) = crate::core::project_apply::retain_unaffected_roots(
                lock,
                &changed_roots.iter().cloned().collect::<Vec<_>>(),
            );
            (Some(lock), removals)
        })
        .unwrap_or((None, Vec::new()));
    removals.retain(|id| !affected.contains(id));
    let plan = ProjectApplyPlan {
        candidates: resolution
            .candidates
            .into_iter()
            .filter(|candidate| affected.contains(candidate.prepared.id()))
            .map(project_candidate)
            .collect(),
        base_lock,
        covered_roots: changed_roots.iter().cloned().collect(),
        manifest_updates: viable
            .iter()
            .filter(|root| changed_roots.contains(&root.entry.id))
            .map(|root| SkillEntry {
                id: root.entry.id.clone(),
                origin: root.origin.clone(),
                groups: root.entry.groups.clone(),
            })
            .collect(),
        removals,
        write_lock: true,
        expected_skills,
        expected_manifest: viable
            .iter()
            .filter(|root| changed_roots.contains(&root.entry.id))
            .map(|root| crate::core::project_apply::ExpectedManifestSkill {
                id: root.entry.id.clone(),
                entry: Some(root.entry.clone()),
            })
            .collect(),
    };
    if let Err(error) = state
        .service
        .validate_project_plan(&state.project_root, &plan)
    {
        append_update_errors(&mut results, &viable, &error);
        results.sort_by(|left, right| left.id.cmp(&right.id));
        return update_batch_response(results, check);
    }
    if !check {
        if let Err(error) = state
            .service
            .apply_project_plan(&state.project_root, plan)
            .await
        {
            append_update_errors(&mut results, &viable, &error);
            results.sort_by(|left, right| left.id.cmp(&right.id));
            return update_batch_response(results, check);
        }
    }
    append_update_outcomes(&mut results, &viable, &changed_roots, &versions, check);
    results.sort_by(|left, right| left.id.cmp(&right.id));
    update_batch_response(results, check)
}

fn resolution_roots(roots: &[UpdateRoot]) -> Vec<ResolutionRoot> {
    roots
        .iter()
        .map(|root| ResolutionRoot {
            origin: root.origin.clone(),
            expected_id: Some(root.entry.id.clone()),
            groups: root.entry.groups.clone(),
            locked: None,
        })
        .collect()
}

async fn isolate_viable_roots(
    state: &AppState,
    roots: Vec<UpdateRoot>,
    locked: &HashMap<String, Resolved>,
    results: &mut Vec<SkillUpdateResult>,
) -> (Vec<UpdateRoot>, Option<ResolutionPlan>) {
    let mut viable = Vec::new();
    for root in roots {
        match prepare_resolution_preview(
            &state.service,
            resolution_roots(std::slice::from_ref(&root)),
            locked,
            5,
        )
        .await
        {
            Ok(_) => viable.push(root),
            Err(error) => append_update_errors(results, std::slice::from_ref(&root), &error),
        }
    }
    (viable, None)
}

fn immutable_reason(origin: &Origin) -> Option<String> {
    match origin {
        Origin::Git {
            r#ref: GitRef::Tag(tag),
            ..
        } => Some(format!("pinned to git tag '{tag}'; tags do not move")),
        Origin::Git {
            r#ref: GitRef::Commit(commit),
            ..
        } => Some(format!(
            "pinned to git commit '{commit}'; commits do not move"
        )),
        Origin::Local { editable: true, .. } => Some(
            "editable local install is a live symlink to the source directory; there is nothing to re-fetch"
                .to_string(),
        ),
        _ => None,
    }
}

fn changed_update_roots(
    state: &AppState,
    roots: &[UpdateRoot],
    resolution: &ResolutionPlan,
    existing: Option<&ProjectSkillsLock>,
) -> BTreeSet<String> {
    let new_graph: HashMap<_, _> = resolution
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.prepared.id().to_string(),
                candidate.dependencies.clone(),
            )
        })
        .collect();
    let old_graph: HashMap<_, _> = existing
        .map(|lock| {
            lock.skills
                .iter()
                .map(|entry| (entry.id.clone(), entry.dependencies.clone()))
                .collect()
        })
        .unwrap_or_default();
    let changed_candidates: BTreeSet<_> = resolution
        .candidates
        .iter()
        .filter(|candidate| !candidate_is_current(state, candidate, existing))
        .map(|candidate| candidate.prepared.id().to_string())
        .collect();
    roots
        .iter()
        .filter(|root| {
            let new = graph_closure(std::iter::once(root.entry.id.clone()), &new_graph);
            let old = graph_closure(std::iter::once(root.entry.id.clone()), &old_graph);
            new != old || !new.is_disjoint(&changed_candidates) || root.entry.origin != root.origin
        })
        .map(|root| root.entry.id.clone())
        .collect()
}

fn candidate_is_current(
    state: &AppState,
    candidate: &crate::core::resolution::ResolutionCandidate,
    existing: Option<&ProjectSkillsLock>,
) -> bool {
    let Some(entry) = existing.and_then(|lock| {
        lock.skills
            .iter()
            .find(|entry| entry.id == candidate.prepared.id())
    }) else {
        return false;
    };
    let mut locked_groups = entry.groups.clone();
    let mut candidate_groups = candidate.groups.clone();
    let mut locked_dependencies = entry.dependencies.clone();
    let mut candidate_dependencies = candidate.dependencies.clone();
    for values in [
        &mut locked_groups,
        &mut candidate_groups,
        &mut locked_dependencies,
        &mut candidate_dependencies,
    ] {
        values.sort();
        values.dedup();
    }
    if !origins_accept_same_resolution(
        &entry.origin.resolved_against(&state.project_root),
        &candidate.origin,
        &candidate.prepared.resolved().version,
    ) || entry.resolved != *candidate.prepared.resolved()
        || locked_groups != candidate_groups
        || locked_dependencies != candidate_dependencies
    {
        return false;
    }
    let installed = state
        .service
        .config()
        .skill_storage_path
        .join(candidate.prepared.id());
    match &candidate.origin {
        Origin::Local {
            path,
            editable: true,
        } => installed
            .canonicalize()
            .ok()
            .zip(path.canonicalize().ok())
            .is_some_and(|(installed, source)| installed == source),
        _ => entry.resolved.checksum.as_ref().is_some_and(|expected| {
            crate::core::install::content_digest(&installed).is_ok_and(|actual| &actual == expected)
        }),
    }
}

fn graph_closure(
    roots: impl IntoIterator<Item = String>,
    graph: &HashMap<String, Vec<String>>,
) -> BTreeSet<String> {
    let mut included: BTreeSet<_> = roots.into_iter().collect();
    loop {
        let before = included.len();
        for id in included.clone() {
            if let Some(dependencies) = graph.get(&id) {
                included.extend(dependencies.iter().cloned());
            }
        }
        if included.len() == before {
            return included;
        }
    }
}

fn append_update_errors(
    results: &mut Vec<SkillUpdateResult>,
    roots: &[UpdateRoot],
    error: &ServiceError,
) {
    results.extend(roots.iter().map(|root| SkillUpdateResult {
        id: root.entry.id.clone(),
        outcome: "error".to_string(),
        reason: Some(error.to_string()),
        resolved_version: None,
    }));
}

fn append_update_outcomes(
    results: &mut Vec<SkillUpdateResult>,
    roots: &[UpdateRoot],
    changed: &BTreeSet<String>,
    versions: &HashMap<String, String>,
    check: bool,
) {
    results.extend(roots.iter().map(|root| {
        let changed = changed.contains(&root.entry.id);
        SkillUpdateResult {
            id: root.entry.id.clone(),
            outcome: if changed {
                if check {
                    "would_update"
                } else {
                    "updated"
                }
            } else {
                "up_to_date"
            }
            .to_string(),
            reason: None,
            resolved_version: versions.get(&root.entry.id).cloned(),
        }
    }));
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

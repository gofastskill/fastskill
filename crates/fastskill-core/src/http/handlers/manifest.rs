//! Manifest (skill-project.toml) endpoint handlers

use crate::core::lock::ProjectSkillsLock;
use crate::core::manifest::{
    DependenciesSection, DependencySpec, SkillProjectToml, MANIFEST_SCHEMA_VERSION,
};
use crate::core::origin::Origin;
use crate::core::project_state::save_project_preserving;
use crate::core::repository::RepositoryManager;
use crate::core::sources::{MarketplaceSkill, SourcesManager};
use crate::core::state_guard::StateMutationGuard;
use crate::core::version::VersionConstraint;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    Json,
};
use std::collections::HashMap;

fn load_project(path: &std::path::Path) -> Result<SkillProjectToml, HttpError> {
    SkillProjectToml::load_from_file(path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
    })
}

fn save_project(project: &SkillProjectToml, path: &std::path::Path) -> Result<(), HttpError> {
    save_project_preserving(path, project).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to save skill-project.toml: {}", e))
    })
}

fn validate_groups(groups: &[String]) -> Result<(), HttpError> {
    if groups.iter().any(|group| group.trim().is_empty()) {
        return Err(HttpError::BadRequest(
            "Group names must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn read_optional_document(path: &std::path::Path) -> Result<Option<Vec<u8>>, HttpError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(HttpError::InternalServerError(format!(
            "Failed to read {}: {error}",
            path.display()
        ))),
    }
}

fn ensure_document_unchanged(
    path: &std::path::Path,
    expected: &Option<Vec<u8>>,
) -> Result<(), HttpError> {
    if &read_optional_document(path)? == expected {
        Ok(())
    } else {
        Err(HttpError::Conflict(
            "Project state changed while the operation was being prepared; retry the request"
                .to_string(),
        ))
    }
}

/// Get repositories from skill-project.toml using the canonical From impl.
pub(crate) fn get_repositories(
    project: &SkillProjectToml,
) -> Vec<crate::core::repository::RepositoryDefinition> {
    project
        .tool
        .as_ref()
        .and_then(|t| t.fastskill.as_ref())
        .and_then(|f| f.repositories.as_ref())
        .map(|repos| {
            repos
                .iter()
                .map(crate::core::repository::RepositoryDefinition::from)
                .collect()
        })
        .unwrap_or_default()
}

fn dep_source_type(spec: &DependencySpec) -> &'static str {
    match spec {
        DependencySpec::Version(_) => "source",
        DependencySpec::Inline { origin, .. } => match origin {
            Origin::Git { .. } => "git",
            Origin::Local { .. } => "local",
            Origin::ZipUrl { .. } => "zip-url",
            Origin::Repository { .. } => "source",
        },
    }
}

/// Human-readable location string for an `Origin`, used in the `/api/project` view.
fn origin_location(origin: &Origin) -> String {
    match origin {
        Origin::Git { url, r#ref, .. } => match r#ref {
            crate::core::origin::GitRef::Branch(b) => format!("{} (branch: {})", url, b),
            crate::core::origin::GitRef::Tag(t) => format!("{} (tag: {})", url, t),
            crate::core::origin::GitRef::Commit(c) => format!("{} (commit: {})", url, c),
            crate::core::origin::GitRef::Default => url.clone(),
        },
        Origin::Local { path, .. } => path.to_string_lossy().to_string(),
        Origin::ZipUrl { url } => url.clone(),
        Origin::Repository { repo, skill, .. } => format!("{} / {}", repo, skill),
    }
}

/// The declared version for a dependency, if any. Only `Origin::Repository`
/// carries a version constraint (ADR-0004); other origins install "whatever
/// is at that location" and have no separate version concept.
fn origin_version(origin: &Origin) -> Option<String> {
    match origin {
        Origin::Repository { version, .. } => version.as_ref().map(|v| v.to_string()),
        _ => None,
    }
}

/// GET /api/project - Full skill-project.toml view (metadata, skills_directory, skills with type/location)
pub async fn get_project(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
    state.require_project_scope()?;
    let project_path = &state.project_file_path;

    if !project_path.exists() {
        return Ok(Json(ApiResponse::success(serde_json::json!({
            "metadata": null,
            "skills_directory": null,
            "skills": []
        }))));
    }

    let project = load_project(project_path)?;

    let metadata = project.metadata.as_ref().map(|m| {
        serde_json::json!({
            "id": m.id,
            "version": m.version,
            "description": m.description,
            "author": m.author,
            "name": m.name
        })
    });

    // Use skills_directory from validated config loaded at server startup
    let skills_directory = state.skills_directory.to_string_lossy().to_string();

    let skills: Vec<serde_json::Value> = project
        .dependencies
        .as_ref()
        .map(|deps| {
            deps.dependencies
                .iter()
                .map(|(id, spec)| {
                    let (typ, location) = match spec {
                        DependencySpec::Version(v) => {
                            (dep_source_type(spec).to_string(), format!("version {}", v))
                        }
                        DependencySpec::Inline { origin, .. } => {
                            (dep_source_type(spec).to_string(), origin_location(origin))
                        }
                    };
                    serde_json::json!({ "id": id, "type": typ, "location": location })
                })
                .collect()
        })
        .unwrap_or_default();

    let data = serde_json::json!({
        "metadata": metadata,
        "skills_directory": skills_directory,
        "skills": skills
    });

    Ok(Json(ApiResponse::success(data)))
}

/// GET /api/manifest/skills - List all skills from skill-project.toml
pub async fn list_manifest_skills(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<Vec<ManifestSkillResponse>>>> {
    state.require_project_scope()?;
    let project_path = &state.project_file_path;

    // Load project
    let project = if project_path.exists() {
        load_project(project_path)?
    } else {
        // Return empty list if project doesn't exist
        return Ok(Json(ApiResponse::success(Vec::new())));
    };

    let manifest_dir = project_path.parent().unwrap_or(std::path::Path::new("."));
    let lock = ProjectSkillsLock::load_from_file(&state.project_root.join("skills.lock")).ok();
    let skills: Vec<ManifestSkillResponse> = project
        .dependencies
        .as_ref()
        .map(|deps| {
            deps.dependencies
                .iter()
                .map(|(id, spec)| {
                    let (version, source_type) = match spec {
                        DependencySpec::Version(v) => {
                            (Some(v.clone()), dep_source_type(spec).to_string())
                        }
                        DependencySpec::Inline { origin, .. } => {
                            (origin_version(origin), dep_source_type(spec).to_string())
                        }
                    };
                    let (groups, editable) = match spec {
                        DependencySpec::Inline { groups, origin } => (
                            groups.clone().unwrap_or_default(),
                            matches!(origin, Origin::Local { editable: true, .. }),
                        ),
                        DependencySpec::Version(_) => (Vec::new(), false),
                    };

                    let reconciliation_required = lock.as_ref().is_none_or(|lock| {
                        let Some(locked) = lock.skills.iter().find(|entry| entry.id == *id) else {
                            return true;
                        };
                        !lock.covered_roots.contains(id)
                            || locked.origin.resolved_against(manifest_dir)
                                != dependency_origin(spec, id).resolved_against(manifest_dir)
                            || locked.groups != groups
                    });
                    ManifestSkillResponse {
                        id: id.clone(),
                        version,
                        groups,
                        editable,
                        source_type,
                        reconciliation_required,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(ApiResponse::success(skills)))
}

fn dependency_origin(spec: &DependencySpec, id: &str) -> Origin {
    match spec {
        DependencySpec::Version(version) => Origin::Repository {
            repo: "default".to_string(),
            skill: id.to_string(),
            version: VersionConstraint::parse(version).ok(),
        },
        DependencySpec::Inline { origin, .. } => origin.clone(),
    }
}

/// POST /api/manifest/skills - Add skill to skill-project.toml
pub async fn add_skill_to_manifest(
    State(state): State<AppState>,
    Json(request): Json<AddSkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<ManifestSkillResponse>>> {
    state.require_project_scope()?;
    let project_path = &state.project_file_path;
    let planned_document = read_optional_document(project_path)?;
    let groups = request.groups.unwrap_or_default();
    validate_groups(&groups)?;
    if request.editable.unwrap_or(false) {
        return Err(HttpError::BadRequest(
            "editable applies only to local directory origins".to_string(),
        ));
    }

    // Load project or create new
    let mut project = if project_path.exists() {
        load_project(project_path)?
    } else {
        SkillProjectToml {
            schema_version: Some(MANIFEST_SCHEMA_VERSION.to_string()),
            metadata: None,
            dependencies: None,
            tool: None,
        }
    };

    // Ensure dependencies section exists (covers both new and loaded projects)
    project
        .dependencies
        .get_or_insert_with(|| DependenciesSection {
            dependencies: HashMap::new(),
        });

    // Get repositories and create manager
    let repositories = get_repositories(&project);
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Get sources manager for marketplace-based repositories. spec 008: opts
    // into the on-disk index cache so a cold/offline add resolves a listing
    // already known to `state.service`'s cache without a network round-trip.
    let sources_manager = SourcesManager::from_repositories(&repo_manager)
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .map(|mgr| mgr.with_skill_cache(state.service.skill_cache().clone()));

    // Find the skill in sources
    let _marketplace_skill = if let Some(sources_mgr) = &sources_manager {
        find_skill_in_sources(sources_mgr, &request.skill_id, &request.source_name)
            .await?
            .ok_or_else(|| {
                HttpError::NotFound(format!(
                    "Skill '{}' not found in source '{}'",
                    request.skill_id, request.source_name
                ))
            })?
    } else {
        return Err(HttpError::NotFound(
            "No marketplace sources configured".to_string(),
        ));
    };

    // This endpoint edits desired intent only. Keep the selected repository in
    // the Manifest and leave the version floating; the next install/update
    // records resolved facts after fetching and verification.
    let dep_spec = DependencySpec::Inline {
        origin: Origin::Repository {
            repo: request.source_name.clone(),
            skill: request.skill_id.clone(),
            version: None,
        },
        groups: (!groups.is_empty()).then_some(groups.clone()),
    };

    // Add to dependencies
    if let Some(ref mut deps) = project.dependencies {
        deps.dependencies.insert(request.skill_id.clone(), dep_spec);
    }

    let guard = StateMutationGuard::acquire_for(
        &state.project_root,
        Some(&state.skills_directory),
        "HTTP add desired skill",
    )
    .map_err(HttpError::from)?;
    if let Err(error) = ensure_document_unchanged(project_path, &planned_document) {
        guard.recovered().map_err(HttpError::from)?;
        return Err(error);
    }
    if let Err(error) = save_project(&project, project_path) {
        guard.recovered().map_err(HttpError::from)?;
        return Err(error);
    }
    guard.commit().map_err(HttpError::from)?;

    let response = ManifestSkillResponse {
        id: request.skill_id.clone(),
        version: None,
        groups,
        editable: false,
        source_type: "source".to_string(),
        reconciliation_required: true,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// DELETE /api/manifest/skills/:id - Remove skill from skill-project.toml
pub async fn remove_skill_from_manifest(
    Path(skill_id): Path<String>,
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
    state.require_project_scope()?;
    let project_path = &state.project_file_path;

    if !project_path.exists() {
        return Err(HttpError::NotFound(
            "skill-project.toml not found".to_string(),
        ));
    }

    let result = super::skills::remove_project_skill(&state, skill_id).await?;
    Ok(Json(ApiResponse::success(result)))
}

/// PUT /api/manifest/skills/:id - Update skill in skill-project.toml
pub async fn update_skill_in_manifest(
    Path(skill_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdateSkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<ManifestSkillResponse>>> {
    state.require_project_scope()?;
    let project_path = &state.project_file_path;
    let planned_document = read_optional_document(project_path)?;

    if !project_path.exists() {
        return Err(HttpError::NotFound(
            "skill-project.toml not found".to_string(),
        ));
    }

    let mut project = load_project(project_path)?;

    if let Some(groups) = request.groups.as_ref() {
        validate_groups(groups)?;
    }

    let (updated_version, updated_groups, updated_editable, source_type) =
        if let Some(ref mut deps) = project.dependencies {
            if let Some(dep_spec) = deps.dependencies.get_mut(&skill_id) {
                apply_manifest_patch(dep_spec, &request)?;
                response_fields(dep_spec)
            } else {
                return Err(HttpError::NotFound(format!(
                    "Skill '{}' not found in project",
                    skill_id
                )));
            }
        } else {
            return Err(HttpError::NotFound(format!(
                "Skill '{}' not found in project",
                skill_id
            )));
        };

    let guard = StateMutationGuard::acquire_for(
        &state.project_root,
        Some(&state.skills_directory),
        "HTTP update desired skill",
    )
    .map_err(HttpError::from)?;
    if let Err(error) = ensure_document_unchanged(project_path, &planned_document) {
        guard.recovered().map_err(HttpError::from)?;
        return Err(error);
    }
    if let Err(error) = save_project(&project, project_path) {
        guard.recovered().map_err(HttpError::from)?;
        return Err(error);
    }
    guard.commit().map_err(HttpError::from)?;

    let response = ManifestSkillResponse {
        id: skill_id,
        version: updated_version,
        groups: updated_groups,
        editable: updated_editable,
        source_type,
        reconciliation_required: true,
    };

    Ok(Json(ApiResponse::success(response)))
}

fn apply_manifest_patch(
    spec: &mut DependencySpec,
    request: &UpdateSkillRequest,
) -> Result<(), HttpError> {
    if let Some(version) = request.version.as_ref() {
        let constraint = VersionConstraint::parse(version)
            .map_err(|error| HttpError::BadRequest(format!("Invalid version: {error}")))?;
        if constraint.as_exact().is_none() {
            return Err(HttpError::BadRequest(
                "Manifest version updates require an exact version".to_string(),
            ));
        }
        match spec {
            DependencySpec::Version(current) => *current = version.clone(),
            DependencySpec::Inline {
                origin:
                    Origin::Repository {
                        version: current, ..
                    },
                ..
            } => *current = Some(constraint),
            DependencySpec::Inline { .. } => {
                return Err(HttpError::BadRequest(
                    "Version updates apply only to repository-origin skills".to_string(),
                ));
            }
        }
    }

    if let Some(groups) = request.groups.as_ref() {
        match spec {
            DependencySpec::Inline {
                groups: current, ..
            } => *current = (!groups.is_empty()).then_some(groups.clone()),
            DependencySpec::Version(_) => {
                return Err(HttpError::BadRequest(
                    "Groups require an explicit repository origin in the Manifest".to_string(),
                ));
            }
        }
    }

    if let Some(editable) = request.editable.as_ref() {
        match spec {
            DependencySpec::Inline {
                origin: Origin::Local {
                    editable: current, ..
                },
                ..
            } => *current = *editable,
            _ => {
                return Err(HttpError::BadRequest(
                    "editable applies only to local directory origins".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn response_fields(spec: &DependencySpec) -> (Option<String>, Vec<String>, bool, String) {
    match spec {
        DependencySpec::Version(version) => (
            Some(version.clone()),
            Vec::new(),
            false,
            "source".to_string(),
        ),
        DependencySpec::Inline { origin, groups } => (
            origin_version(origin),
            groups.clone().unwrap_or_default(),
            matches!(origin, Origin::Local { editable: true, .. }),
            dep_source_type(spec).to_string(),
        ),
    }
}

/// Helper function to find skill in sources
async fn find_skill_in_sources(
    sources_manager: &SourcesManager,
    skill_id: &str,
    source_name: &str,
) -> Result<Option<MarketplaceSkill>, HttpError> {
    let marketplace = sources_manager
        .get_marketplace_json(source_name)
        .await
        .map_err(|error| {
            HttpError::ServiceUnavailable(format!(
                "Failed to acquire repository source '{source_name}': {error}"
            ))
        })?;
    Ok(marketplace.skills.into_iter().find(|s| s.id == skill_id))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::origin::GitRef;
    use tempfile::TempDir;

    fn patch(json: &str) -> UpdateSkillRequest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn document_and_group_guards_report_stale_or_invalid_input() {
        let project = TempDir::new().unwrap();
        let path = project.path().join("skill-project.toml");
        assert_eq!(read_optional_document(&path).unwrap(), None);
        let expected = None;
        ensure_document_unchanged(&path, &expected).unwrap();
        std::fs::write(&path, "[dependencies]\n").unwrap();
        assert!(matches!(
            ensure_document_unchanged(&path, &expected),
            Err(HttpError::Conflict(_))
        ));
        assert!(matches!(
            validate_groups(&[" ".to_string()]),
            Err(HttpError::BadRequest(_))
        ));
    }

    #[test]
    fn origin_display_and_dependency_types_cover_every_variant() {
        let git = |r#ref| Origin::Git {
            url: "https://example.invalid/repo.git".to_string(),
            r#ref,
            subdir: None,
        };
        assert!(origin_location(&git(GitRef::Branch("main".into()))).contains("branch"));
        assert!(origin_location(&git(GitRef::Tag("v1".into()))).contains("tag"));
        assert!(origin_location(&git(GitRef::Commit("abc".into()))).contains("commit"));
        assert_eq!(
            origin_location(&git(GitRef::Default)),
            "https://example.invalid/repo.git"
        );
        let version = DependencySpec::Version("1.2.3".to_string());
        assert_eq!(dep_source_type(&version), "source");
        let resolved = dependency_origin(&version, "demo");
        assert_eq!(origin_version(&resolved).unwrap(), "=1.2.3");
        for (origin, expected) in [
            (
                Origin::Local {
                    path: "demo".into(),
                    editable: false,
                },
                "local",
            ),
            (
                Origin::ZipUrl {
                    url: "https://example.invalid/demo.zip".into(),
                },
                "zip-url",
            ),
            (git(GitRef::Default), "git"),
        ] {
            let spec = DependencySpec::Inline {
                origin,
                groups: None,
            };
            assert_eq!(dep_source_type(&spec), expected);
        }
    }

    #[test]
    fn manifest_patch_enforces_origin_specific_fields() {
        let mut version = DependencySpec::Version("1.0.0".to_string());
        apply_manifest_patch(&mut version, &patch(r#"{"version":"2.0.0"}"#)).unwrap();
        assert_eq!(response_fields(&version).0.as_deref(), Some("2.0.0"));
        assert!(apply_manifest_patch(&mut version, &patch(r#"{"version":"^2"}"#)).is_err());
        assert!(apply_manifest_patch(&mut version, &patch(r#"{"groups":["dev"]}"#)).is_err());
        assert!(apply_manifest_patch(&mut version, &patch(r#"{"editable":true}"#)).is_err());

        let mut repository = DependencySpec::Inline {
            origin: Origin::Repository {
                repo: "default".to_string(),
                skill: "demo".to_string(),
                version: None,
            },
            groups: None,
        };
        apply_manifest_patch(
            &mut repository,
            &patch(r#"{"version":"2.0.0","groups":["dev"]}"#),
        )
        .unwrap();
        let fields = response_fields(&repository);
        assert_eq!(fields.0.as_deref(), Some("=2.0.0"));
        assert_eq!(fields.1, ["dev"]);

        let mut local = DependencySpec::Inline {
            origin: Origin::Local {
                path: "demo".into(),
                editable: false,
            },
            groups: None,
        };
        apply_manifest_patch(&mut local, &patch(r#"{"editable":true}"#)).unwrap();
        assert!(response_fields(&local).2);
        assert!(apply_manifest_patch(&mut local, &patch(r#"{"version":"2.0.0"}"#)).is_err());
    }
}

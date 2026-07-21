//! Manifest (skill-project.toml) endpoint handlers

use crate::core::manifest::{DependenciesSection, DependencySpec, SkillProjectToml};
use crate::core::origin::Origin;
use crate::core::repository::RepositoryManager;
use crate::core::sources::{MarketplaceSkill, SourcesManager};
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    Json,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn load_project(path: &std::path::Path) -> Result<SkillProjectToml, HttpError> {
    SkillProjectToml::load_from_file(path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
    })
}

fn save_project(project: &SkillProjectToml, path: &std::path::Path) -> Result<(), HttpError> {
    project.save_to_file(path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to save skill-project.toml: {}", e))
    })
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

/// Get lock file path from project path
/// Returns the canonicalized lock path to prevent path traversal
fn get_lock_path(project_path: &std::path::Path) -> Result<PathBuf, HttpError> {
    let lock_path = if let Some(parent) = project_path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };

    // Canonicalize if it exists, otherwise return the path as-is for creation
    if lock_path.exists() {
        lock_path.canonicalize().map_err(|e| {
            HttpError::InternalServerError(format!("Failed to resolve lock path: {}", e))
        })
    } else {
        Ok(lock_path)
    }
}

/// GET /api/project - Full skill-project.toml view (metadata, skills_directory, skills with type/location)
pub async fn get_project(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<serde_json::Value>>> {
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
    let project_path = &state.project_file_path;

    // Load project
    let project = if project_path.exists() {
        load_project(project_path)?
    } else {
        // Return empty list if project doesn't exist
        return Ok(Json(ApiResponse::success(Vec::new())));
    };

    let skills: Vec<ManifestSkillResponse> = project
        .dependencies
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

                    ManifestSkillResponse {
                        id: id.clone(),
                        version,
                        groups,
                        editable,
                        source_type,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(ApiResponse::success(skills)))
}

/// POST /api/manifest/skills - Add skill to skill-project.toml
pub async fn add_skill_to_manifest(
    State(state): State<AppState>,
    Json(request): Json<AddSkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<ManifestSkillResponse>>> {
    let project_path = &state.project_file_path;

    // Load project or create new
    let mut project = if project_path.exists() {
        load_project(project_path)?
    } else {
        // Ensure parent directory exists
        if let Some(parent) = project_path.parent() {
            // Validate parent path to prevent traversal
            let safe_parent = if parent.exists() {
                parent.canonicalize().map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to resolve parent path: {}", e))
                })?
            } else {
                parent.to_path_buf()
            };
            std::fs::create_dir_all(&safe_parent).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to create directory: {}", e))
            })?;
        }

        SkillProjectToml {
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

    // Get sources manager for marketplace-based repositories
    let sources_manager = SourcesManager::from_repositories(&repo_manager)
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    // Find the skill in sources
    let marketplace_skill = if let Some(sources_mgr) = &sources_manager {
        find_skill_in_sources(sources_mgr, &request.skill_id, &request.source_name)
            .await
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

    // Create dependency spec - simple version for now
    let dep_spec = DependencySpec::Version(marketplace_skill.version.clone());

    // Add to dependencies
    if let Some(ref mut deps) = project.dependencies {
        deps.dependencies.insert(request.skill_id.clone(), dep_spec);
    }

    // Save project
    save_project(&project, project_path)?;

    let response = ManifestSkillResponse {
        id: request.skill_id.clone(),
        version: Some(marketplace_skill.version),
        groups: request.groups.unwrap_or_default(),
        editable: request.editable.unwrap_or(false),
        source_type: "source".to_string(),
    };

    Ok(Json(ApiResponse::success(response)))
}

/// DELETE /api/manifest/skills/:id - Remove skill from skill-project.toml
pub async fn remove_skill_from_manifest(
    Path(skill_id): Path<String>,
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<()>>> {
    let project_path = &state.project_file_path;
    let lock_path = get_lock_path(project_path)?;

    if !project_path.exists() {
        return Err(HttpError::NotFound(
            "skill-project.toml not found".to_string(),
        ));
    }

    // Remove from project
    let mut project = load_project(project_path)?;

    if let Some(ref mut deps) = project.dependencies {
        deps.dependencies.remove(&skill_id);
    }

    save_project(&project, project_path)?;

    // Remove from lock file if it exists
    if lock_path.exists() {
        use crate::core::lock::ProjectSkillsLock;
        let mut lock = ProjectSkillsLock::load_from_file(&lock_path).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to load lock file: {}", e))
        })?;
        lock.remove_skill(&skill_id);
        lock.save_to_file(&lock_path).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to save lock file: {}", e))
        })?;
    }

    Ok(Json(ApiResponse::success(())))
}

/// PUT /api/manifest/skills/:id - Update skill in skill-project.toml
pub async fn update_skill_in_manifest(
    Path(skill_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdateSkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<ManifestSkillResponse>>> {
    let project_path = &state.project_file_path;

    if !project_path.exists() {
        return Err(HttpError::NotFound(
            "skill-project.toml not found".to_string(),
        ));
    }

    let mut project = load_project(project_path)?;

    // Find and update dependency
    let updated_version = if let Some(ref mut deps) = project.dependencies {
        if let Some(dep_spec) = deps.dependencies.get_mut(&skill_id) {
            // Update version if provided
            if let Some(ref version) = request.version {
                *dep_spec = DependencySpec::Version(version.clone());
            }
            match dep_spec {
                DependencySpec::Version(v) => Some(v.clone()),
                _ => None,
            }
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

    // Save project
    save_project(&project, project_path)?;

    let response = ManifestSkillResponse {
        id: skill_id,
        version: request.version.or(updated_version),
        groups: request.groups.unwrap_or_default(),
        editable: request.editable.unwrap_or(false),
        source_type: "source".to_string(),
    };

    Ok(Json(ApiResponse::success(response)))
}

/// Helper function to find skill in sources
async fn find_skill_in_sources(
    sources_manager: &SourcesManager,
    skill_id: &str,
    source_name: &str,
) -> Option<MarketplaceSkill> {
    let marketplace = sources_manager
        .get_marketplace_json(source_name)
        .await
        .ok()?;
    marketplace.skills.into_iter().find(|s| s.id == skill_id)
}

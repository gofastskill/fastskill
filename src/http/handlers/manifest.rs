//! Manifest (skill-project.toml) endpoint handlers

use crate::core::manifest::{
    DependenciesSection, DependencySource, DependencySpec, SkillProjectToml,
};
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

/// Get repositories from skill-project.toml
pub(crate) fn get_repositories(
    project: &SkillProjectToml,
) -> Vec<crate::core::repository::RepositoryDefinition> {
    use crate::core::repository::{
        RepositoryAuth, RepositoryConfig, RepositoryDefinition, RepositoryType,
    };

    if let Some(tool) = &project.tool {
        if let Some(fastskill_config) = &tool.fastskill {
            if let Some(repos) = &fastskill_config.repositories {
                return repos
                    .iter()
                    .map(|r| {
                        let repo_type = match r.r#type {
                            crate::core::manifest::RepositoryType::HttpRegistry => {
                                RepositoryType::HttpRegistry
                            }
                            crate::core::manifest::RepositoryType::GitMarketplace => {
                                RepositoryType::GitMarketplace
                            }
                            crate::core::manifest::RepositoryType::ZipUrl => RepositoryType::ZipUrl,
                            crate::core::manifest::RepositoryType::Local => RepositoryType::Local,
                        };

                        let config = match &r.connection {
                            crate::core::manifest::RepositoryConnection::HttpRegistry {
                                index_url,
                            } => RepositoryConfig::HttpRegistry {
                                index_url: index_url.clone(),
                            },
                            crate::core::manifest::RepositoryConnection::GitMarketplace {
                                url,
                                branch,
                            } => RepositoryConfig::GitMarketplace {
                                url: url.clone(),
                                branch: branch.clone(),
                                tag: None,
                            },
                            crate::core::manifest::RepositoryConnection::ZipUrl { zip_url } => {
                                RepositoryConfig::ZipUrl {
                                    base_url: zip_url.clone(),
                                }
                            }
                            crate::core::manifest::RepositoryConnection::Local { path } => {
                                RepositoryConfig::Local {
                                    path: std::path::PathBuf::from(path),
                                }
                            }
                        };

                        let auth = r.auth.as_ref().map(|a| match a.r#type {
                            crate::core::manifest::AuthType::Pat => RepositoryAuth::Pat {
                                env_var: a
                                    .env_var
                                    .clone()
                                    .unwrap_or_else(|| "PAT_TOKEN".to_string()),
                            },
                        });

                        RepositoryDefinition {
                            name: r.name.clone(),
                            repo_type,
                            priority: r.priority,
                            config,
                            auth,
                            storage: None,
                        }
                    })
                    .collect();
            }
        }
    }
    Vec::new()
}

/// Get lock file path from project path
fn get_lock_path(project_path: &std::path::Path) -> PathBuf {
    if let Some(parent) = project_path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
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

    let project = SkillProjectToml::load_from_file(project_path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
    })?;

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
                            ("source".to_string(), format!("version {}", v))
                        }
                        DependencySpec::Inline {
                            source,
                            source_specific,
                            ..
                        } => {
                            let typ = match source {
                                DependencySource::Git => "git",
                                DependencySource::Local => "local",
                                DependencySource::ZipUrl => "zip-url",
                                DependencySource::Source => "source",
                            }
                            .to_string();
                            let location = match source {
                                DependencySource::Git => source_specific
                                    .url
                                    .clone()
                                    .map(|u| {
                                        source_specific
                                            .branch
                                            .as_ref()
                                            .map(|b| format!("{} (branch: {})", u, b))
                                            .unwrap_or(u)
                                    })
                                    .unwrap_or_else(|| "—".to_string()),
                                DependencySource::Local => source_specific
                                    .path
                                    .clone()
                                    .unwrap_or_else(|| "—".to_string()),
                                DependencySource::ZipUrl => source_specific
                                    .zip_url
                                    .clone()
                                    .unwrap_or_else(|| "—".to_string()),
                                DependencySource::Source => source_specific
                                    .name
                                    .as_ref()
                                    .zip(source_specific.skill.as_ref())
                                    .map(|(n, s)| format!("{} / {}", n, s))
                                    .unwrap_or_else(|| "—".to_string()),
                            };
                            (typ, location)
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
        SkillProjectToml::load_from_file(project_path).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
        })?
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
                        DependencySpec::Version(v) => (Some(v.clone()), "source"),
                        DependencySpec::Inline {
                            source,
                            source_specific,
                            ..
                        } => {
                            let stype = match source {
                                DependencySource::Git => "git",
                                DependencySource::Local => "local",
                                DependencySource::ZipUrl => "zip-url",
                                DependencySource::Source => "source",
                            };
                            (source_specific.version.clone(), stype)
                        }
                    };

                    ManifestSkillResponse {
                        id: id.clone(),
                        version,
                        groups: Vec::new(), // TODO: extract from inline spec
                        editable: false,    // TODO: extract from inline spec
                        source_type: source_type.to_string(),
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
    let _lock_path = get_lock_path(project_path);

    // Load project or create new
    let mut project = if project_path.exists() {
        SkillProjectToml::load_from_file(project_path).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
        })?
    } else {
        // Ensure parent directory exists
        if let Some(parent) = project_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to create directory: {}", e))
            })?;
        }

        SkillProjectToml {
            metadata: None,
            dependencies: Some(DependenciesSection {
                dependencies: HashMap::new(),
            }),
            tool: None,
        }
    };

    // Ensure dependencies section exists
    if project.dependencies.is_none() {
        project.dependencies = Some(DependenciesSection {
            dependencies: HashMap::new(),
        });
    }

    // Get repositories and create manager
    let repositories = get_repositories(&project);
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Get sources manager for marketplace-based repositories
    let sources_manager = create_sources_manager_from_repositories(&repo_manager)?;

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
    project.save_to_file(project_path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to save skill-project.toml: {}", e))
    })?;

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
    let lock_path = get_lock_path(project_path);

    if !project_path.exists() {
        return Err(HttpError::NotFound(
            "skill-project.toml not found".to_string(),
        ));
    }

    // Remove from project
    let mut project = SkillProjectToml::load_from_file(project_path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
    })?;

    if let Some(ref mut deps) = project.dependencies {
        deps.dependencies.remove(&skill_id);
    }

    project.save_to_file(project_path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to save skill-project.toml: {}", e))
    })?;

    // Remove from lock file if it exists
    if lock_path.exists() {
        use crate::core::lock::SkillsLock;
        let mut lock = SkillsLock::load_from_file(&lock_path).map_err(|e| {
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

    let mut project = SkillProjectToml::load_from_file(project_path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to load skill-project.toml: {}", e))
    })?;

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
    project.save_to_file(project_path).map_err(|e| {
        HttpError::InternalServerError(format!("Failed to save skill-project.toml: {}", e))
    })?;

    let response = ManifestSkillResponse {
        id: skill_id,
        version: request.version.or(updated_version),
        groups: request.groups.unwrap_or_default(),
        editable: request.editable.unwrap_or(false),
        source_type: "source".to_string(),
    };

    Ok(Json(ApiResponse::success(response)))
}

/// POST /api/manifest/generate-mdc - Generate skills.mdc file
pub async fn generate_mdc(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<()>>> {
    generate_skills_mdc(&state).await.map_err(|e| {
        HttpError::InternalServerError(format!("Failed to generate skills.mdc: {}", e))
    })?;

    Ok(Json(ApiResponse::success(())))
}

/// Helper function to create sources manager from repository manager
fn create_sources_manager_from_repositories(
    _repo_manager: &RepositoryManager,
) -> Result<Option<SourcesManager>, HttpError> {
    // For now, create an empty sources manager - in future can be enhanced to convert repositories
    // This is a simplified implementation
    Ok(None)
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

/// Helper function to generate skills.mdc
async fn generate_skills_mdc(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let skills_dir = state.service.config().skill_storage_path.clone();

    // Find workspace root by walking up from skill-project.toml path
    let project_path = &state.project_file_path;
    let workspace_root = project_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| std::path::Path::new("."));

    let output_file = workspace_root
        .join(".cursor")
        .join("rules")
        .join("skills.mdc");

    // Ensure output directory exists
    if let Some(parent) = output_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Simple Rust implementation: scan skills directory and generate markdown
    let mut output = String::from("---\nalwaysApply: true\n---\n# Skills Registry\n\n");
    output.push_str("Skills are modular packages in `<skills-directory>/<category>/<skill-name>/SKILL.md` that provide specialized workflows, tool integrations, and domain knowledge. Each SKILL.md contains YAML frontmatter (shown here: name, description) and full instructions with optional scripts/references/assets. Use the `description` field to identify relevant skills, then read the full SKILL.md at the path shown.\n\n");

    // Find all SKILL.md files
    let skill_files = find_skill_files(&skills_dir)?;

    if skill_files.is_empty() {
        output.push_str("No skills found.\n");
    } else {
        let mut current_category = String::new();

        for skill_file in skill_files {
            let category = extract_category(&skill_file, &skills_dir);

            // Extract relative path from workspace root
            let relative_path_str = if let Ok(rel) = skill_file.strip_prefix(workspace_root) {
                rel.to_string_lossy().to_string()
            } else if let Ok(rel) = skill_file.strip_prefix(&skills_dir) {
                // Fallback: relative to skills directory
                let skills_dir_name = skills_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "skills".to_string());
                format!("{}/{}", skills_dir_name, rel.to_string_lossy())
            } else {
                skill_file.to_string_lossy().to_string()
            };

            // Print category header if it's a new category
            if category != current_category {
                if !current_category.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("## {}\n\n", category));
                current_category = category.clone();
            }

            // Print skill path header
            output.push_str(&format!("### {}\n\n", relative_path_str));

            // Extract and include YAML frontmatter
            if let Ok(frontmatter) = extract_frontmatter(&skill_file) {
                output.push_str(&frontmatter);
                output.push('\n');
            }
        }
    }

    std::fs::write(&output_file, output)?;

    Ok(())
}

/// Find all SKILL.md files in the skills directory
fn find_skill_files(skills_dir: &PathBuf) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();

    if !skills_dir.exists() {
        return Ok(files);
    }

    for entry in std::fs::read_dir(skills_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            files.extend(find_skill_files(&path)?);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

/// Extract category from skill path
fn extract_category(skill_file: &std::path::Path, skills_dir: &std::path::Path) -> String {
    // Get relative path from skills directory
    if let Ok(rel_path) = skill_file.strip_prefix(skills_dir) {
        let components: Vec<_> = rel_path.components().collect();
        // First component after skills_dir should be the category
        if let Some(std::path::Component::Normal(category)) = components.first() {
            return category.to_string_lossy().to_string();
        }
    }

    "uncategorized".to_string()
}

/// Extract YAML frontmatter from SKILL.md file
fn extract_frontmatter(file_path: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(file_path)?;
    let lines = content.lines();
    let mut frontmatter = String::from("---\n");
    let mut in_frontmatter = false;
    let mut found_start = false;

    for line in lines {
        if line == "---" {
            if !found_start {
                found_start = true;
                in_frontmatter = true;
                continue;
            } else {
                frontmatter.push_str("---\n");
                break;
            }
        }

        if in_frontmatter {
            frontmatter.push_str(line);
            frontmatter.push('\n');
        }
    }

    Ok(frontmatter)
}

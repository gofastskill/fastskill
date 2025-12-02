//! Registry endpoint handlers

use crate::core::repository::RepositoryManager;
use crate::core::sources::{MarketplaceJson, SourceConfig, SourceDefinition, SourcesManager};
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    Json,
};
use std::collections::HashSet;

/// Get repository manager from service config
fn get_repository_manager(service: &crate::core::service::FastSkillService) -> RepositoryManager {
    let config = service.config();
    // Repository config is typically in the parent directory of skills directory
    let repos_config_path = config
        .skill_storage_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(".claude")
        .join("repositories.toml");

    let mut manager = RepositoryManager::new(repos_config_path);
    let _ = manager.load(); // Try to load, ignore errors
    manager
}

/// Get sources manager from repository manager (for marketplace-based repositories)
/// This creates a temporary SourcesManager from marketplace-based repositories
async fn get_sources_manager_from_repos(
    repo_manager: &RepositoryManager,
) -> Result<SourcesManager, String> {
    use crate::core::repository::{RepositoryConfig, RepositoryType};
    use crate::core::sources::{SourceAuth, SourceConfig, SourceDefinition};

    let repos = repo_manager.list_repositories();
    let mut marketplace_sources = Vec::new();

    for repo in repos {
        // Only include marketplace-based repositories
        let source_config = match &repo.repo_type {
            RepositoryType::GitMarketplace => {
                if let RepositoryConfig::GitMarketplace { url, branch, tag } = &repo.config {
                    let auth = repo.auth.as_ref().and_then(|a| match a {
                        crate::core::repository::RepositoryAuth::Pat { env_var } => {
                            Some(SourceAuth::Pat {
                                env_var: env_var.clone(),
                            })
                        }
                        crate::core::repository::RepositoryAuth::SshKey { path } => {
                            Some(SourceAuth::SshKey { path: path.clone() })
                        }
                        crate::core::repository::RepositoryAuth::Basic {
                            username,
                            password_env,
                        } => Some(SourceAuth::Basic {
                            username: username.clone(),
                            password_env: password_env.clone(),
                        }),
                        _ => None,
                    });

                    Some(SourceConfig::Git {
                        url: url.clone(),
                        branch: branch.clone(),
                        tag: tag.clone(),
                        auth,
                    })
                } else {
                    None
                }
            }
            RepositoryType::ZipUrl => {
                if let RepositoryConfig::ZipUrl { base_url } = &repo.config {
                    let auth = repo.auth.as_ref().and_then(|a| match a {
                        crate::core::repository::RepositoryAuth::Pat { env_var } => {
                            Some(SourceAuth::Pat {
                                env_var: env_var.clone(),
                            })
                        }
                        crate::core::repository::RepositoryAuth::Basic {
                            username,
                            password_env,
                        } => Some(SourceAuth::Basic {
                            username: username.clone(),
                            password_env: password_env.clone(),
                        }),
                        _ => None,
                    });

                    Some(SourceConfig::ZipUrl {
                        base_url: base_url.clone(),
                        auth,
                    })
                } else {
                    None
                }
            }
            RepositoryType::Local => {
                if let RepositoryConfig::Local { path } = &repo.config {
                    Some(SourceConfig::Local { path: path.clone() })
                } else {
                    None
                }
            }
            RepositoryType::HttpRegistry => None, // Http-registry repos don't work with SourcesManager
        };

        if let Some(source_config) = source_config {
            marketplace_sources.push(SourceDefinition {
                name: repo.name.clone(),
                priority: repo.priority,
                source: source_config,
            });
        }
    }

    // Create a temporary SourcesManager with these sources
    let temp_path = std::env::temp_dir().join("fastskill-sources-temp.toml");
    let mut sources_manager = SourcesManager::new(temp_path);

    // Add all sources
    for source_def in marketplace_sources {
        sources_manager
            .add_source_with_priority(
                source_def.name.clone(),
                source_def.source,
                source_def.priority,
            )
            .map_err(|e| format!("Failed to add source: {}", e))?;
    }

    Ok(sources_manager)
}

/// GET /api/registry/sources - List all configured sources/repositories
pub async fn list_sources(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<Vec<SourceResponse>>>> {
    // Get repository manager (supports all formats)
    let repo_manager = get_repository_manager(&state.service);

    let repos = repo_manager.list_repositories();

    let source_responses: Vec<SourceResponse> = repos
        .iter()
        .map(|repo_def| {
            let (source_type, url, path, supports_marketplace) = match &repo_def.config {
                crate::core::repository::RepositoryConfig::GitMarketplace { url, .. } => {
                    ("git-marketplace".to_string(), Some(url.clone()), None, true)
                }
                crate::core::repository::RepositoryConfig::HttpRegistry { index_url } => (
                    "http-registry".to_string(),
                    Some(index_url.clone()),
                    None,
                    false,
                ),
                crate::core::repository::RepositoryConfig::ZipUrl { base_url } => {
                    ("zip-url".to_string(), Some(base_url.clone()), None, true)
                }
                crate::core::repository::RepositoryConfig::Local { path } => (
                    "local".to_string(),
                    None,
                    Some(path.to_string_lossy().to_string()),
                    false,
                ),
            };

            SourceResponse {
                name: repo_def.name.clone(),
                source_type,
                url,
                path,
                supports_marketplace,
            }
        })
        .collect();

    Ok(Json(ApiResponse::success(source_responses)))
}

/// GET /api/registry/skills - Get all skills grouped by source
pub async fn list_all_skills(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<RegistrySkillsResponse>>> {
    let repo_manager = get_repository_manager(&state.service);
    let sources_manager = get_sources_manager_from_repos(&repo_manager)
        .await
        .map_err(|e| {
            HttpError::InternalServerError(format!("Failed to create sources manager: {}", e))
        })?;
    let skill_manager = state.service.skill_manager();

    // Get all installed skills
    let installed_skills = skill_manager
        .list_skills(None)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    let installed_ids: HashSet<String> =
        installed_skills.iter().map(|s| s.id.to_string()).collect();

    // Get skills from all sources
    let sources = sources_manager.list_sources();
    let mut source_responses = Vec::new();
    let mut total_skills = 0;

    for source_def in sources {
        if !source_def.supports_marketplace() {
            tracing::debug!(
                "Skipping source '{}': does not support marketplace",
                source_def.name
            );
            continue;
        }

        match sources_manager.get_marketplace_json(&source_def.name).await {
            Ok(marketplace) => {
                let skills: Vec<MarketplaceSkillResponse> = marketplace
                    .skills
                    .iter()
                    .map(|skill| MarketplaceSkillResponse {
                        id: skill.id.clone(),
                        name: skill.name.clone(),
                        description: skill.description.clone(),
                        version: skill.version.clone(),
                        author: skill.author.clone(),
                        tags: skill.tags.clone(),
                        capabilities: skill.capabilities.clone(),
                        download_url: skill.download_url.clone(),
                        source_name: source_def.name.clone(),
                        installed: installed_ids.contains(&skill.id),
                    })
                    .collect();

                let skills_count = skills.len();
                total_skills += skills_count;

                tracing::info!(
                    "Loaded {} skills from source '{}'",
                    skills_count,
                    source_def.name
                );

                source_responses.push(SourceSkillsResponse {
                    source_name: source_def.name.clone(),
                    skills,
                    count: skills_count,
                });
            }
            Err(e) => {
                // Log the error so it's visible in the console
                tracing::warn!(
                    "Failed to load marketplace.json from source '{}': {}",
                    source_def.name,
                    e
                );
                continue;
            }
        }
    }

    Ok(Json(ApiResponse::success(RegistrySkillsResponse {
        sources: source_responses,
        total_skills,
        total_sources: sources_manager.list_sources().len(),
    })))
}

/// GET /api/registry/sources/:name/skills - Get skills from a specific source
pub async fn list_source_skills(
    Path(source_name): Path<String>,
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<SourceSkillsResponse>>> {
    let repo_manager = get_repository_manager(&state.service);
    let sources_manager = get_sources_manager_from_repos(&repo_manager)
        .await
        .map_err(|e| {
            HttpError::InternalServerError(format!("Failed to create sources manager: {}", e))
        })?;
    let skill_manager = state.service.skill_manager();

    // Get source definition
    let source_def = sources_manager
        .get_source(&source_name)
        .ok_or_else(|| HttpError::NotFound(format!("Source '{}' not found", source_name)))?;

    if !source_def.supports_marketplace() {
        return Err(HttpError::BadRequest(format!(
            "Source '{}' does not support marketplace.json",
            source_name
        )));
    }

    // Get marketplace.json
    let marketplace = sources_manager
        .get_marketplace_json(&source_name)
        .await
        .map_err(|e| {
            HttpError::InternalServerError(format!("Failed to load marketplace: {}", e))
        })?;

    // Get installed skills to check installation status
    let installed_skills = skill_manager
        .list_skills(None)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    let installed_ids: HashSet<String> =
        installed_skills.iter().map(|s| s.id.to_string()).collect();

    let skills: Vec<MarketplaceSkillResponse> = marketplace
        .skills
        .iter()
        .map(|skill| MarketplaceSkillResponse {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            version: skill.version.clone(),
            author: skill.author.clone(),
            tags: skill.tags.clone(),
            capabilities: skill.capabilities.clone(),
            download_url: skill.download_url.clone(),
            source_name: source_def.name.clone(),
            installed: installed_ids.contains(&skill.id),
        })
        .collect();

    let skills_count = skills.len();

    Ok(Json(ApiResponse::success(SourceSkillsResponse {
        source_name: source_def.name.clone(),
        skills,
        count: skills_count,
    })))
}

/// GET /api/registry/sources/:name/marketplace - Get raw marketplace.json
pub async fn get_marketplace(
    Path(source_name): Path<String>,
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<MarketplaceJson>>> {
    let repo_manager = get_repository_manager(&state.service);
    let sources_manager = get_sources_manager_from_repos(&repo_manager)
        .await
        .map_err(|e| {
            HttpError::InternalServerError(format!("Failed to create sources manager: {}", e))
        })?;

    // Get source definition
    let source_def = sources_manager
        .get_source(&source_name)
        .ok_or_else(|| HttpError::NotFound(format!("Source '{}' not found", source_name)))?;

    if !source_def.supports_marketplace() {
        return Err(HttpError::BadRequest(format!(
            "Source '{}' does not support marketplace.json",
            source_name
        )));
    }

    // Get marketplace.json
    let marketplace = sources_manager
        .get_marketplace_json(&source_name)
        .await
        .map_err(|e| {
            HttpError::InternalServerError(format!("Failed to load marketplace: {}", e))
        })?;

    Ok(Json(ApiResponse::success(marketplace)))
}

/// POST /api/registry/refresh - Refresh sources cache
pub async fn refresh_sources(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<RegistrySkillsResponse>>> {
    let repo_manager = get_repository_manager(&state.service);
    let sources_manager = get_sources_manager_from_repos(&repo_manager)
        .await
        .map_err(|e| {
            HttpError::InternalServerError(format!("Failed to create sources manager: {}", e))
        })?;

    // Clear the cache
    sources_manager.clear_cache().await;

    // Call list_all_skills to return refreshed data (it will reload from sources)
    list_all_skills(State(state)).await
}

impl SourceDefinition {
    fn supports_marketplace(&self) -> bool {
        matches!(
            &self.source,
            SourceConfig::Git { .. } | SourceConfig::ZipUrl { .. }
        )
    }
}

/// GET /index/:skill_id - Serve registry index file for a skill (flat layout)
/// This endpoint serves the index file from the registry_index_path
/// Format: /index/{scope}/{skill-name} (e.g., /index/dev-user/test-skill)
pub async fn serve_index_file(
    State(state): State<AppState>,
    Path(skill_id): Path<String>,
) -> HttpResult<axum::response::Response> {
    let config = state.service.config();

    let registry_index_path = config.registry_index_path.as_ref().ok_or_else(|| {
        HttpError::InternalServerError("Registry index path not configured".to_string())
    })?;

    // Use flat layout: skill_id is already in format "scope/skill-name"
    // Security: Normalize paths to prevent directory traversal
    let index_file_path = registry_index_path.join(&skill_id);

    // Canonicalize both paths to resolve any .. or . components
    let canonical_registry_path = registry_index_path.canonicalize().map_err(|e| {
        HttpError::InternalServerError(format!("Failed to canonicalize registry path: {}", e))
    })?;

    let canonical_index_path = index_file_path.canonicalize().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            HttpError::NotFound(format!("Index file not found for skill: {}", skill_id))
        } else {
            HttpError::InternalServerError(format!("Failed to canonicalize index path: {}", e))
        }
    })?;

    // Security: Ensure the canonical path is within the registry_index_path
    if !canonical_index_path.starts_with(&canonical_registry_path) {
        return Err(HttpError::BadRequest(format!(
            "Invalid skill ID: {}",
            skill_id
        )));
    }

    // Read the index file (use canonical path)
    match tokio::fs::read_to_string(&canonical_index_path).await {
        Ok(content) => Ok(axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(content))
            .map_err(|e| {
                HttpError::InternalServerError(format!("Failed to build response: {}", e))
            })?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(HttpError::NotFound(format!(
            "Index file not found for skill: {}",
            skill_id
        ))),
        Err(e) => Err(HttpError::InternalServerError(format!(
            "Failed to read index file: {}",
            e
        ))),
    }
}

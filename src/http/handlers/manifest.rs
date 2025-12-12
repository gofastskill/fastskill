//! Manifest (skills.toml) endpoint handlers

use crate::core::manifest::{SkillEntry, SkillSource, SkillsManifest};
use crate::core::sources::{MarketplaceSkill, SourcesManager};
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    Json,
};
use std::path::PathBuf;

/// Get sources manager from service config
fn get_sources_manager(service: &crate::core::service::FastSkillService) -> SourcesManager {
    let config = service.config();
    // Sources config is typically in the parent directory of skills directory
    let sources_config_path = config
        .skill_storage_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(".claude")
        .join("repositories.toml");

    let mut manager = SourcesManager::new(sources_config_path);
    let _ = manager.load(); // Try to load, ignore errors
    manager
}

/// Get lock file path from manifest path
fn get_lock_path(manifest_path: &std::path::Path) -> PathBuf {
    if let Some(parent) = manifest_path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    }
}

/// GET /api/manifest/skills - List all skills from skills.toml
pub async fn list_manifest_skills(
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<Vec<ManifestSkillResponse>>>> {
    let manifest_path = &state.skills_toml_path;

    // Load manifest
    let manifest = if manifest_path.exists() {
        SkillsManifest::load_from_file(manifest_path).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to load manifest: {}", e))
        })?
    } else {
        // Return empty list if manifest doesn't exist
        return Ok(Json(ApiResponse::success(Vec::new())));
    };

    let skills: Vec<ManifestSkillResponse> = manifest
        .get_all_skills()
        .iter()
        .map(|entry| {
            let source_type = match &entry.source {
                SkillSource::Git { .. } => "git",
                SkillSource::Source { .. } => "source",
                SkillSource::Local { .. } => "local",
                SkillSource::ZipUrl { .. } => "zip-url",
            };

            ManifestSkillResponse {
                id: entry.id.clone(),
                version: entry.version.clone(),
                groups: entry.groups.clone(),
                editable: entry.editable,
                source_type: source_type.to_string(),
            }
        })
        .collect();

    Ok(Json(ApiResponse::success(skills)))
}

/// POST /api/manifest/skills - Add skill to skills.toml
pub async fn add_skill_to_manifest(
    State(state): State<AppState>,
    Json(request): Json<AddSkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<ManifestSkillResponse>>> {
    let manifest_path = &state.skills_toml_path;
    let _lock_path = get_lock_path(manifest_path);

    // Get sources manager to find skill information
    let sources_manager = get_sources_manager(&state.service);

    // Find the skill in sources
    let marketplace_skill =
        find_skill_in_sources(&sources_manager, &request.skill_id, &request.source_name)
            .await
            .ok_or_else(|| {
                HttpError::NotFound(format!(
                    "Skill '{}' not found in source '{}'",
                    request.skill_id, request.source_name
                ))
            })?;

    // Get source definition
    let source_def = sources_manager.get_source(&request.source_name).ok_or_else(|| {
        HttpError::NotFound(format!("Source '{}' not found", request.source_name))
    })?;

    // Create SkillSource from source definition and marketplace skill
    let skill_source = match &source_def.source {
        crate::core::sources::SourceConfig::Git {
            url, branch, tag, ..
        } => SkillSource::Git {
            url: url.clone(),
            branch: branch.clone(),
            tag: tag.clone(),
            subdir: None,
        },
        crate::core::sources::SourceConfig::ZipUrl { base_url, .. } => SkillSource::ZipUrl {
            base_url: base_url.clone(),
            version: Some(marketplace_skill.version.clone()),
        },
        crate::core::sources::SourceConfig::Local { path } => SkillSource::Local {
            path: path.clone(),
            editable: request.editable.unwrap_or(false),
        },
    };

    // Create skill entry
    let skill_entry = SkillEntry {
        id: request.skill_id.clone(),
        source: skill_source,
        version: Some(marketplace_skill.version.clone()),
        groups: request.groups.unwrap_or_default(),
        editable: request.editable.unwrap_or(false),
    };

    // Load or create manifest
    let mut manifest = if manifest_path.exists() {
        SkillsManifest::load_from_file(manifest_path).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to load manifest: {}", e))
        })?
    } else {
        // Ensure parent directory exists
        if let Some(parent) = manifest_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to create directory: {}", e))
            })?;
        }

        SkillsManifest {
            metadata: crate::core::manifest::ManifestMetadata {
                version: "1.0.0".to_string(),
            },
            skills: Vec::new(),
        }
    };

    // Remove existing entry if present
    manifest.remove_skill(&request.skill_id);
    manifest.add_skill(skill_entry.clone());

    // Save manifest
    manifest
        .save_to_file(manifest_path)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to save manifest: {}", e)))?;

    // Generate skills.mdc if enabled
    if state.auto_generate_mdc {
        if let Err(e) = generate_skills_mdc(&state).await {
            tracing::warn!("Failed to generate skills.mdc: {}", e);
        }
    }

    let response = ManifestSkillResponse {
        id: skill_entry.id,
        version: skill_entry.version,
        groups: skill_entry.groups,
        editable: skill_entry.editable,
        source_type: match skill_entry.source {
            SkillSource::Git { .. } => "git",
            SkillSource::Source { .. } => "source",
            SkillSource::Local { .. } => "local",
            SkillSource::ZipUrl { .. } => "zip-url",
        }
        .to_string(),
    };

    Ok(Json(ApiResponse::success(response)))
}

/// DELETE /api/manifest/skills/:id - Remove skill from skills.toml
pub async fn remove_skill_from_manifest(
    Path(skill_id): Path<String>,
    State(state): State<AppState>,
) -> HttpResult<axum::Json<ApiResponse<()>>> {
    let manifest_path = &state.skills_toml_path;
    let lock_path = get_lock_path(manifest_path);

    if !manifest_path.exists() {
        return Err(HttpError::NotFound("skills.toml not found".to_string()));
    }

    // Remove from manifest
    let mut manifest = SkillsManifest::load_from_file(manifest_path)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to load manifest: {}", e)))?;
    manifest.remove_skill(&skill_id);
    manifest
        .save_to_file(manifest_path)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to save manifest: {}", e)))?;

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

    // Generate skills.mdc if enabled
    if state.auto_generate_mdc {
        if let Err(e) = generate_skills_mdc(&state).await {
            tracing::warn!("Failed to generate skills.mdc: {}", e);
        }
    }

    Ok(Json(ApiResponse::success(())))
}

/// PUT /api/manifest/skills/:id - Update skill in skills.toml
pub async fn update_skill_in_manifest(
    Path(skill_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<UpdateSkillRequest>,
) -> HttpResult<axum::Json<ApiResponse<ManifestSkillResponse>>> {
    let manifest_path = &state.skills_toml_path;

    if !manifest_path.exists() {
        return Err(HttpError::NotFound("skills.toml not found".to_string()));
    }

    let mut manifest = SkillsManifest::load_from_file(manifest_path)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to load manifest: {}", e)))?;

    // Find existing entry and update fields
    let entry_data = manifest.skills.iter_mut().find(|s| s.id == skill_id).ok_or_else(|| {
        HttpError::NotFound(format!("Skill '{}' not found in manifest", skill_id))
    })?;

    // Update fields
    if let Some(groups) = request.groups {
        entry_data.groups = groups;
    }
    if let Some(editable) = request.editable {
        entry_data.editable = editable;
    }
    if let Some(version) = request.version {
        entry_data.version = Some(version);
    }

    // Clone entry data before saving (to avoid borrow checker issues)
    let response = ManifestSkillResponse {
        id: entry_data.id.clone(),
        version: entry_data.version.clone(),
        groups: entry_data.groups.clone(),
        editable: entry_data.editable,
        source_type: match &entry_data.source {
            SkillSource::Git { .. } => "git",
            SkillSource::Source { .. } => "source",
            SkillSource::Local { .. } => "local",
            SkillSource::ZipUrl { .. } => "zip-url",
        }
        .to_string(),
    };

    // Save manifest
    manifest
        .save_to_file(manifest_path)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to save manifest: {}", e)))?;

    // Generate skills.mdc if enabled
    if state.auto_generate_mdc {
        if let Err(e) = generate_skills_mdc(&state).await {
            tracing::warn!("Failed to generate skills.mdc: {}", e);
        }
    }

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

/// Helper function to find skill in sources
async fn find_skill_in_sources(
    sources_manager: &SourcesManager,
    skill_id: &str,
    source_name: &str,
) -> Option<MarketplaceSkill> {
    let marketplace = sources_manager.get_marketplace_json(source_name).await.ok()?;
    marketplace.skills.into_iter().find(|s| s.id == skill_id)
}

/// Helper function to generate skills.mdc
async fn generate_skills_mdc(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let skills_dir = state.service.config().skill_storage_path.clone();

    // Find workspace root by walking up from skills.toml path
    let manifest_path = &state.skills_toml_path;
    let workspace_root = manifest_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| std::path::Path::new("."));

    let output_file = workspace_root.join(".cursor").join("rules").join("skills.mdc");

    // Ensure output directory exists
    if let Some(parent) = output_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Simple Rust implementation: scan skills directory and generate markdown
    let mut output = String::from("---\nalwaysApply: true\n---\n# Skills Registry\n\n");
    output.push_str("Skills are modular packages in `.claude/skills/<category>/<skill-name>/SKILL.md` that provide specialized workflows, tool integrations, and domain knowledge. Each SKILL.md contains YAML frontmatter (shown here: name, description) and full instructions with optional scripts/references/assets. Use the `description` field to identify relevant skills, then read the full SKILL.md at the path shown.\n\n");

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
                // Fallback: relative to skills directory, prepend .claude/skills
                format!(".claude/skills/{}", rel.to_string_lossy())
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

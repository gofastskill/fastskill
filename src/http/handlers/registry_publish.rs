//! Registry publish endpoint handlers

use crate::core::metadata::parse_yaml_frontmatter;
use crate::core::registry::staging::{StagingManager, StagingStatus};
use crate::http::auth::jwt::JwtService;
use crate::http::auth::roles::{EndpointPermissions, UserRole};
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Multipart, Path, State},
    http::{header, HeaderMap},
    Json,
};
use serde::Serialize;
use std::io::Read;
use zip::ZipArchive;

/// Publish package response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResponse {
    pub job_id: String,
    pub status: String,
    pub skill_id: String,
    pub version: String,
    pub message: String,
}

/// Publish status response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishStatusResponse {
    pub job_id: String,
    pub status: String,
    pub skill_id: String,
    pub version: String,
    pub checksum: String,
    pub uploaded_at: String,
    pub uploaded_by: Option<String>,
    pub validation_errors: Vec<String>,
    pub message: Option<String>,
    pub published_to_blob_storage: Option<bool>,
    pub blob_storage_url: Option<String>,
}

/// POST /api/registry/publish - Publish a skill package
pub async fn publish_package(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> HttpResult<Json<ApiResponse<PublishResponse>>> {
    // Check authentication
    let jwt_service = JwtService::from_env()
        .map_err(|e| HttpError::InternalServerError(format!("JWT service error: {:?}", e)))?;

    // Extract and validate token from headers
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()).map(|s| s.to_string()))
        .ok_or_else(|| HttpError::Unauthorized("No authentication token provided".to_string()))?;

    let claims = jwt_service.validate_token(&token)?;
    let user_role = UserRole::parse_role(&claims.role)
        .map_err(|e| HttpError::Unauthorized(format!("Invalid role in token: {}", e)))?;

    // Check permissions
    let check = EndpointPermissions::REGISTRY_PUBLISH.check(Some(&user_role));
    if !check.allowed {
        return Err(HttpError::Forbidden(format!(
            "Insufficient permissions. Required: {}, Got: {}",
            check.required_role, user_role
        )));
    }

    let uploaded_by = Some(claims.sub.clone());

    // Extract scope from user account (claims.sub)
    // If sub is "org/user", use "org" as scope; if "user", use "user" as scope
    let user_scope = if claims.sub.contains('/') {
        claims.sub.split('/').next().unwrap_or(&claims.sub).to_string()
    } else {
        claims.sub.clone()
    };

    // Get staging manager
    let staging_dir = state
        .service
        .config()
        .staging_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from(".staging"));

    tracing::info!("Using staging directory: {}", staging_dir.display());
    tracing::info!("Staging directory exists: {}", staging_dir.exists());

    if !staging_dir.exists() {
        tracing::info!("Creating staging directory: {}", staging_dir.display());
        std::fs::create_dir_all(&staging_dir).map_err(|e| {
            tracing::error!(
                "Failed to create staging directory {}: {}",
                staging_dir.display(),
                e
            );
            HttpError::InternalServerError("Failed to create staging directory".to_string())
        })?;
    }

    let staging_manager = StagingManager::new(staging_dir.clone());
    tracing::info!(
        "Initializing staging manager with directory: {}",
        staging_dir.display()
    );
    staging_manager.initialize().map_err(|e| {
        tracing::error!("Failed to initialize staging manager: {}", e);
        HttpError::InternalServerError("Failed to initialize staging".to_string())
    })?;

    // Extract file from multipart
    let mut package_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| HttpError::BadRequest(format!("Failed to read multipart field: {}", e)))?
    {
        let field_name = field.name().unwrap_or("");

        if field_name == "file" || field_name == "package" {
            let data = field
                .bytes()
                .await
                .map_err(|e| HttpError::BadRequest(format!("Failed to read file data: {}", e)))?;
            package_data = Some(data.to_vec());
        }
    }

    let package_data = package_data
        .ok_or_else(|| HttpError::BadRequest("No file provided in multipart form".to_string()))?;

    // Extract id and version from ZIP (id comes from skill-project.toml)
    let (id, version) = extract_skill_metadata_from_zip(&package_data)?;

    // Combine user scope with id: scope/id (for response)
    // id from skill-project.toml should not contain slashes
    let skill_id = format!("{}/{}", user_scope, id);

    // Store in staging (pass scope and id separately)
    tracing::info!(
        "Storing package: scope={}, id={}, version={}, package_size={} bytes",
        user_scope,
        id,
        version,
        package_data.len()
    );

    // Calculate the staging path that will be created
    let staging_path = staging_manager.get_staging_path(&user_scope, &id, &version);
    tracing::info!("Calculated staging path: {}", staging_path.display());

    let (_, job_id) = staging_manager
        .store_package(
            &user_scope,
            &id,
            &version,
            &package_data,
            uploaded_by.as_deref(),
        )
        .await
        .map_err(|e| {
            // Log detailed error information on server side for debugging
            tracing::error!("Failed to store package {} v{}: {}", skill_id, version, e);
            tracing::error!("Staging directory: {}", staging_dir.display());
            tracing::error!("Staging directory exists: {}", staging_dir.exists());
            tracing::error!("Calculated staging path: {}", staging_path.display());
            tracing::error!(
                "Staging path parent exists: {}",
                staging_path.parent().is_some_and(|p| p.exists())
            );
            if let Some(parent) = staging_path.parent() {
                tracing::error!("Staging path parent: {}", parent.display());
                if let Err(check_err) = std::fs::read_dir(parent) {
                    tracing::error!("Cannot read staging path parent: {}", check_err);
                }
            }
            // Return generic error message to client for security
            HttpError::InternalServerError("Failed to store package".to_string())
        })?;

    // Validation worker will automatically pick up and process this package

    let response = PublishResponse {
        job_id,
        status: "pending".to_string(),
        skill_id,
        version,
        message: "Package queued for validation".to_string(),
    };

    Ok(Json(ApiResponse::success(response)))
}

/// GET /api/registry/publish/status/:job_id - Get publish job status
pub async fn get_publish_status(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HttpResult<Json<ApiResponse<PublishStatusResponse>>> {
    // Check authentication
    let jwt_service = JwtService::from_env()
        .map_err(|e| HttpError::InternalServerError(format!("JWT service error: {:?}", e)))?;

    // Extract and validate token from headers
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()).map(|s| s.to_string()))
        .ok_or_else(|| HttpError::Unauthorized("No authentication token provided".to_string()))?;

    let claims = jwt_service.validate_token(&token)?;
    let user_role = UserRole::parse_role(&claims.role)
        .map_err(|e| HttpError::Unauthorized(format!("Invalid role in token: {}", e)))?;

    // Check permissions
    let check = EndpointPermissions::REGISTRY_PUBLISH_STATUS.check(Some(&user_role));
    if !check.allowed {
        return Err(HttpError::Forbidden(format!(
            "Insufficient permissions. Required: {}, Got: {}",
            check.required_role, user_role
        )));
    }

    // Get staging manager
    let staging_dir = state
        .service
        .config()
        .staging_dir
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from(".staging"));
    let staging_manager = StagingManager::new(staging_dir);

    // Load metadata
    let metadata = staging_manager
        .load_metadata(&job_id)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to load metadata: {}", e)))?
        .ok_or_else(|| HttpError::NotFound(format!("Job {} not found", job_id)))?;

    // Check server configuration to determine publishing status
    let config = state.service.config();
    let blob_storage_configured = config.registry_blob_storage.is_some();

    // Determine if package was actually published (only if accepted and configs are present)
    let published_to_blob_storage =
        if metadata.status == StagingStatus::Accepted && blob_storage_configured {
            Some(true)
        } else if metadata.status == StagingStatus::Accepted {
            Some(false)
        } else {
            None
        };

    // Try to get blob storage URL if published
    let blob_storage_url = if published_to_blob_storage == Some(true) {
        // Try to construct URL from config
        if let Some(ref blob_config) = config.registry_blob_storage {
            let package_path = staging_manager
                .get_package_path(&job_id)
                .map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to get package path: {}", e))
                })?
                .ok_or_else(|| {
                    HttpError::InternalServerError(format!("Package not found for job {}", job_id))
                })?;

            let package_filename =
                package_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");

            // Extract scope from the user who uploaded the package
            // Scope is the part before '/' in uploaded_by, or the entire uploaded_by if no '/'
            let scope = metadata
                .uploaded_by
                .as_ref()
                .map(|u| u.split('/').next().unwrap_or(u).to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let storage_path = format!("skills/{}/{}", scope, package_filename);

            if let Some(base_url) = &blob_config.base_url {
                Some(format!(
                    "{}/{}",
                    base_url.trim_end_matches('/'),
                    storage_path
                ))
            } else {
                config.registry_blob_base_url.as_ref().map(|blob_base_url| {
                    format!("{}/{}", blob_base_url.trim_end_matches('/'), storage_path)
                })
            }
        } else {
            None
        }
    } else {
        None
    };

    // Build descriptive message
    let message = match metadata.status {
        StagingStatus::Pending => Some("Package is pending validation".to_string()),
        StagingStatus::Validating => Some("Package is being validated".to_string()),
        StagingStatus::Accepted => {
            if blob_storage_configured {
                Some("Package has been accepted (published to blob storage)".to_string())
            } else {
                Some("Package has been accepted (staging only)".to_string())
            }
        }
        StagingStatus::Rejected => Some("Package was rejected during validation".to_string()),
    };

    let response = PublishStatusResponse {
        job_id: metadata.job_id.clone(),
        status: metadata.status.as_str().to_string(),
        skill_id: metadata.skill_id,
        version: metadata.version,
        checksum: metadata.checksum,
        uploaded_at: metadata.uploaded_at.to_rfc3339(),
        uploaded_by: metadata.uploaded_by,
        validation_errors: metadata.validation_errors,
        message,
        published_to_blob_storage,
        blob_storage_url,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// Extract skill_id and version from ZIP package
/// id is mandatory from skill-project.toml
/// version priority: skill-project.toml > SKILL.md frontmatter > default "1.0.0"
fn extract_skill_metadata_from_zip(zip_data: &[u8]) -> HttpResult<(String, String)> {
    use crate::core::manifest::MetadataSection;
    use std::io::Cursor;

    let cursor = Cursor::new(zip_data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| HttpError::BadRequest(format!("Invalid ZIP file: {}", e)))?;

    // Find and read SKILL.md and skill-project.toml
    let mut skill_content = String::new();
    let mut skill_project_content: Option<String> = None;

    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| HttpError::BadRequest(format!("Failed to read ZIP entry: {}", e)))?;

        let file_name = file.name();

        if file_name.ends_with("SKILL.md") {
            let mut reader = std::io::BufReader::new(file);
            reader
                .read_to_string(&mut skill_content)
                .map_err(|e| HttpError::BadRequest(format!("Failed to read SKILL.md: {}", e)))?;
        } else if file_name.ends_with("skill-project.toml") {
            let mut reader = std::io::BufReader::new(file);
            let mut content = String::new();
            reader.read_to_string(&mut content).map_err(|e| {
                HttpError::BadRequest(format!("Failed to read skill-project.toml: {}", e))
            })?;
            skill_project_content = Some(content);
        }
    }

    // skill-project.toml is mandatory
    let skill_project_str = skill_project_content.ok_or_else(|| {
        HttpError::BadRequest("skill-project.toml is required but not found in package".to_string())
    })?;

    // Parse skill-project.toml to extract id (mandatory) and version
    #[derive(serde::Deserialize)]
    struct SkillProjectToml {
        #[serde(default)]
        metadata: Option<MetadataSection>,
    }

    let skill_project: SkillProjectToml = toml::from_str(&skill_project_str)
        .map_err(|e| HttpError::BadRequest(format!("Failed to parse skill-project.toml: {}", e)))?;

    let metadata = skill_project.metadata.ok_or_else(|| {
        HttpError::BadRequest("skill-project.toml must have a [metadata] section".to_string())
    })?;

    // Extract id (mandatory)
    let skill_id = metadata.id.ok_or_else(|| {
        HttpError::BadRequest(
            "skill-project.toml [metadata] section must have a non-empty 'id' field".to_string(),
        )
    })?;

    // Extract version: priority skill-project.toml > SKILL.md frontmatter > default
    let version = if let Some(ref v) = metadata.version {
        if !v.is_empty() {
            v.clone()
        } else if !skill_content.is_empty() {
            // Try to get version from SKILL.md frontmatter as fallback
            let frontmatter = parse_yaml_frontmatter(&skill_content).ok();
            if let Some(ref f) = frontmatter {
                if let Some(ref v) = f.version {
                    if !v.is_empty() {
                        v.clone()
                    } else {
                        "1.0.0".to_string()
                    }
                } else {
                    "1.0.0".to_string()
                }
            } else {
                "1.0.0".to_string()
            }
        } else {
            "1.0.0".to_string()
        }
    } else if !skill_content.is_empty() {
        // Try to get version from SKILL.md frontmatter as fallback
        let frontmatter = parse_yaml_frontmatter(&skill_content).ok();
        if let Some(ref f) = frontmatter {
            if let Some(ref v) = f.version {
                if !v.is_empty() {
                    v.clone()
                } else {
                    "1.0.0".to_string()
                }
            } else {
                "1.0.0".to_string()
            }
        } else {
            "1.0.0".to_string()
        }
    } else {
        "1.0.0".to_string()
    };

    Ok((skill_id, version))
}

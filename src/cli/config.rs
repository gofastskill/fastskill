//! Configuration and skills directory resolution for CLI

use crate::cli::error::{CliError, CliResult};
use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::project;
use fastskill::core::repository::RepositoryDefinition;
use fastskill::{core::BlobStorageConfig, ServiceConfig};
use std::env;
use std::path::PathBuf;
use tracing::debug;

/// Load repositories from skill-project.toml [tool.fastskill.repositories]
/// Returns empty vector if skill-project.toml not found or no repositories configured
pub fn load_repositories_from_project() -> CliResult<Vec<RepositoryDefinition>> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    // Try to find skill-project.toml
    let project_file = project::resolve_project_file(&current_dir);
    if !project_file.found {
        return Ok(Vec::new()); // No skill-project.toml found
    }
    let project_path = project_file.path;

    let project = SkillProjectToml::load_from_file(&project_path).map_err(|e| {
        CliError::Config(format!(
            "Failed to load skill-project.toml from {}: {}",
            project_path.display(),
            e
        ))
    })?;

    // Extract repositories from [tool.fastskill]
    let repositories = project
        .tool
        .and_then(|t| t.fastskill)
        .and_then(|f| f.repositories)
        .unwrap_or_default();

    // Convert manifest::RepositoryDefinition to repository::RepositoryDefinition
    let converted_repos = repositories
        .into_iter()
        .map(convert_repository_definition)
        .collect();
    Ok(converted_repos)
}

/// Convert manifest::RepositoryDefinition to repository::RepositoryDefinition
/// These are different types because they're in different modules with slightly different structures
pub fn convert_repository_definition(
    manifest_repo: fastskill::core::manifest::RepositoryDefinition,
) -> RepositoryDefinition {
    use fastskill::core::repository::{RepositoryAuth, RepositoryConfig, RepositoryType};

    // Convert repository type
    let repo_type = match manifest_repo.r#type {
        fastskill::core::manifest::RepositoryType::HttpRegistry => RepositoryType::HttpRegistry,
        fastskill::core::manifest::RepositoryType::GitMarketplace => RepositoryType::GitMarketplace,
        fastskill::core::manifest::RepositoryType::ZipUrl => RepositoryType::ZipUrl,
        fastskill::core::manifest::RepositoryType::Local => RepositoryType::Local,
    };

    // Convert connection to config
    let config = match manifest_repo.connection {
        fastskill::core::manifest::RepositoryConnection::HttpRegistry { index_url } => {
            RepositoryConfig::HttpRegistry { index_url }
        }
        fastskill::core::manifest::RepositoryConnection::GitMarketplace { url, branch } => {
            RepositoryConfig::GitMarketplace {
                url,
                branch,
                tag: None,
            }
        }
        fastskill::core::manifest::RepositoryConnection::ZipUrl { zip_url } => {
            RepositoryConfig::ZipUrl { base_url: zip_url }
        }
        fastskill::core::manifest::RepositoryConnection::Local { path } => {
            RepositoryConfig::Local {
                path: PathBuf::from(path),
            }
        }
    };

    // Convert auth
    let auth = manifest_repo.auth.map(|a| match a.r#type {
        fastskill::core::manifest::AuthType::Pat => RepositoryAuth::Pat {
            env_var: a.env_var.unwrap_or_else(|| "PAT_TOKEN".to_string()),
        },
    });

    RepositoryDefinition {
        name: manifest_repo.name,
        repo_type,
        priority: manifest_repo.priority,
        config,
        auth,
        storage: None, // Not used in manifest format
    }
}

/// Return the list of paths (and labels) used when resolving skills, for display in "skill not found" errors.
///
/// Returns the skills directory from skill-project.toml [tool.fastskill].skills_directory.
/// No fallbacks or global directories - only the configured project directory.
pub fn get_skill_search_locations_for_display() -> CliResult<Vec<(PathBuf, String)>> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    // Use the single loader
    let config = fastskill::core::load_project_config(&current_dir).map_err(CliError::Config)?;

    Ok(vec![(config.skills_directory, "project".to_string())])
}

/// Resolve skills storage directory from skill-project.toml [tool.fastskill]
///
/// This function requires a valid skill-project.toml with [tool.fastskill].skills_directory.
/// No fallbacks - the project file must exist and be properly configured.
pub fn resolve_skills_storage_directory() -> CliResult<PathBuf> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    // Load project config using the single loader
    let config = fastskill::core::load_project_config(&current_dir).map_err(CliError::Config)?;

    debug!(
        "Using skills_directory from skill-project.toml: {}",
        config.skills_directory.display()
    );

    Ok(config.skills_directory)
}

/// Create service configuration with resolved skills directory
pub fn create_service_config(
    _skills_dir_override: Option<PathBuf>,
    _sources_path_override: Option<PathBuf>,
) -> CliResult<ServiceConfig> {
    // Resolve skills storage directory from skill-project.toml or default location
    let resolved_dir = resolve_skills_storage_directory()?;

    // Load configuration from file if available
    let config_file = crate::cli::config_file::load_config()?;

    // Extract embedding config from file
    let embedding_config = config_file
        .and_then(|config| config.embedding)
        .map(|embedding| fastskill::EmbeddingConfig {
            openai_base_url: embedding.openai_base_url,
            embedding_model: embedding.embedding_model,
            index_path: embedding.index_path,
        });

    // Read registry configuration from environment variables
    let registry_blob_storage =
        if let (Ok(bucket), Ok(region)) = (env::var("S3_BUCKET"), env::var("S3_REGION")) {
            let access_key = env::var("AWS_ACCESS_KEY_ID").unwrap_or_default();
            let secret_key = env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default();
            let endpoint = env::var("S3_ENDPOINT").ok();
            let base_url = env::var("BLOB_BASE_URL").ok();

            if !bucket.is_empty() && !region.is_empty() {
                Some(BlobStorageConfig {
                    storage_type: "s3".to_string(),
                    base_path: String::new(),
                    bucket,
                    region,
                    endpoint,
                    access_key,
                    secret_key,
                    base_url,
                })
            } else {
                None
            }
        } else {
            None
        };

    // Read registry index path from environment
    let registry_index_path = env::var("REGISTRY_INDEX_PATH").ok().map(PathBuf::from);

    // Read staging directory from environment or use default
    let staging_dir = env::var("REGISTRY_STAGING_DIR").ok().map(PathBuf::from);

    Ok(ServiceConfig {
        skill_storage_path: resolved_dir,
        embedding: embedding_config,
        registry_blob_storage,
        registry_index_path,
        staging_dir,
        ..Default::default()
    })
}

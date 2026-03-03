//! Remote search implementation for repository catalogs
//!
//! This module handles searching through remote skill repositories
//! configured in the user's repository configuration.

use super::{SearchError, SearchQuery, SearchResultItem};
use crate::core::manifest::SkillProjectToml;
use crate::core::project;
use crate::core::repository::{RepositoryDefinition, RepositoryManager};
use std::env;

/// Execute remote search query
pub async fn execute_remote_search(
    query: SearchQuery,
    repository_filter: Option<String>,
) -> Result<Vec<SearchResultItem>, SearchError> {
    // Load repository definitions from default locations
    let definitions = load_repository_definitions()?;

    let repo_manager = RepositoryManager::from_definitions(definitions);

    let repos = if let Some(repo_name) = repository_filter {
        vec![repo_manager
            .get_repository(&repo_name)
            .ok_or_else(|| SearchError::Config(format!("Repository '{}' not found", repo_name)))?]
    } else {
        repo_manager.list_repositories()
    };

    let mut all_results = Vec::new();

    for repo in repos {
        match repo_manager.get_client(&repo.name).await {
            Ok(client) => {
                if let Ok(results) = client.search(&query.query).await {
                    for result in results {
                        let result_item = SearchResultItem {
                            id: result.name.clone(),
                            name: result.name,
                            description: if result.description.is_empty() {
                                None
                            } else {
                                Some(result.description)
                            },
                            source: repo.name.clone(),
                            similarity: None, // Remote search doesn't provide similarity scores
                            path: None,
                            repository: Some(repo.name.clone()),
                        };
                        all_results.push(result_item);
                    }
                }
            }
            Err(_) => continue, // Skip repositories that fail to load
        }
    }

    // Sort by repository name for consistent ordering
    all_results.sort_by(|a, b| {
        a.repository
            .as_deref()
            .unwrap_or("")
            .cmp(b.repository.as_deref().unwrap_or(""))
            .then_with(|| a.name.cmp(&b.name))
    });
    all_results.truncate(query.limit);

    Ok(all_results)
}

/// Load repository definitions from default configuration locations
fn load_repository_definitions() -> Result<Vec<RepositoryDefinition>, SearchError> {
    let current_dir = env::current_dir()
        .map_err(|e| SearchError::Config(format!("Failed to get current directory: {}", e)))?;

    // Try to find skill-project.toml
    let project_file = project::resolve_project_file(&current_dir);
    if !project_file.found {
        return Ok(Vec::new()); // No skill-project.toml found
    }
    let project_path = project_file.path;

    let project = SkillProjectToml::load_from_file(&project_path).map_err(|e| {
        SearchError::Config(format!(
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
fn convert_repository_definition(
    manifest_repo: crate::core::manifest::RepositoryDefinition,
) -> RepositoryDefinition {
    use crate::core::repository::{RepositoryAuth, RepositoryConfig, RepositoryType};

    // Convert repository type
    let repo_type = match manifest_repo.r#type {
        crate::core::manifest::RepositoryType::HttpRegistry => RepositoryType::HttpRegistry,
        crate::core::manifest::RepositoryType::GitMarketplace => RepositoryType::GitMarketplace,
        crate::core::manifest::RepositoryType::ZipUrl => RepositoryType::ZipUrl,
        crate::core::manifest::RepositoryType::Local => RepositoryType::Local,
    };

    // Convert connection to config
    let config = match manifest_repo.connection {
        crate::core::manifest::RepositoryConnection::HttpRegistry { index_url } => {
            RepositoryConfig::HttpRegistry { index_url }
        }
        crate::core::manifest::RepositoryConnection::GitMarketplace { url, branch } => {
            RepositoryConfig::GitMarketplace {
                url,
                branch,
                tag: None,
            }
        }
        crate::core::manifest::RepositoryConnection::ZipUrl { zip_url } => {
            RepositoryConfig::ZipUrl { base_url: zip_url }
        }
        crate::core::manifest::RepositoryConnection::Local { path } => RepositoryConfig::Local {
            path: std::path::PathBuf::from(path),
        },
    };

    // Convert auth
    let auth = manifest_repo.auth.map(|a| match a.r#type {
        crate::core::manifest::AuthType::Pat => RepositoryAuth::Pat {
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

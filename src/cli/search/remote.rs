//! Remote search implementation for repository catalogs
//!
//! This module handles searching through remote skill repositories
//! configured in the user's repository configuration.

use super::{SearchError, SearchQuery, SearchResultItem};
use crate::cli::commands::registry::helpers;

/// Execute remote search query
pub async fn execute_remote_search(
    query: SearchQuery,
    repository_filter: Option<String>,
) -> Result<Vec<SearchResultItem>, SearchError> {
    let repo_manager = helpers::load_repo_manager()
        .await
        .map_err(|e| SearchError::Config(format!("Failed to load repository manager: {}", e)))?;

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

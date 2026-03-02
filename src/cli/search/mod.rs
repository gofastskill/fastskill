//! Search domain module providing unified search capabilities across local and remote scopes
//!
//! This module provides a unified interface for searching skills across different scopes:
//! - Local: Search installed/local skills using vector embeddings or text search
//! - Remote: Search remote skill catalogs across configured repositories
//!
//! The module is designed to be CLI-agnostic and can be used by other entry points
//! like HTTP API endpoints.

pub mod local;
pub mod remote;

use serde::Serialize;
use std::fmt;

/// Search scope determines where to search for skills
#[derive(Debug, Clone, PartialEq)]
pub enum SearchScope {
    /// Search local/installed skills
    Local,
    /// Search remote skill catalogs (all repositories)
    Remote,
    /// Search remote skill catalogs limited to specific repository
    RemoteRepo(String),
}

/// Search query configuration
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// The search query string
    pub query: String,
    /// Search scope (local or remote)
    pub scope: SearchScope,
    /// Maximum number of results to return
    pub limit: usize,
    /// Whether to use embedding search (for local search only)
    pub embedding: Option<bool>,
}

/// Unified search result item that works across all search scopes
#[derive(Debug, Clone, Serialize)]
pub struct SearchResultItem {
    /// Skill identifier
    pub id: String,
    /// Display name or title
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// Source of the result (e.g., "local" or repository name)
    pub source: String,
    /// Optional similarity score (for embedding-based searches)
    pub similarity: Option<f32>,
    /// Optional skill path (for local results)
    pub path: Option<String>,
    /// Optional repository name (for remote results)
    pub repository: Option<String>,
}

impl fmt::Display for SearchResultItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}",
            self.name,
            self.description.as_deref().unwrap_or("No description")
        )
    }
}

/// Execute a search query and return unified results
pub async fn execute(
    query: SearchQuery,
    service: &fastskill::FastSkillService,
) -> Result<Vec<SearchResultItem>, SearchError> {
    let scope = query.scope.clone();
    match scope {
        SearchScope::Local => local::execute_local_search(query, service).await,
        SearchScope::Remote => remote::execute_remote_search(query, None).await,
        SearchScope::RemoteRepo(repo_name) => {
            remote::execute_remote_search(query, Some(repo_name)).await
        }
    }
}

/// Search-specific error types
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Service error: {0}")]
    Service(#[from] fastskill::ServiceError),
}

impl From<SearchError> for crate::cli::error::CliError {
    fn from(err: SearchError) -> Self {
        match err {
            SearchError::Config(msg) => crate::cli::error::CliError::Config(msg),
            SearchError::Validation(msg) => crate::cli::error::CliError::Validation(msg),
            SearchError::Service(err) => crate::cli::error::CliError::Service(err),
        }
    }
}

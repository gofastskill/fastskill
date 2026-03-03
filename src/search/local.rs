//! Local search implementation for installed/local skills
//!
//! This module handles searching through skills that are installed locally,
//! using either embedding-based semantic search or fallback text search.

use super::{SearchError, SearchQuery, SearchResultItem};
use crate::{EmbeddingService, FastSkillService};

/// Execute local search query
pub async fn execute_local_search(
    query: SearchQuery,
    service: &FastSkillService,
) -> Result<Vec<SearchResultItem>, SearchError> {
    let results = match query.embedding {
        Some(false) => {
            // --embedding false: use text search only
            perform_text_search(service, &query.query, query.limit).await?
        }
        Some(true) => {
            // --embedding true: use embedding search only, no fallback
            perform_embedding_search(service, &query.query, query.limit).await?
        }
        None => {
            // No flag: try embedding, fall back to text on config error
            match perform_embedding_search(service, &query.query, query.limit).await {
                Ok(r) => r,
                Err(SearchError::Config(_)) => {
                    perform_text_search(service, &query.query, query.limit).await?
                }
                Err(e) => return Err(e),
            }
        }
    };

    Ok(results)
}

/// Text/fuzzy search fallback when embedding or OPENAI_API_KEY is not available.
async fn perform_text_search(
    service: &FastSkillService,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResultItem>, SearchError> {
    let meta_list = service
        .metadata_service()
        .search_skills(query)
        .await
        .map_err(|e| SearchError::Validation(format!("Text search failed: {}", e)))?;

    let mut results = Vec::new();
    for meta in meta_list.into_iter().take(limit) {
        let Some(skill_def) = service
            .skill_manager()
            .get_skill(&meta.id)
            .await
            .map_err(|e| SearchError::Validation(format!("Lookup failed: {}", e)))?
        else {
            continue;
        };

        let skill_path = skill_def
            .skill_file
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| skill_def.skill_file.clone());

        let result_item = SearchResultItem {
            id: meta.id.as_str().to_string(),
            name: if meta.name.is_empty() {
                meta.id.as_str().to_string()
            } else {
                meta.name
            },
            description: if meta.description.is_empty() {
                None
            } else {
                Some(meta.description)
            },
            source: "local".to_string(),
            similarity: Some(1.0), // Text search has no similarity score
            path: Some(skill_path.to_string_lossy().to_string()),
            repository: None,
        };

        results.push(result_item);
    }
    Ok(results)
}

/// Perform embedding-based search
async fn perform_embedding_search(
    service: &FastSkillService,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResultItem>, SearchError> {
    let embedding_config = service
        .config()
        .embedding
        .as_ref()
        .ok_or_else(|| {
            SearchError::Config(
                "Embedding configuration required but not found. Please configure embedding settings in skill-project.toml and set OPENAI_API_KEY environment variable.".to_string()
            )
        })?;

    let vector_index_service = service
        .vector_index_service()
        .ok_or_else(|| SearchError::Config("Vector index service not available".to_string()))?;

    // Get API key from environment
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|e| {
        SearchError::Config(format!(
            "Failed to get OPENAI_API_KEY from environment: {}",
            e
        ))
    })?;

    // Initialize embedding service
    let embedding_service = crate::OpenAIEmbeddingService::from_config(embedding_config, api_key);

    // Generate query embedding
    let query_embedding = embedding_service.embed_query(query).await.map_err(|e| {
        SearchError::Validation(format!("Failed to generate query embedding: {}", e))
    })?;

    // Search vector index
    let matches = vector_index_service
        .search_similar(&query_embedding, limit)
        .await
        .map_err(|e| SearchError::Validation(format!("Vector search failed: {}", e)))?;

    // Convert to SearchResultItem
    let results = matches
        .into_iter()
        .map(|skill_match| {
            let name = skill_match
                .skill
                .frontmatter_json
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&skill_match.skill.id)
                .to_string();

            let description = skill_match
                .skill
                .frontmatter_json
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            SearchResultItem {
                id: skill_match.skill.id,
                name,
                description,
                source: "local".to_string(),
                similarity: Some(skill_match.similarity),
                path: Some(skill_match.skill.skill_path.to_string_lossy().to_string()),
                repository: None,
            }
        })
        .collect();

    Ok(results)
}

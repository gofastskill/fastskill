//! Local search implementation for installed/local skills
//!
//! This module handles searching through skills that are installed locally,
//! using either embedding-based semantic search or fallback text search.

use super::{SearchError, SearchQuery, SearchResultItem};
use crate::FastSkillService;

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
            version: Some(skill_def.version),
            install_command: None,
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
    let vector_index_service = service
        .vector_index_service()
        .ok_or_else(|| SearchError::Config("Vector index service not available".to_string()))?;
    let embedding_service = service.embedding_service().ok_or_else(|| {
        SearchError::Config(
            "Embedding provider required but not configured for this service".to_string(),
        )
    })?;

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
                version: skill_match
                    .skill
                    .frontmatter_json
                    .get("version")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                install_command: None,
            }
        })
        .collect();

    Ok(results)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{EmbeddingConfig, EmbeddingService, ServiceConfig, ServiceError};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct TestEmbedding;

    #[async_trait]
    impl EmbeddingService for TestEmbedding {
        async fn embed_text(&self, text: &str) -> Result<Vec<f32>, ServiceError> {
            Ok(vec![text.len() as f32, 0.0, 0.0])
        }

        async fn embed_query(&self, query: &str) -> Result<Vec<f32>, ServiceError> {
            self.embed_text(query).await
        }
    }

    async fn service(root: &TempDir, embeddings: bool) -> FastSkillService {
        let skills = root.path().join("skills");
        let demo = skills.join("demo");
        std::fs::create_dir_all(&demo).unwrap();
        std::fs::write(
            demo.join("SKILL.md"),
            "---\nname: Demo\nversion: 1.2.3\ndescription: Searchable fixture\n---\n# Demo\n",
        )
        .unwrap();
        let config = ServiceConfig {
            skill_storage_path: skills,
            embedding: embeddings.then(|| EmbeddingConfig {
                openai_base_url: "http://unused.invalid".to_string(),
                embedding_model: "fixture".to_string(),
                index_path: Some(root.path().join("index.db")),
            }),
            ..Default::default()
        };
        let service = FastSkillService::new(config).await.unwrap();
        let mut service = if embeddings {
            service.with_embedding_service(Arc::new(TestEmbedding))
        } else {
            service
        };
        service.initialize().await.unwrap();
        if embeddings {
            service.reindex(None, None).await.unwrap();
        }
        service
    }

    #[tokio::test]
    async fn local_text_embedding_and_auto_fallback_return_versioned_results() {
        let text_root = TempDir::new().unwrap();
        let text = service(&text_root, false).await;
        for embedding in [Some(false), None] {
            let results = execute_local_search(
                SearchQuery {
                    query: "demo".to_string(),
                    scope: super::super::SearchScope::Local,
                    limit: 1,
                    embedding,
                },
                &text,
            )
            .await
            .unwrap();
            assert_eq!(results[0].version.as_deref(), Some("1.2.3"));
        }

        let embedding_root = TempDir::new().unwrap();
        let embedding = service(&embedding_root, true).await;
        let results = execute_local_search(
            SearchQuery {
                query: "demo".to_string(),
                scope: super::super::SearchScope::Local,
                limit: 1,
                embedding: Some(true),
            },
            &embedding,
        )
        .await
        .unwrap();
        assert_eq!(results[0].version.as_deref(), Some("1.2.3"));

        let provider_root = TempDir::new().unwrap();
        let skills = provider_root.path().join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        let mut without_provider = FastSkillService::new(ServiceConfig {
            skill_storage_path: skills,
            embedding: Some(EmbeddingConfig {
                openai_base_url: "http://unused.invalid".to_string(),
                embedding_model: "fixture".to_string(),
                index_path: Some(provider_root.path().join("index.db")),
            }),
            ..Default::default()
        })
        .await
        .unwrap();
        without_provider.initialize().await.unwrap();
        let error = execute_local_search(
            SearchQuery {
                query: "demo".to_string(),
                scope: super::super::SearchScope::Local,
                limit: 1,
                embedding: Some(true),
            },
            &without_provider,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SearchError::Config(message) if message.contains("provider")));
    }
}

//! Search endpoint handlers

use crate::core::embedding::EmbeddingService;
use crate::http::auth::roles::EndpointPermissions;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use crate::OpenAIEmbeddingService;
use axum::{extract::State, Json};
use validator::Validate;

/// POST /api/search - Search skills
pub async fn search_skills(
    State(state): State<AppState>,
    Json(request): Json<SearchRequest>,
) -> HttpResult<axum::Json<ApiResponse<SearchResponse>>> {
    // Check permissions
    let _check = EndpointPermissions::SEARCH.check(None);

    // Validate request
    request.validate().map_err(|e| {
        HttpError::ValidationError(
            e.field_errors()
                .into_iter()
                .map(|(field, errors)| {
                    (
                        field.to_string(),
                        errors
                            .iter()
                            .map(|e| e.message.clone().unwrap_or_default().to_string())
                            .collect(),
                    )
                })
                .collect(),
        )
    })?;

    let limit = request.limit.unwrap_or(10).clamp(1, 50);

    // Determine search mode
    let use_semantic = request.semantic != Some(false)
        && state.service.config().embedding.is_some()
        && state.service.vector_index_service().is_some();

    let skills: Vec<SkillMatchResponse> = if use_semantic {
        // Semantic search requires OPENAI_API_KEY
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            HttpError::ServiceUnavailable(
                "OPENAI_API_KEY environment variable is required for semantic search".to_string(),
            )
        })?;

        let embedding_config = state.service.config().embedding.as_ref().ok_or_else(|| {
            HttpError::ServiceError("Embedding configuration is not available".to_string())
        })?;
        let vector_index_service = state.service.vector_index_service().ok_or_else(|| {
            HttpError::ServiceError("Vector index service is not available".to_string())
        })?;

        // Initialize embedding service
        let embedding_service: OpenAIEmbeddingService =
            OpenAIEmbeddingService::from_config(embedding_config, api_key);

        // Generate query embedding
        let query_embedding = embedding_service
            .embed_query(&request.query)
            .await
            .map_err(|e| {
                HttpError::ServiceUnavailable(format!("Failed to generate query embedding: {}", e))
            })?;

        // Search vector index
        let matches = vector_index_service
            .search_similar(&query_embedding, limit as usize)
            .await
            .map_err(|e| HttpError::ServiceError(format!("Vector search failed: {}", e)))?;

        // Convert matches to SkillMatchResponse
        matches
            .into_iter()
            .map(|m| SkillMatchResponse {
                skill: SkillResponse {
                    id: m.skill.id.clone(),
                    name: m
                        .skill
                        .frontmatter_json
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&m.skill.id)
                        .to_string(),
                    description: m
                        .skill
                        .frontmatter_json
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    metadata: m.skill.frontmatter_json.clone(),
                    created_at: None,
                    updated_at: Some(m.skill.updated_at.to_rfc3339()),
                },
                score: m.similarity,
                relevance: if m.similarity >= 0.8 {
                    "high".to_string()
                } else if m.similarity >= 0.5 {
                    "medium".to_string()
                } else {
                    "low".to_string()
                },
            })
            .collect()
    } else {
        // Text fallback: search by substring in name/description
        let skills_list = state
            .service
            .skill_manager()
            .list_skills(None)
            .await
            .map_err(|e| HttpError::ServiceError(format!("Failed to list skills: {}", e)))?;

        let query_lower = request.query.to_lowercase();
        skills_list
            .into_iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&query_lower)
                    || s.description.to_lowercase().contains(&query_lower)
            })
            .take(limit as usize)
            .map(|s| SkillMatchResponse {
                skill: SkillResponse {
                    id: s.id.to_string(),
                    name: s.name,
                    description: s.description,
                    metadata: serde_json::json!({}),
                    created_at: None,
                    updated_at: None,
                },
                score: 1.0,
                relevance: "keyword".to_string(),
            })
            .collect()
    };

    let response = SearchResponse {
        count: skills.len(),
        query: request.query,
        skills,
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

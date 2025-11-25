//! Search endpoint handlers

use crate::http::auth::roles::EndpointPermissions;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{extract::State, Json};
use validator::Validate;

/// POST /api/search - Search skills
pub async fn search_skills(
    State(_state): State<AppState>,
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

    // For now, return a basic response (search functionality needs to be implemented)
    let response = SearchResponse {
        skills: vec![], // TODO: Implement actual search
        count: 0,
        query: request.query.clone(),
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

//! Resolve endpoint handler

use crate::core::context_resolver::ResolveContextRequest;
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::ApiResponse;
use axum::{extract::State, Json};
use std::collections::HashMap;

/// POST /api/resolve - Resolve skills with canonical paths and optional content
pub async fn resolve_context(
    State(state): State<AppState>,
    Json(request): Json<ResolveContextRequest>,
) -> HttpResult<axum::Json<ApiResponse<crate::core::context_resolver::ResolveContextResponse>>> {
    if request.prompt.trim().is_empty() {
        let mut errs = HashMap::new();
        errs.insert(
            "prompt".to_string(),
            vec!["RESOLVE_EMPTY_PROMPT: prompt cannot be empty".to_string()],
        );
        return Err(HttpError::ValidationError(errs));
    }

    if request.limit == 0 {
        let mut errs = HashMap::new();
        errs.insert(
            "limit".to_string(),
            vec!["RESOLVE_LIMIT_ZERO: limit must be greater than 0".to_string()],
        );
        return Err(HttpError::ValidationError(errs));
    }

    let resolver = state.service.context_resolver();
    let response = resolver
        .resolve_context(request)
        .await
        .map_err(|e| HttpError::ServiceError(e.to_string()))?;

    Ok(axum::Json(ApiResponse::success(response)))
}

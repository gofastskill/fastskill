//! Reindex endpoint handlers

use crate::http::auth::roles::EndpointPermissions;
use crate::http::errors::HttpResult;
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    Json,
};
use std::time::Instant;

/// POST /api/reindex - Reindex all skills
pub async fn reindex_all(
    State(_state): State<AppState>,
    Json(_request): Json<ReindexRequest>,
) -> HttpResult<axum::Json<ApiResponse<ReindexResponse>>> {
    // Check permissions
    let _check = EndpointPermissions::REINDEX.check(None);

    let start_time = Instant::now();

    // For now, return a mock response (reindexing needs to be implemented)
    let response = ReindexResponse {
        success_count: 0,
        error_count: 0,
        total_processed: 0,
        duration_ms: start_time.elapsed().as_millis() as u64,
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

/// POST /api/reindex/{id} - Reindex specific skill
pub async fn reindex_skill(
    State(_state): State<AppState>,
    Path(_skill_id): Path<String>,
    Json(_request): Json<ReindexRequest>,
) -> HttpResult<axum::Json<ApiResponse<ReindexResponse>>> {
    // Check permissions
    let _check = EndpointPermissions::REINDEX_SKILL.check(None);

    let start_time = Instant::now();

    // For now, return a mock response
    let response = ReindexResponse {
        success_count: 1,
        error_count: 0,
        total_processed: 1,
        duration_ms: start_time.elapsed().as_millis() as u64,
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

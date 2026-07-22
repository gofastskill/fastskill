//! Reindex endpoint handlers
//!
//! PARTIAL-6 / ADR-0003: these endpoints mutate the embedding index and are
//! therefore write-gated (only mounted with `--enable-write`). They call the
//! core reindex seam (`FastSkillService::reindex`, ADR-0002/0005) directly with
//! no observer (HTTP has no progress-streaming channel). Per ADR-0002, when no
//! embedding provider is configured the seam skips silently — that is surfaced
//! here as `200` with `reindexed: false` and a `reason`, not an error.
//!
//! The core seam has no single-skill mode (it always walks the whole skills
//! directory); rather than growing the core seam for an HTTP-only convenience,
//! `POST /reindex/{id}` reindexes the *whole* index, same as `POST /reindex`.
//! This is a deliberate, documented simplification, not an oversight.

use crate::http::errors::HttpResult;
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    Json,
};

fn outcome_response(
    outcome: crate::core::reindex::ReindexOutcome,
) -> axum::Json<ApiResponse<ReindexOutcomeResponse>> {
    Json(ApiResponse::success(ReindexOutcomeResponse {
        reindexed: outcome.reindexed,
        count: outcome.count,
        reason: outcome.reason,
    }))
}

/// POST /api/v1/reindex - Reindex all skills (skips silently, 200, when no
/// embedding provider is configured; ADR-0002).
pub async fn reindex_all(
    State(state): State<AppState>,
    Json(_request): Json<ReindexRequest>,
) -> HttpResult<axum::Json<ApiResponse<ReindexOutcomeResponse>>> {
    let outcome = state.service.reindex(None, None).await?;
    Ok(outcome_response(outcome))
}

/// POST /api/v1/reindex/{id} - Reindex a single skill.
///
/// The core reindex seam has no single-skill mode (see module docs); `id` is
/// accepted for URL/API-contract compatibility but the whole index is
/// reindexed, same as `POST /reindex`.
pub async fn reindex_skill(
    State(state): State<AppState>,
    Path(_skill_id): Path<String>,
    Json(_request): Json<ReindexRequest>,
) -> HttpResult<axum::Json<ApiResponse<ReindexOutcomeResponse>>> {
    let outcome = state.service.reindex(None, None).await?;
    Ok(outcome_response(outcome))
}

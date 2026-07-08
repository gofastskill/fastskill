//! Reindex endpoint handlers
//!
//! PARTIAL-6 / ADR-0003: these endpoints mutate the embedding index and are
//! therefore write-gated (only mounted with `--enable-write`). The real reindex
//! implementation (`execute_reindex`) lives in the `fastskill-cli` crate and
//! depends on CLI-only pieces (OpenAI key loading, progress reporting), so it is
//! NOT cleanly reachable from `fastskill-core`. Rather than returning a fake
//! `200` with mock counts, these handlers honestly return `501 Not Implemented`
//! and direct callers to `fastskill reindex` on the CLI. Wiring a core-level
//! reindex service is tracked separately (spec 003 / browser skill management).

use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

fn not_implemented() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ApiResponse::<ReindexResponse>::error(ErrorResponse {
            code: "NOT_IMPLEMENTED".to_string(),
            message: "reindex via HTTP is not implemented; run `fastskill reindex` from the CLI"
                .to_string(),
            details: None,
        })),
    )
        .into_response()
}

/// POST /api/reindex - Reindex all skills
pub async fn reindex_all(
    State(_state): State<AppState>,
    Json(_request): Json<ReindexRequest>,
) -> Response {
    not_implemented()
}

/// POST /api/reindex/{id} - Reindex specific skill
pub async fn reindex_skill(
    State(_state): State<AppState>,
    Path(_skill_id): Path<String>,
    Json(_request): Json<ReindexRequest>,
) -> Response {
    not_implemented()
}

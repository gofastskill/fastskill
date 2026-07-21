//! Core reindex seam (ADR-0002/0005).
//!
//! Reindex is domain logic (skills dir → Vector index) and lives in core. It runs
//! only when an [`EmbeddingService`](crate::core::embedding::EmbeddingService) has
//! been injected (edge-constructed with the API key); otherwise it **skips
//! silently**. The CLI/serve edge injects the provider via
//! [`FastSkillService::with_embedding_service`].

use crate::core::service::{FastSkillService, ServiceError};
use std::path::Path;

/// Progress datum emitted per skill as reindex proceeds. The core emits neutral
/// data; the caller (CLI) decides how to render it (HTTP passes no observer).
#[derive(Debug, Clone)]
pub struct ReindexProgress {
    pub current: usize,
    pub total: usize,
    pub skill_id: String,
}

/// Outcome of a reindex call. `reindexed=false` + a `reason` means it was skipped
/// (no provider), which is a success, not a failure (ADR-0002).
#[derive(Debug, Clone)]
pub struct ReindexOutcome {
    pub reindexed: bool,
    pub count: usize,
    pub reason: Option<String>,
}

impl ReindexOutcome {
    pub(crate) fn skipped(reason: &str) -> Self {
        ReindexOutcome {
            reindexed: false,
            count: 0,
            reason: Some(reason.to_string()),
        }
    }
}

impl FastSkillService {
    /// Reindex the vector index for `skills_dir` (defaults to the configured skill
    /// storage path). Skips silently with an outcome reason when no embedding
    /// provider is injected. `observer`, if provided, is called once per skill.
    pub async fn reindex(
        &self,
        skills_dir: Option<&Path>,
        observer: Option<&(dyn Fn(ReindexProgress) + Send + Sync)>,
    ) -> Result<ReindexOutcome, ServiceError> {
        let _ = (skills_dir, observer);
        // No provider ⇒ skip silently (the common, non-configured case).
        if self.embedding_service().is_none() {
            return Ok(ReindexOutcome::skipped("no embedding provider configured"));
        }
        // TODO(phase1-agent-A): move the real reindex logic here from the CLI
        // `execute_reindex` (find SKILL.md files, embed each via the injected
        // provider, write to the VectorIndexService), emitting `observer`
        // progress. Return `{reindexed:true, count, reason:None}`.
        Ok(ReindexOutcome::skipped("reindex not yet implemented"))
    }
}

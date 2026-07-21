//! The core install seam (ADR-0005): `add_from_origin(origin, mode)`.
//!
//! `add_from_origin = commit(fetch(origin), origin, mode)`. `fetch` is the only
//! per-variant part (match-dispatched, produces a temp dir + the [`Resolved`]
//! facts); `commit` is the shared pipeline (validate → atomic-move into the
//! skills dir → upsert Manifest → write Lock → reindex-if-provider). `mode` only
//! governs the id-conflict policy. `add`/`update` are one operation.

use crate::core::origin::{Origin, Resolved};
use crate::core::service::{FastSkillService, ServiceError};
use std::path::PathBuf;
use tempfile::TempDir;

/// Whether an install is a fresh add (409 on an existing id) or an update
/// (overwrite the recorded skill). See ADR-0005.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddMode {
    /// Adding a new skill: fail if the resolved id is already installed.
    Fresh,
    /// Re-installing an already-recorded skill from its origin: overwrite.
    Update,
}

/// Result of a successful `add_from_origin`.
#[derive(Debug, Clone)]
pub struct AddOutcome {
    pub id: String,
    pub origin: Origin,
    pub resolved: Resolved,
    /// Whether the auto-reindex ran (false = skipped, e.g. no embedding provider).
    pub reindexed: bool,
}

/// The outcome of the update preflight (ADR-0005 §Q6). Only `Updatable` proceeds
/// to a re-fetch; the others are honest no-ops with a reason.
#[derive(Debug, Clone)]
pub enum UpdatePreflight {
    UpToDate,
    Immutable { reason: String },
    Updatable,
}

/// A fetched skill sitting in a temp dir, plus the facts the fetch resolved.
/// The `TempDir` guards the extracted contents until `commit` moves them.
pub struct Fetched {
    pub temp_dir: TempDir,
    pub skill_path: PathBuf,
    pub resolved: Resolved,
}

impl FastSkillService {
    /// Install a single skill from an [`Origin`] (ADR-0005). `Fresh` adds a new
    /// skill (409-style error if the id already exists); `Update` re-resolves the
    /// origin and overwrites. Equivalent to `commit(fetch(origin), origin, mode)`.
    pub async fn add_from_origin(
        &self,
        origin: Origin,
        mode: AddMode,
    ) -> Result<AddOutcome, ServiceError> {
        let fetched = self.fetch(&origin).await?;
        self.commit(fetched, origin, mode).await
    }

    /// Fetch a skill described by `origin` into a temp dir, capturing the resolved
    /// facts. The only per-variant step (git clone / local copy-or-unzip / remote
    /// zip download / registry download).
    async fn fetch(&self, origin: &Origin) -> Result<Fetched, ServiceError> {
        let _ = origin;
        // TODO(phase1-agent-B): match on the four Origin variants and fetch into a
        // temp dir using the core primitives (storage::git::clone_repository,
        // storage::zip::ZipHandler::extract_to_dir, the registry clients via
        // `self.repository_manager()`), returning `Fetched{temp_dir, skill_path,
        // resolved}`. Move the orchestration from the CLI add_from_*/install_from_*.
        Err(ServiceError::InvalidOperation(
            "add_from_origin: fetch not yet implemented".to_string(),
        ))
    }

    /// The shared post-fetch pipeline: validate → atomic-move into the skills dir →
    /// upsert Manifest → write Lock (origin + resolved) → reindex-if-provider.
    /// Ordering is skills-dir → manifest → lock → reindex (never reference a skill
    /// before it exists); each store write is atomic; recovery is idempotent
    /// re-run + reconcile (ADR-0005 §Q3).
    async fn commit(
        &self,
        fetched: Fetched,
        origin: Origin,
        mode: AddMode,
    ) -> Result<AddOutcome, ServiceError> {
        let _ = (fetched, origin, mode);
        // TODO(phase1-agent-B): validate the fetched skill, derive its id, enforce
        // the mode's id-conflict policy (Fresh → error on existing), atomic-rename
        // into the skills dir, upsert the manifest entry, write the lock entry
        // (origin + resolved), then `self.reindex(None, None)` best-effort and set
        // `reindexed`. Return the AddOutcome.
        Err(ServiceError::InvalidOperation(
            "add_from_origin: commit not yet implemented".to_string(),
        ))
    }

    /// Update preflight (ADR-0005 §Q6): decide whether the recorded origin has
    /// anything to update before doing any fetch. `repository` → newest allowed
    /// via UpdateService; immutable git tag/commit + editable local → `Immutable`;
    /// git branch / local copy / zip-url → `Updatable` (re-fetch; commit is idempotent).
    pub async fn preflight(&self, origin: &Origin) -> Result<UpdatePreflight, ServiceError> {
        let _ = origin;
        // TODO(phase1-agent-B): implement the per-variant preflight table.
        Err(ServiceError::InvalidOperation(
            "update preflight not yet implemented".to_string(),
        ))
    }
}

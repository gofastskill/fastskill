//! Application context for the fastskill CLI.
//!
//! `FsCtx` is a trivial empty struct that satisfies the `AppContext` trait bound.
//! All shared state lives in `FsState`, which is captured via `Arc<FsState>` in
//! each command closure at registration time — no `Any`-downcasting needed.

use cli_framework::prelude::AppContext;
use fastskill_core::FastSkillService;
use std::path::PathBuf;
use std::sync::Arc;

/// Trivial application context — satisfies `AppContext: Send + Sync`.
/// All operational state is captured via `Arc<FsState>` in command closures.
pub struct FsCtx;

impl AppContext for FsCtx {}

/// Shared application state captured by each command closure.
///
/// Created once in `main()` and cloned via `Arc` into every registered command.
/// Global flags (--skills-dir, --global, --verbose) are parsed by the framework
/// and accessed via `AppContext::opt_global_args()` in each handler.
pub struct FsState {
    /// Shared cell for lazy service initialisation.
    service_cell: Arc<tokio::sync::OnceCell<Arc<FastSkillService>>>,
}

impl FsState {
    pub fn new() -> Self {
        Self {
            service_cell: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Initialise (or reuse) the `FastSkillService` and return an `Arc` to it.
    ///
    /// `global` and `skills_dir` come from the framework-parsed global flags
    /// (extracted from `ctx.opt_global_args()` in each handler). The OnceCell
    /// ensures the service is initialised only once per process invocation.
    pub async fn service_with(
        &self,
        global: bool,
        skills_dir: Option<PathBuf>,
    ) -> crate::error::CliResult<Arc<FastSkillService>> {
        use crate::error::CliError;
        self.service_cell
            .get_or_try_init(|| async move {
                let cfg = crate::config::create_service_config(global, skills_dir)?;
                let mut s = FastSkillService::new(cfg)
                    .await
                    .map_err(CliError::Service)?;
                s.initialize().await.map_err(CliError::Service)?;
                Ok(Arc::new(s))
            })
            .await
            .cloned()
    }
}

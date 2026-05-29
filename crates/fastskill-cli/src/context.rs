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
pub struct FsState {
    /// Shared cell for lazy service initialisation.
    service_cell: Arc<tokio::sync::OnceCell<Arc<FastSkillService>>>,

    /// Optional override for the skills directory (from `--skills-dir`).
    pub skills_dir_override: Option<PathBuf>,

    /// Whether `--global` was passed (use `~/.config/fastskill/skills`).
    pub global: bool,

    /// Whether `--verbose` / `-v` was passed.
    #[allow(dead_code)]
    pub verbose: bool,

    /// The remaining argv after `strip_global_flags`, starting with the
    /// binary name.  Bridge closures use this to reconstruct subcommand args.
    pub raw_remaining_args: Vec<String>,
}

impl FsState {
    pub fn new(
        skills_dir_override: Option<PathBuf>,
        global: bool,
        verbose: bool,
        raw_remaining_args: Vec<String>,
    ) -> Self {
        Self {
            service_cell: Arc::new(tokio::sync::OnceCell::new()),
            skills_dir_override,
            global,
            verbose,
            raw_remaining_args,
        }
    }

    /// Initialise (or reuse) the `FastSkillService` and return an `Arc` to it.
    pub async fn service(&self) -> crate::error::CliResult<Arc<FastSkillService>> {
        use crate::error::CliError;
        let global = self.global;
        let skills_dir = self.skills_dir_override.clone();
        self.service_cell
            .get_or_try_init(|| async move {
                let cfg = crate::config::create_service_config(global, skills_dir, None)?;
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

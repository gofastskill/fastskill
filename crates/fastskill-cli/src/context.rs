//! Application context for the fastskill CLI.
//!
//! `FsCtx` is the single shared context passed to every command.  It owns the
//! lazily-initialised `FastSkillService` (via an async `OnceCell`) and the
//! pre-parsed global flags extracted by `strip_global_flags` in `main.rs`.

use cli_framework::prelude::AppContext;
use fastskill_core::FastSkillService;
use std::path::PathBuf;
use std::sync::Arc;

/// Main application context.
pub struct FsCtx {
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

impl AppContext for FsCtx {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl FsCtx {
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

    /// Return a lightweight, clone-friendly handle for service initialisation.
    ///
    /// Cloning the handle is cheap (Arc clone). The underlying OnceCell is
    /// shared, so `handle.get().await` initialises the service exactly once
    /// regardless of how many handles exist.
    pub fn service_handle(&self) -> ServiceHandle {
        ServiceHandle {
            cell: self.service_cell.clone(),
            skills_dir: self.skills_dir_override.clone(),
            global: self.global,
        }
    }
}

/// Sendable handle to the shared service OnceCell.
///
/// Bridge closures clone this handle before any `.await` to avoid holding a
/// borrow of `ctx: &mut dyn AppContext` across await points.
pub struct ServiceHandle {
    cell: Arc<tokio::sync::OnceCell<Arc<FastSkillService>>>,
    skills_dir: Option<PathBuf>,
    global: bool,
}

impl ServiceHandle {
    /// Initialise (or reuse) the `FastSkillService` and return an `Arc` to it.
    pub async fn get(&self) -> crate::error::CliResult<Arc<FastSkillService>> {
        use crate::error::CliError;
        let global = self.global;
        let skills_dir = self.skills_dir.clone();
        self.cell
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctx_downcast_roundtrip() {
        let ctx = FsCtx::new(None, false, false, vec!["fastskill".to_string()]);
        let any = ctx.as_any();
        assert!(any.is_some());
        let back = any.and_then(|a| a.downcast_ref::<FsCtx>());
        assert!(back.is_some());
    }

    #[test]
    fn test_ctx_downcast_mut_roundtrip() {
        let mut ctx = FsCtx::new(None, false, false, vec!["fastskill".to_string()]);
        let any = ctx.as_any_mut();
        assert!(any.is_some());
        let back = any.and_then(|a| a.downcast_mut::<FsCtx>());
        assert!(back.is_some());
    }
}

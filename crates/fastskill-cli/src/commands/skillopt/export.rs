//! `fastskill skillopt export` subcommand

use crate::error::{CliError, CliResult};
use clap::Args;
use std::path::PathBuf;

/// Arguments for `fastskill skillopt export`
#[derive(Debug, Args)]
#[command(
    about = "Export the best skill document from a completed run",
    after_help = "Examples:\n  fastskill skillopt export .skillopt/runs/2026-06-02T14-00-00Z --out ./SKILL.md"
)]
pub struct ExportArgs {
    /// Path to the run directory
    pub run_dir: PathBuf,

    /// Destination path for the exported skill document
    #[arg(long, required = true)]
    pub out: PathBuf,
}

pub async fn execute_export(args: ExportArgs) -> CliResult<()> {
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    let best_skill_path = args.run_dir.join("best_skill.md");
    if !best_skill_path.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_EXPORT_BEST_MISSING: best_skill.md not found in run directory: {}",
            args.run_dir.display()
        )));
    }

    if let Some(parent) = args.out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(CliError::Io)?;
        }
    }

    std::fs::copy(&best_skill_path, &args.out).map_err(CliError::Io)?;

    Ok(())
}

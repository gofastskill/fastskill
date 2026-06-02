//! SkillOpt command group — iterative skill-document optimization via text-gradient.

pub mod config;
pub mod export;
pub mod inspect;
pub mod resume;
pub mod run;
pub mod status;

use crate::error::CliResult;
use clap::{Args, Subcommand};

/// Iterative skill-document optimization via text-gradient.
#[derive(Debug, Args)]
#[command(
    about = "Iterative skill-document optimization via text-gradient",
    after_help = "Examples:\n  fastskill skillopt run --config skillopt.toml\n  fastskill skillopt status .skillopt/runs/2026-06-02T14-00-00Z\n  fastskill skillopt export .skillopt/runs/2026-06-02T14-00-00Z --out ./SKILL.md"
)]
pub struct SkillOptCommand {
    #[command(subcommand)]
    pub command: SkillOptSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SkillOptSubcommand {
    /// Run skill optimization from a config file
    Run(run::RunArgs),

    /// Resume an interrupted optimization run
    Resume(resume::ResumeArgs),

    /// Show the status of a training run
    Status(status::StatusArgs),

    /// Inspect per-step artifacts from a training run
    Inspect(inspect::InspectArgs),

    /// Export the best skill document from a completed run
    Export(export::ExportArgs),
}

pub async fn execute_skillopt(args: SkillOptCommand) -> CliResult<()> {
    match args.command {
        SkillOptSubcommand::Run(args) => run::execute_run(args).await,
        SkillOptSubcommand::Resume(args) => resume::execute_resume(args).await,
        SkillOptSubcommand::Status(args) => status::execute_status(args).await,
        SkillOptSubcommand::Inspect(args) => inspect::execute_inspect(args).await,
        SkillOptSubcommand::Export(args) => export::execute_export(args).await,
    }
}

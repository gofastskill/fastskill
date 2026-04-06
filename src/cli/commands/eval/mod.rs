//! Eval command group for skill quality assurance

pub mod report;
pub mod run;
pub mod score;
pub mod validate;

use crate::cli::error::CliResult;
use clap::{Args, Subcommand};

/// Eval command group
#[derive(Debug, Args)]
#[command(
    about = "Evaluation commands for skill quality assurance",
    after_help = "Examples:\n  fastskill eval validate\n  fastskill eval run --agent codex --output-dir /tmp/evals\n  fastskill eval run --agent agent --output-dir /tmp/evals"
)]
pub struct EvalCommand {
    #[command(subcommand)]
    pub command: EvalSubcommand,
}

/// Eval subcommands
#[derive(Debug, Subcommand)]
pub enum EvalSubcommand {
    /// Validate eval configuration and files
    #[command(
        about = "Validate eval configuration and files",
        after_help = "Examples:\n  fastskill eval validate\n  fastskill eval validate --agent codex\n  fastskill eval validate --agent agent"
    )]
    Validate(validate::ValidateArgs),

    /// Run eval cases against an agent
    #[command(
        about = "Run eval cases against an agent",
        after_help = "Examples:\n  fastskill eval run --agent codex --output-dir /tmp/evals"
    )]
    Run(run::RunArgs),

    /// Show a report for a completed eval run
    #[command(
        about = "Show a report for a completed eval run",
        after_help = "Examples:\n  fastskill eval report --run-dir /tmp/evals/2026-04-01T14-32-10Z"
    )]
    Report(report::ReportArgs),

    /// Re-score saved eval artifacts without running the agent again
    #[command(
        about = "Re-score saved eval artifacts without running the agent again",
        after_help = "Examples:\n  fastskill eval score --run-dir /tmp/evals/2026-04-01T14-32-10Z"
    )]
    Score(score::ScoreArgs),
}

/// Execute the eval command group
pub async fn execute_eval(args: EvalCommand) -> CliResult<()> {
    match args.command {
        EvalSubcommand::Validate(args) => validate::execute_validate(args).await,
        EvalSubcommand::Run(args) => run::execute_run(args).await,
        EvalSubcommand::Report(args) => report::execute_report(args).await,
        EvalSubcommand::Score(args) => score::execute_score(args).await,
    }
}

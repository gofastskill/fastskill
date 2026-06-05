//! Eval command group for skill quality assurance

pub mod report;
pub mod run;
pub mod score;
pub mod validate;

/// Eval command group
#[allow(dead_code)]
#[derive(Debug)]
pub struct EvalCommand {
    pub command: EvalSubcommand,
}

/// Eval subcommands
#[allow(dead_code)]
#[derive(Debug)]
pub enum EvalSubcommand {
    /// Validate eval configuration and files
    Validate(validate::ValidateArgs),

    /// Run eval cases against an agent
    Run(run::RunArgs),

    /// Show a report for a completed eval run
    Report(report::ReportArgs),

    /// Re-score saved eval artifacts without running the agent again
    Score(score::ScoreArgs),
}

//! Eval report subcommand - artifact summary and formatting

use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult};
use clap::Args;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::read_summary;
use std::path::PathBuf;

/// Arguments for `fastskill eval report`
#[derive(Debug, Args)]
#[command(
    about = "Show a report for a completed eval run",
    after_help = "Examples:\n  fastskill eval report --run-dir /tmp/evals/2026-04-01T14-32-10Z\n  fastskill eval report --run-dir ./evals/2026-04-01T14-32-10Z --json"
)]
pub struct ReportArgs {
    /// Path to the specific run directory
    #[arg(long, required = true)]
    pub run_dir: PathBuf,

    /// Output format: table, json, grid, xml (default: table)
    #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    #[arg(long, help = "Shorthand for --format json")]
    pub json: bool,
}

/// Execute the `eval report` command
pub async fn execute_report(args: ReportArgs) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Run directory does not exist: {}",
            args.run_dir.display()
        )));
    }

    let summary = read_summary(&args.run_dir).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Failed to read summary.json: {}",
            e
        ))
    })?;

    if use_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        println!("Eval Report");
        println!("  run_dir: {}", args.run_dir.display());
        println!("  agent: {}", summary.agent);
        if let Some(ref model) = summary.model {
            println!("  model: {}", model);
        }
        println!(
            "  result: {}",
            if summary.suite_pass {
                "PASSED"
            } else {
                "FAILED"
            }
        );
        println!("  cases: {}/{} passed", summary.passed, summary.total_cases);

        if !summary.cases.is_empty() {
            println!("\nCase Results:");
            for case in &summary.cases {
                println!("  [{}] {}", case.status, case.id);
            }
        }
    }

    Ok(())
}

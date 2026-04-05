//! Eval score subcommand - offline re-scoring from saved artifacts

use crate::cli::error::{CliError, CliResult};
use clap::Args;
use fastskill::eval::artifacts::{read_summary, write_summary, CaseStatus};
use fastskill::eval::checks::load_checks;
use std::path::PathBuf;

/// Arguments for `fastskill eval score`
#[derive(Debug, Args)]
#[command(
    about = "Re-score saved eval artifacts without running the agent again",
    after_help = "Examples:\n  fastskill eval score --run-dir /tmp/evals/2026-04-01T14-32-10Z\n  fastskill eval score --run-dir ./evals/2026-04-01T14-32-10Z --json"
)]
pub struct ScoreArgs {
    /// Path to the run directory to re-score
    #[arg(long, required = true)]
    pub run_dir: PathBuf,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    #[arg(long)]
    pub no_fail: bool,
}

/// Execute the `eval score` command
pub async fn execute_score(args: ScoreArgs) -> CliResult<()> {
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Run directory does not exist: {}",
            args.run_dir.display()
        )));
    }

    // Read existing summary
    let mut summary = read_summary(&args.run_dir).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Failed to read summary.json: {}",
            e
        ))
    })?;

    // Validate that we have usable paths
    let checks_path = summary.checks_path.as_ref().ok_or_else(|| {
        CliError::Config(
            "EVAL_ARTIFACTS_CORRUPT: summary.json lacks checks_path - cannot re-score".to_string(),
        )
    })?;

    if !checks_path.exists() {
        return Err(CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: checks_path '{}' does not exist",
            checks_path.display()
        )));
    }

    // Load checks
    let checks = load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?;

    // Read existing case artifacts and re-score
    let mut new_passed = 0;
    let mut new_failed = 0;

    let mut updated_cases = summary.cases.clone();

    for case_summary in &mut updated_cases {
        let case_dir = args.run_dir.join(&case_summary.id);
        if !case_dir.exists() {
            continue;
        }

        let stdout_path = case_dir.join("stdout.txt");
        let trace_path = case_dir.join("trace.jsonl");

        let stdout_content = std::fs::read_to_string(&stdout_path).unwrap_or_default();
        let trace_jsonl = std::fs::read_to_string(&trace_path).unwrap_or_default();

        let check_results = fastskill::eval::checks::run_checks(
            &checks,
            &stdout_content,
            &trace_jsonl,
            &summary.skill_project_root,
        );

        let all_passed = check_results.iter().all(|r| r.passed);
        case_summary.status = if all_passed {
            CaseStatus::Passed
        } else {
            CaseStatus::Failed
        };

        if all_passed {
            new_passed += 1;
        } else {
            new_failed += 1;
        }
    }

    summary.passed = new_passed;
    summary.failed = new_failed;
    summary.suite_pass = new_failed == 0;
    summary.cases = updated_cases;

    // Update summary.json
    write_summary(&args.run_dir, &summary)
        .map_err(|e| CliError::Config(format!("Failed to write updated summary.json: {}", e)))?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        println!("Re-scoring complete");
        println!(
            "  result: {}",
            if summary.suite_pass {
                "PASSED"
            } else {
                "FAILED"
            }
        );
        println!("  cases: {}/{} passed", summary.passed, summary.total_cases);
    }

    if !summary.suite_pass && !args.no_fail {
        return Err(CliError::Config(format!(
            "Eval suite failed: {}/{} cases passed after re-scoring",
            summary.passed, summary.total_cases
        )));
    }

    Ok(())
}

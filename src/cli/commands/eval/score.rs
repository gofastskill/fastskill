//! Eval score subcommand - offline re-scoring from saved artifacts

use crate::cli::commands::common::validate_format_args;
use crate::cli::error::{CliError, CliResult};
use clap::Args;
use fastskill::eval::artifacts::{read_summary, write_summary, CaseStatus, TrialResult};
use fastskill::eval::checks::load_checks;
use fastskill::OutputFormat;
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

    /// Output format: table, json, grid, xml (default: table)
    #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    #[arg(long, help = "Shorthand for --format json")]
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    #[arg(long)]
    pub no_fail: bool,
}

/// Execute the `eval score` command
pub async fn execute_score(args: ScoreArgs) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

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
    let pass_threshold = summary.pass_threshold.unwrap_or(1.0);

    for case_summary in &mut updated_cases {
        let case_dir = args.run_dir.join(&case_summary.id);
        if !case_dir.exists() {
            continue;
        }

        let mut trial_dirs: Vec<(u32, PathBuf)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&case_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if let Some(suffix) = name.strip_prefix("trial-") {
                            if let Ok(id) = suffix.parse::<u32>() {
                                trial_dirs.push((id, path));
                            }
                        }
                    }
                }
            }
        }
        trial_dirs.sort_by_key(|(id, _)| *id);

        // Legacy fallback: treat case root as a single trial.
        if trial_dirs.is_empty() {
            trial_dirs.push((1, case_dir.clone()));
        }

        let mut trials: Vec<TrialResult> = Vec::with_capacity(trial_dirs.len());
        for (trial_id, tdir) in &trial_dirs {
            let stdout_path = tdir.join("stdout.txt");
            let trace_path = tdir.join("trace.jsonl");

            let stdout_content = std::fs::read_to_string(&stdout_path).unwrap_or_default();
            let trace_jsonl = std::fs::read_to_string(&trace_path).unwrap_or_default();

            let check_results = fastskill::eval::checks::run_checks(
                &checks,
                &stdout_content,
                &trace_jsonl,
                &summary.skill_project_root,
            );
            let all_passed = check_results.iter().all(|r| r.passed);

            trials.push(TrialResult {
                trial_id: *trial_id,
                status: if all_passed {
                    CaseStatus::Passed
                } else {
                    CaseStatus::Failed
                },
                command_count: None,
                input_tokens: None,
                output_tokens: None,
                check_results,
                error_message: None,
            });
        }

        let pass_count = trials
            .iter()
            .filter(|t| t.status == CaseStatus::Passed)
            .count() as u32;
        let total_trials = trials.len().max(1) as u32;
        let pass_rate = pass_count as f64 / total_trials as f64;

        case_summary.trials = trials;
        case_summary.pass_count = Some(pass_count);
        case_summary.total_trials = Some(total_trials);
        case_summary.pass_rate = Some(pass_rate);
        case_summary.status = if pass_rate >= pass_threshold {
            CaseStatus::Passed
        } else {
            CaseStatus::Failed
        };

        if case_summary.status == CaseStatus::Passed {
            new_passed += 1;
        } else {
            new_failed += 1;
        }
    }

    summary.passed = new_passed;
    summary.failed = new_failed;
    summary.suite_pass_rate = if summary.total_cases == 0 {
        Some(0.0)
    } else {
        Some(new_passed as f64 / summary.total_cases as f64)
    };
    summary.suite_pass = new_failed == 0;
    summary.cases = updated_cases;

    // Update summary.json
    write_summary(&args.run_dir, &summary)
        .map_err(|e| CliError::Config(format!("Failed to write updated summary.json: {}", e)))?;

    if use_json {
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

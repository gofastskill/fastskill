//! Eval run subcommand - case execution orchestration

use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult};
use aikit_sdk::{is_agent_available, is_runnable, runnable_agents};
use chrono::Utc;
use clap::Args;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::eval::artifacts::{
    allocate_run_dir, write_case_trials_summary, write_summary, write_trial_artifacts, CaseStatus,
    CaseSummary, CaseTrialsResult, SummaryResult, TrialResult,
};
use fastskill_core::eval::checks::load_checks;
use fastskill_core::eval::config::resolve_eval_config;
use fastskill_core::eval::runner::{AikitEvalRunner, CaseRunOptions, EvalRunner};
use fastskill_core::eval::suite::load_suite;
use fastskill_core::OutputFormat;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Arguments for `fastskill eval run`
#[derive(Debug, Args)]
#[command(
    about = "Run eval cases against an agent",
    after_help = "Examples:\n  fastskill eval run --agent codex --output-dir /tmp/evals\n  fastskill eval run --agent agent --output-dir ./evals --case my-case\n  fastskill eval run --agent codex --output-dir ./evals --tag basic"
)]
pub struct RunArgs {
    /// Agent to use for execution (required)
    #[arg(long, required = true, value_parser = validate_agent_key_for_run)]
    pub agent: String,

    /// Output directory for artifacts (required)
    #[arg(long, required = true)]
    pub output_dir: PathBuf,

    /// Optional model override forwarded to the agent
    #[arg(long)]
    pub model: Option<String>,

    /// Filter: run only the case with this ID
    #[arg(long)]
    pub case: Option<String>,

    /// Filter: run only cases with this tag
    #[arg(long)]
    pub tag: Option<String>,

    /// Output format: table, json, grid, xml (default: table)
    #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    #[arg(long, help = "Shorthand for --format json")]
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    #[arg(long)]
    pub no_fail: bool,

    /// Override trials per case from config
    #[arg(long)]
    pub trials: Option<u32>,

    /// Enable CI mode: exit non-zero if suite pass rate below threshold
    #[arg(long)]
    pub ci: bool,

    /// Override pass threshold (0.0-1.0)
    #[arg(long)]
    pub threshold: Option<f64>,
}

fn validate_agent_key_for_run(s: &str) -> Result<String, String> {
    if is_runnable(s) {
        Ok(s.to_string())
    } else {
        Err(format!(
            "'{}' is not a supported agent. Supported: {}",
            s,
            runnable_agents().join(", ")
        ))
    }
}

/// Execute the `eval run` command using the default aikit-backed runner.
pub async fn execute_run(args: RunArgs) -> CliResult<()> {
    execute_run_with_runner(args, Arc::new(AikitEvalRunner)).await
}

/// Execute `eval run` with an injectable [`EvalRunner`] (tests or future adapters).
pub async fn execute_run_with_runner<R: EvalRunner + 'static>(
    args: RunArgs,
    runner: Arc<R>,
) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    let resolution = resolve_project_file(&current_dir);
    if !resolution.found {
        return Err(CliError::Config(
            "EVAL_CONFIG_MISSING: No skill-project.toml found. Run 'fastskill init' first."
                .to_string(),
        ));
    }

    let project_root = resolution
        .path
        .parent()
        .unwrap_or(&current_dir)
        .to_path_buf();

    let eval_config = resolve_eval_config(&resolution.path, &project_root)
        .map_err(|e| CliError::Config(e.to_string()))?;

    let trials_per_case = args.trials.unwrap_or(eval_config.trials_per_case);
    if !(1..=1000).contains(&trials_per_case) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_TRIALS_CONFIG: trials must be in range [1, 1000], got {}",
            trials_per_case
        )));
    }

    let pass_threshold = args.threshold.unwrap_or(eval_config.pass_threshold);
    if !(0.0..=1.0).contains(&pass_threshold) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_THRESHOLD: threshold must be in range [0.0, 1.0], got {}",
            pass_threshold
        )));
    }

    // Check agent availability
    if eval_config.fail_on_missing_agent && !is_agent_available(&args.agent) {
        return Err(CliError::Config(format!(
            "EVAL_AGENT_UNAVAILABLE: Agent '{}' is not available. Install it first.",
            args.agent
        )));
    }

    // Load suite
    let mut suite =
        load_suite(&eval_config.prompts_path).map_err(|e| CliError::Config(e.to_string()))?;

    // Apply filters
    if let Some(ref case_id) = args.case {
        suite = suite.filter_by_id(case_id);
        if suite.cases.is_empty() {
            return Err(CliError::Config(format!(
                "No case found with id '{}'",
                case_id
            )));
        }
    }
    if let Some(ref tag) = args.tag {
        suite = suite.filter_by_tag(tag);
        if suite.cases.is_empty() {
            return Err(CliError::Config(format!(
                "No cases found with tag '{}'",
                tag
            )));
        }
    }

    // Load checks if configured
    let checks = if let Some(ref checks_path) = eval_config.checks_path {
        load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?
    } else {
        vec![]
    };

    let total_trial_runs = (suite.cases.len() as u64) * (trials_per_case as u64);
    if total_trial_runs >= 100 && !use_json {
        eprintln!(
            "warning: EVAL_COST_WARNING: running {} case(s) × {} trial(s) = {} total trial runs",
            suite.cases.len(),
            trials_per_case,
            total_trial_runs
        );
    }

    // Allocate run directory
    let run_id = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    std::fs::create_dir_all(&args.output_dir).map_err(|e| {
        CliError::Config(format!(
            "Failed to create output directory '{}': {}",
            args.output_dir.display(),
            e
        ))
    })?;
    let run_dir =
        allocate_run_dir(&args.output_dir, &run_id).map_err(|e| CliError::Config(e.to_string()))?;

    let run_opts = CaseRunOptions {
        agent_key: args.agent.clone(),
        model: args.model.clone(),
        project_root: project_root.clone(),
        timeout_seconds: eval_config.timeout_seconds,
        pass_threshold,
    };

    if !use_json {
        eprintln!(
            "Running {} eval case(s) with agent '{}' ({} trial(s) per case)...",
            suite.cases.len(),
            args.agent,
            trials_per_case
        );
    }

    let mut case_summaries = Vec::new();

    for case in &suite.cases {
        if !use_json {
            eprintln!("  Running case '{}'...", case.id);
        }

        let max_parallel = eval_config
            .parallel
            .unwrap_or_else(|| num_cpus::get().max(1) as u32)
            .max(1) as usize;
        let semaphore = Arc::new(Semaphore::new(max_parallel));
        let mut join_set: JoinSet<
            CliResult<(
                u32,
                fastskill_core::eval::runner::CaseRunOutput,
                fastskill_core::eval::artifacts::CaseResult,
                String,
            )>,
        > = JoinSet::new();

        for trial_id in 1..=trials_per_case {
            let permit = Arc::clone(&semaphore);
            let runner = Arc::clone(&runner);
            let case_clone = case.clone();
            let opts_clone = run_opts.clone();
            let checks_vec = checks.clone();

            join_set.spawn(async move {
                let Ok(_permit) = permit.acquire().await else {
                    return Err(CliError::Config(
                        "EVAL_PARALLEL_EXHAUSTION: semaphore closed".to_string(),
                    ));
                };
                let (out, res, trace) =
                    runner.run_case(&case_clone, &opts_clone, &checks_vec).await;
                Ok((trial_id, out, res, trace))
            });
        }

        let mut trials: Vec<TrialResult> = Vec::with_capacity(trials_per_case as usize);
        let mut pass_count: u32 = 0;
        let mut command_count_sum: usize = 0;
        let mut input_tokens_sum: u64 = 0;
        let mut output_tokens_sum: u64 = 0;
        let mut saw_any_command_count = false;
        let mut saw_any_input_tokens = false;
        let mut saw_any_output_tokens = false;

        while let Some(joined) = join_set.join_next().await {
            let (trial_id, out, case_result, trace_jsonl) = joined.map_err(|e| {
                CliError::Config(format!(
                    "EVAL_PARALLEL_EXHAUSTION: trial task failed: {}",
                    e
                ))
            })??;

            let trial = TrialResult {
                trial_id,
                status: case_result.status.clone(),
                command_count: case_result.command_count,
                input_tokens: case_result.input_tokens,
                output_tokens: case_result.output_tokens,
                check_results: case_result.check_results.clone(),
                error_message: case_result.error_message.clone(),
            };

            if trial.status == CaseStatus::Passed {
                pass_count += 1;
            }
            if let Some(cc) = trial.command_count {
                saw_any_command_count = true;
                command_count_sum = command_count_sum.saturating_add(cc);
            }
            if let Some(it) = trial.input_tokens {
                saw_any_input_tokens = true;
                input_tokens_sum = input_tokens_sum.saturating_add(it);
            }
            if let Some(ot) = trial.output_tokens {
                saw_any_output_tokens = true;
                output_tokens_sum = output_tokens_sum.saturating_add(ot);
            }

            // Write trial artifacts immediately (keeps memory bounded).
            if let Err(e) = write_trial_artifacts(
                &run_dir,
                &case.id,
                trial_id,
                &out.stdout,
                &out.stderr,
                &trace_jsonl,
                &trial,
            ) {
                if !use_json {
                    eprintln!(
                        "  warning: failed to write artifacts for case '{}' trial {}: {}",
                        case.id, trial_id, e
                    );
                }
            }

            trials.push(trial);
        }

        trials.sort_by_key(|t| t.trial_id);
        let total_trials = trials_per_case;
        let pass_rate = pass_count as f64 / total_trials as f64;
        let aggregated_status = if pass_rate >= pass_threshold {
            CaseStatus::Passed
        } else {
            CaseStatus::Failed
        };

        let aggregated = CaseTrialsResult {
            id: case.id.clone(),
            trials: trials.clone(),
            aggregated_status: aggregated_status.clone(),
            pass_count,
            total_trials,
            pass_rate,
        };

        if let Err(e) = write_case_trials_summary(&run_dir, &case.id, &aggregated) {
            if !use_json {
                eprintln!(
                    "  warning: failed to write aggregated summary for case '{}': {}",
                    case.id, e
                );
            }
        }

        case_summaries.push(CaseSummary {
            id: case.id.clone(),
            status: aggregated_status,
            command_count: if saw_any_command_count {
                Some(command_count_sum)
            } else {
                None
            },
            input_tokens: if saw_any_input_tokens {
                Some(input_tokens_sum)
            } else {
                None
            },
            output_tokens: if saw_any_output_tokens {
                Some(output_tokens_sum)
            } else {
                None
            },
            pass_count: Some(pass_count),
            total_trials: Some(total_trials),
            pass_rate: Some(pass_rate),
            trials,
        });
    }

    let passed = case_summaries
        .iter()
        .filter(|r| r.status == fastskill_core::eval::artifacts::CaseStatus::Passed)
        .count();
    let failed = case_summaries.len() - passed;
    let suite_pass_rate = if case_summaries.is_empty() {
        0.0
    } else {
        passed as f64 / case_summaries.len() as f64
    };
    let suite_pass = if args.ci {
        suite_pass_rate >= pass_threshold
    } else {
        failed == 0
    };

    let summary = SummaryResult {
        suite_pass,
        suite_pass_rate: Some(suite_pass_rate),
        agent: args.agent.clone(),
        model: args.model.clone(),
        total_cases: case_summaries.len(),
        passed,
        failed,
        trials_per_case: Some(trials_per_case),
        parallel: eval_config.parallel,
        pass_threshold: Some(pass_threshold),
        run_dir: run_dir.clone(),
        checks_path: eval_config.checks_path.clone(),
        skill_project_root: project_root.clone(),
        cases: case_summaries,
    };

    // Write summary
    if let Err(e) = write_summary(&run_dir, &summary) {
        if !use_json {
            eprintln!("warning: failed to write summary.json: {}", e);
        }
    }

    if use_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        println!(
            "\nEval run complete: {}/{} passed",
            passed, summary.total_cases
        );
        println!("  run_dir: {}", run_dir.display());
        if suite_pass {
            if args.ci {
                println!(
                    "  result: PASSED (suite pass rate {:.0}% ≥ {:.0}% threshold)",
                    suite_pass_rate * 100.0,
                    pass_threshold * 100.0
                );
            } else {
                println!("  result: PASSED");
            }
        } else {
            if args.ci {
                println!(
                    "  result: FAILED (suite pass rate {:.0}% < {:.0}% threshold)",
                    suite_pass_rate * 100.0,
                    pass_threshold * 100.0
                );
            } else {
                println!("  result: FAILED ({} case(s) failed)", failed);
            }
        }
    }

    let should_fail = if args.ci {
        suite_pass_rate < pass_threshold
    } else {
        !suite_pass
    };

    if should_fail && !args.no_fail {
        return Err(CliError::Config(format!(
            "Eval suite failed: {}/{} cases passed (threshold={})",
            passed, summary.total_cases, pass_threshold
        )));
    }

    Ok(())
}

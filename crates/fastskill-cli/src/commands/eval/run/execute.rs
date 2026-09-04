//! Eval run subcommand - the run itself.
//!
//! Split from the argument surface in [`super`] only because the two together
//! outgrow the repo's per-file line cap; the seam is "describe the command"
//! versus "carry it out".

use crate::commands::common::{runtime_selection_error_to_cli, validate_eval_format_args};
use crate::error::{CliError, CliResult};
use crate::runtime_selector::RuntimeSelectionInput;
use aikit_sdk::is_agent_available;
use chrono::Utc;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::{
    aggregate_trials, allocate_run_dir, read_summary, skill_git_identity,
    write_case_trials_summary, write_summary, write_trial_artifacts, CaseStatus, CaseSummary,
    IsolationReport, SummaryResult, TrialArtifacts, TrialResult,
};
use fastskill_evals::checks::load_checks;
use fastskill_evals::judge::{JudgeRunOptions, SuitePassRule};
use fastskill_evals::resolve_eval_config;
use fastskill_evals::runner::{AikitEvalRunner, CaseRunOptions, EvalRunner};
use fastskill_evals::suite::load_suite;

use crate::commands::eval::isolation::{render_isolation_line, resolve_isolation_mode};
use crate::commands::eval::observability::scoreable_runtimes;
use std::env;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::RunArgs;

/// Pass rate over every case in a run.
fn case_rate(passed: usize, total_cases: usize) -> f64 {
    if total_cases == 0 {
        0.0
    } else {
        passed as f64 / total_cases as f64
    }
}

/// The verdict `eval run` reports and exits on.
///
/// Without `--judge` this is exactly the rule the run has always applied. With
/// it, `summary.suite_pass` has been rewritten by the engine, which asks a
/// deliberately narrower question — every *scored* case must pass, so a case
/// the agent never completed is outside its scope. `eval run` still fails on
/// such a case: adding `--judge` must not turn a red run green.
fn run_verdict(summary: &SummaryResult, ci: bool, pass_threshold: f64) -> bool {
    if ci {
        case_rate(summary.passed, summary.total_cases) >= pass_threshold
    } else {
        summary.failed == 0
    }
}

/// Execute the `eval run` command using the default aikit-backed runner.
pub async fn execute_run(args: RunArgs) -> CliResult<()> {
    execute_run_with_runner(args, Arc::new(AikitEvalRunner::new())).await
}

/// Execute `eval run` with an injectable [`EvalRunner`] (tests or future adapters).
pub async fn execute_run_with_runner<R: EvalRunner + 'static>(
    args: RunArgs,
    runner: Arc<R>,
) -> CliResult<()> {
    let format = validate_eval_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    // Resolve runtime selection first so missing --agent is caught before project-file checks.
    let input = RuntimeSelectionInput::from(&args);
    let selection = crate::runtime_selector::resolve_runtime_selection(&input)
        .map_err(runtime_selection_error_to_cli)?;

    let runtimes = match selection {
        Some(sel) => sel.runtimes,
        None => {
            return Err(CliError::Config(
                "RUNTIME_NO_SELECTION: No runtime selected. Use --agent <id> or --all to \
                 specify a target runtime."
                    .to_string(),
            ));
        }
    };

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

    // Asked once, before anything runs: a later answer would describe a
    // checkout that has moved on. `None` when the project is not in git, never
    // a guess (scorecard R2).
    let skill_identity = skill_git_identity(&project_root);

    let eval_config = resolve_eval_config(&resolution.path, &project_root)
        .map_err(|e| CliError::Config(e.to_string()))?;

    let isolation = resolve_isolation_mode(args.no_isolation, &resolution.path, &project_root)?;

    // Validate against the raw parsed value so the error echoes exactly what the
    // user typed (e.g. a negative `-3`), not a wrapped/clamped integer.
    let trials_raw = args
        .trials
        .unwrap_or(i64::from(eval_config.trials_per_case));
    if !(1..=1000).contains(&trials_raw) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_TRIALS_CONFIG: trials must be in range [1, 1000], got {}",
            trials_raw
        )));
    }
    // Safe: validated to be within [1, 1000] above.
    let trials_per_case = trials_raw as u32;

    let pass_threshold = args.threshold.unwrap_or(eval_config.pass_threshold);
    if !(0.0..=1.0).contains(&pass_threshold) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_THRESHOLD: threshold must be in range [0.0, 1.0], got {}",
            pass_threshold
        )));
    }

    // Load suite and apply filters (same for all runtimes).
    let mut suite =
        load_suite(&eval_config.prompts_path).map_err(|e| CliError::Config(e.to_string()))?;

    // Reject a suite that parsed to zero cases before any filtering (the
    // --case/--tag filters below already guard their own empty results). The
    // default verdict is `failed == 0`, so an empty suite — a header-only CSV
    // or a wrong prompts path pointing at a template — would otherwise report
    // `0/0 passed · PASSED` and exit 0, green-lighting CI while running
    // nothing at all.
    if suite.cases.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_EMPTY_SUITE: suite '{}' contains zero cases",
            eval_config.prompts_path.display()
        )));
    }

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

    // Load checks if configured.
    let checks = if let Some(ref checks_path) = eval_config.checks_path {
        load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?
    } else {
        vec![]
    };

    // R10: a required check whose evidence a backend never emits makes the
    // suite unscoreable there. Ask before the first trial — every one after
    // this point costs a provider call, and none of them would produce a score.
    // Runtimes that cannot score are dropped rather than failing the whole
    // invocation, so `--all` is not held hostage by one text-only decoder;
    // naming a backend with `--agent` leaves nothing to fall back to and fails.
    let (runtimes, exclusion_notice) = scoreable_runtimes(
        &runtimes,
        &suite.cases,
        &checks,
        matches!(
            isolation,
            fastskill_evals::runner::IsolationMode::Isolated { .. }
        ),
    )?;
    if let Some(notice) = exclusion_notice {
        // stderr even under --json: the machine-readable summary covers the
        // runtimes that ran, and the ones that did not must not vanish.
        eprintln!("{}", notice);
    }

    let total_trial_runs =
        (suite.cases.len() as u64) * (trials_per_case as u64) * (runtimes.len() as u64);
    if total_trial_runs >= 100 && !use_json {
        eprintln!(
            "warning: EVAL_COST_WARNING: running {} case(s) × {} trial(s) × {} agent(s) = {} total trial runs",
            suite.cases.len(),
            trials_per_case,
            runtimes.len(),
            total_trial_runs
        );
    }

    // Allocate run base directory.
    let run_id = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    std::fs::create_dir_all(&args.output_dir).map_err(|e| {
        CliError::Config(format!(
            "Failed to create output directory '{}': {}",
            args.output_dir.display(),
            e
        ))
    })?;
    let run_dir_base =
        allocate_run_dir(&args.output_dir, &run_id).map_err(|e| CliError::Config(e.to_string()))?;

    let mut all_summaries: Vec<SummaryResult> = Vec::new();
    let mut any_agent_failed = false;
    let mut judge_errors: u32 = 0;

    for agent_key in &runtimes {
        // Per-agent subdirectory.
        let run_dir = run_dir_base.join(agent_key);
        std::fs::create_dir_all(&run_dir).map_err(|e| {
            CliError::Config(format!(
                "Failed to create run directory '{}': {}",
                run_dir.display(),
                e
            ))
        })?;

        // Check agent availability.
        if eval_config.fail_on_missing_agent && !is_agent_available(agent_key) {
            return Err(CliError::Config(format!(
                "EVAL_AGENT_UNAVAILABLE: Agent '{}' is not available. Install it first.",
                agent_key
            )));
        }

        let run_opts = CaseRunOptions {
            agent_key: agent_key.clone(),
            model: args.model.clone(),
            project_root: project_root.clone(),
            timeout_seconds: eval_config.timeout_seconds,
            pass_threshold,
            isolation: isolation.clone(),
            // A failed case's scratch workspace is moved here so it survives
            // for debugging; successful workspaces are deleted.
            retain_workspace_in: Some(run_dir.join("workspaces")),
        };

        if !use_json {
            eprintln!(
                "Running {} eval case(s) with agent '{}' ({} trial(s) per case)...",
                suite.cases.len(),
                agent_key,
                trials_per_case
            );
        }

        let mut case_summaries = Vec::new();
        // First observed per-case isolation report stands in for the run: the
        // backend and mechanism are constant across an agent's run, and a
        // per-case copy lives in each trial's artifacts.
        let mut run_isolation: Option<IsolationReport> = None;

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
                    fastskill_evals::runner::CaseRunOutput,
                    fastskill_evals::artifacts::CaseResult,
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

                if run_isolation.is_none() {
                    run_isolation = out.isolation.clone();
                }

                let trial = TrialResult {
                    trial_id,
                    status: case_result.status.clone(),
                    command_count: case_result.command_count,
                    input_tokens: case_result.input_tokens,
                    output_tokens: case_result.output_tokens,
                    check_results: case_result.check_results.clone(),
                    error_message: case_result.error_message.clone(),
                    // R5: every field the runner already held and the artifact
                    // previously narrowed away. Copied verbatim — `eval run` is
                    // the writer, and a writer that reshapes what the runner
                    // measured is a second source of truth.
                    exit_code: case_result.exit_code,
                    terminal: case_result.terminal.clone(),
                    cost_usd: case_result.cost_usd,
                    tokens: case_result.tokens.clone(),
                    skill_path: case_result.skill_path.clone(),
                    // Set by `eval judge`, never by the runner: no judge has
                    // seen this trial yet.
                    judge_excluded: false,
                };

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

                if let Err(e) = write_trial_artifacts(
                    &run_dir,
                    &case.id,
                    trial_id,
                    &TrialArtifacts {
                        stdout: &out.stdout,
                        stderr: &out.stderr,
                        trace_jsonl: &trace_jsonl,
                        // `None` when the trial had no seeded workspace to diff
                        // against, and then no `workspace.diff` is written at
                        // all: a judge must see "no evidence", never an empty
                        // diff claiming nothing changed.
                        workspace_diff: out.workspace_diff.as_deref(),
                        result: &trial,
                    },
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

            // R4: one fold, shared with the engine. Errored trials leave the
            // ratio entirely, and a case with none left scores `error` rather
            // than a 0% fail. Re-deriving the rate here would let the CLI and
            // the engine disagree about the same run.
            let aggregated = aggregate_trials(&case.id, trials, trials_per_case, pass_threshold);
            let trials = aggregated.trials.clone();
            let aggregated_status = aggregated.aggregated_status.clone();
            let total_trials = aggregated.total_trials;
            let pass_rate = aggregated.pass_rate;
            let pass_count = aggregated.pass_count;

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
                error_count: Some(aggregated.error_count),
                scored_trials: Some(aggregated.scored_trials),
                // Recorded so `eval score` can rebuild the same effective check
                // list offline: under R7 this column generates an implicit
                // skill-invocation check, and a scorer that cannot see it drops
                // that check and reports a different verdict than the run.
                should_trigger: Some(case.should_trigger),
                judge_excluded_count: Some(aggregated.judge_excluded_count),
                scores: aggregated.scores.clone(),
                trials,
            });
        }

        let passed = case_summaries
            .iter()
            .filter(|r| r.status == CaseStatus::Passed)
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
            agent: agent_key.clone(),
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
            isolation: run_isolation,
            // Judge totals belong to `eval judge`, which rewrites them into
            // this file after it has judged. The runner reports no judgment.
            judge_errors: None,
            judge_skipped_trials: None,
            judge_tokens: None,
            judge_cost_usd: None,
            // Recorded now, at run time: the skill on disk when a scorecard is
            // built later is not the skill that ran (scorecard R2).
            skill_git_sha: skill_identity.as_ref().map(|i| i.sha.clone()),
            skill_dirty: skill_identity.as_ref().map(|i| i.dirty),
            cases: case_summaries,
        };

        if let Err(e) = write_summary(&run_dir, &summary) {
            if !use_json {
                eprintln!("warning: failed to write summary.json: {}", e);
            }
        }

        // R13: the same judging function `eval judge` calls, run right after
        // this agent's own scoring. It rewrites the run's artifacts in place,
        // so the summary reported from here on is re-read from the file the
        // judge left rather than the one held in memory.
        let summary = if args.judge {
            let opts = JudgeRunOptions {
                judge_model: args.judge_model.clone(),
                parallel: eval_config.parallel,
                suite_rule: if args.ci {
                    SuitePassRule::RateAtLeast(pass_threshold)
                } else {
                    SuitePassRule::AllCases
                },
                ..Default::default()
            };
            let report = crate::commands::eval::judge::judge_run(&run_dir, &suite, &opts).await?;
            judge_errors += report.errors;
            if !use_json {
                if report.judges.is_empty() {
                    eprintln!("  no [[judge]] declared in the checks file; nothing was judged");
                } else {
                    eprintln!("  judged agent '{}'", agent_key);
                    crate::commands::eval::judge::render_report(&report);
                }
            }
            read_summary(&run_dir).map_err(|e| {
                CliError::Config(format!(
                    "EVAL_ARTIFACTS_CORRUPT: failed to re-read summary.json after judging: {}",
                    e
                ))
            })?
        } else {
            summary
        };

        if !run_verdict(&summary, args.ci, pass_threshold) {
            any_agent_failed = true;
        }

        all_summaries.push(summary);
    }

    // Output results.
    if use_json {
        if all_summaries.len() == 1 {
            crate::outln!(
                "{}",
                serde_json::to_string_pretty(&all_summaries[0]).unwrap_or_default()
            );
        } else {
            crate::outln!(
                "{}",
                serde_json::to_string_pretty(&all_summaries).unwrap_or_default()
            );
        }
    } else {
        for summary in &all_summaries {
            // Over every case in the run, which is the question `run_verdict`
            // asks. After judging `summary.suite_pass_rate` answers a narrower
            // one (scored cases only); the two must not be reported as one
            // number.
            let suite_pass_rate = case_rate(summary.passed, summary.total_cases);
            let verdict = run_verdict(summary, args.ci, pass_threshold);
            crate::outln!(
                "\nEval run complete for agent '{}': {}/{} passed",
                summary.agent,
                summary.passed,
                summary.total_cases
            );
            crate::outln!("  run_dir: {}", summary.run_dir.display());
            crate::outln!("  {}", render_isolation_line(summary.isolation.as_ref()));
            if let Some(iso) = &summary.isolation {
                if !iso.ambient_skills.is_empty() {
                    crate::outln!(
                        "  ambient skills visible to agent: {}",
                        iso.ambient_skills.join(", ")
                    );
                }
            }
            if verdict {
                if args.ci {
                    crate::outln!(
                        "  result: PASSED (suite pass rate {:.0}% ≥ {:.0}% threshold)",
                        suite_pass_rate * 100.0,
                        pass_threshold * 100.0
                    );
                } else {
                    crate::outln!("  result: PASSED");
                }
            } else if args.ci {
                crate::outln!(
                    "  result: FAILED (suite pass rate {:.0}% < {:.0}% threshold)",
                    suite_pass_rate * 100.0,
                    pass_threshold * 100.0
                );
            } else {
                crate::outln!("  result: FAILED ({} case(s) failed)", summary.failed);
            }
        }
    }

    // R13: a judge that could not render a judgment left a gap in the
    // measurement. `--no-fail` suppresses a failing verdict, never a missing
    // one — reporting an outage as a score is the one thing this must not do.
    if judge_errors > 0 {
        return Err(CliError::Config(format!(
            "EVAL_JUDGE_ERRORS: {} judgment(s) failed; see judgments.json under {} for the \
             recorded attempts",
            judge_errors,
            run_dir_base.display()
        )));
    }

    let should_fail = any_agent_failed;

    if should_fail && !args.no_fail {
        let total_passed: usize = all_summaries.iter().map(|s| s.passed).sum();
        let total_cases: usize = all_summaries.iter().map(|s| s.total_cases).sum();
        return Err(CliError::Config(format!(
            "Eval suite failed: {}/{} cases passed across {} agent(s) (threshold={})",
            total_passed,
            total_cases,
            all_summaries.len(),
            pass_threshold
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A summary as the engine leaves it after judging: one case passed, one
    /// never produced a score, and `suite_pass` is the engine's narrower
    /// question — every *scored* case passed, so it says true.
    fn judged_summary() -> SummaryResult {
        SummaryResult {
            suite_pass: true,
            suite_pass_rate: Some(1.0),
            agent: "aikit".to_string(),
            model: None,
            total_cases: 2,
            passed: 1,
            failed: 1,
            trials_per_case: Some(1),
            parallel: None,
            pass_threshold: Some(0.5),
            run_dir: PathBuf::from("run"),
            checks_path: None,
            skill_project_root: PathBuf::from("."),
            isolation: None,
            judge_errors: Some(0),
            judge_skipped_trials: Some(0),
            judge_tokens: None,
            judge_cost_usd: None,
            skill_git_sha: None,
            skill_dirty: None,
            cases: vec![],
        }
    }

    /// spec eval-judge R13: `--judge` must not turn a red run green. A case
    /// the agent never completed is outside the engine's judged verdict, and
    /// `eval run` has always failed on one.
    #[test]
    fn test_run_verdict_fails_on_an_unscored_case_the_engine_left_out() {
        let summary = judged_summary();
        assert!(
            !run_verdict(&summary, false, 0.5),
            "an unscored case must still fail the run, whatever suite_pass says"
        );
        // Under --ci the rate is over every case, so 1 of 2 is 50%.
        assert!(run_verdict(&summary, true, 0.5));
        assert!(!run_verdict(&summary, true, 0.75));
    }

    #[test]
    fn test_run_verdict_passes_when_every_case_passed() {
        let mut summary = judged_summary();
        summary.passed = 2;
        summary.failed = 0;
        assert!(run_verdict(&summary, false, 0.5));
        assert!(run_verdict(&summary, true, 1.0));
    }
}

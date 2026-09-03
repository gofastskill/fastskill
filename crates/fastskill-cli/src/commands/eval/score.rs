//! Eval score subcommand - offline re-scoring from saved artifacts

use crate::commands::common::validate_eval_format_args;
use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::{aggregate_trials, read_summary, CaseStatus, TrialResult};
use fastskill_evals::checks::{
    effective_checks, load_checks, run_checks_in_context, suite_passes, CheckDefinition,
};
use std::collections::HashMap;
use std::path::PathBuf;

use super::observability::check_context;

/// Arguments for `fastskill eval score`
#[derive(Debug)]
pub struct ScoreArgs {
    /// Path to the run directory to re-score
    pub run_dir: PathBuf,

    /// Output format: table, json (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    pub no_fail: bool,
}

fn parse_output_format(s: &str) -> Option<fastskill_core::OutputFormat> {
    match s {
        "table" => Some(fastskill_core::OutputFormat::Table),
        "json" => Some(fastskill_core::OutputFormat::Json),
        "grid" => Some(fastskill_core::OutputFormat::Grid),
        "xml" => Some(fastskill_core::OutputFormat::Xml),
        _ => None,
    }
}

impl IntoCommandSpec for ScoreArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Re-score saved eval artifacts without running the agent again",
            syntax: Some("eval score [OPTIONS]"),
            category: Some("quality"),
            examples: vec![
                "fastskill eval score --run-dir ./eval-runs/2026-08-14T12-00-00Z/claude",
            ],
            args: vec![
                ArgSpec {
                    name: "run-dir",
                    kind: ArgKind::Option,
                    long: Some("run-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Path to the run directory to re-score",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Shorthand for --format json",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-fail",
                    kind: ArgKind::Flag,
                    long: Some("no-fail"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Do not fail with non-zero exit code on suite failure",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ScoreArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ScoreArgs {
            run_dir: map
                .get("run-dir")
                .map(|v| {
                    if let ArgValue::Str(s) = v {
                        PathBuf::from(s)
                    } else {
                        panic!("fw bug")
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing run-dir")),
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            no_fail: matches!(map.get("no-fail"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Execute the `eval score` command
pub async fn execute_score(args: ScoreArgs) -> CliResult<()> {
    let format = validate_eval_format_args(&args.format, args.json)?;
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

    // A summary with zero cases must not re-score as `0/0 passed · PASSED`
    // (the suite verdict below is `failed == 0`) — same guard as `eval run`'s
    // empty-suite check, applied to the artifact side.
    if summary.cases.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_EMPTY_SUITE: summary.json in '{}' contains zero cases — nothing to re-score",
            args.run_dir.display()
        )));
    }

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
    // Cases whose every trial errored. They have no measurement, so they are
    // neither a pass nor a fail and leave the rate entirely — but they must not
    // leave the report, or a total outage reads as 100% over zero cases.
    let mut errored_cases: Vec<String> = Vec::new();

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

        // The effective check list for this case, rebuilt exactly as the run
        // built it: those scoped to the case, plus the skill-invocation check
        // implied by its `should_trigger` column.
        //
        // A pre-R7 artifact did not record the column. Guessing `false` there
        // would invent an assertion the run never made, so the fallback is the
        // explicit checks alone — which is precisely what those artifacts were
        // scored with when they were written.
        let case_checks: Vec<CheckDefinition> = match case_summary.should_trigger {
            Some(should_trigger) => effective_checks(&checks, &case_summary.id, should_trigger),
            None => checks
                .iter()
                .filter(|c| c.applies_to(&case_summary.id))
                .cloned()
                .collect(),
        };

        let mut trials: Vec<TrialResult> = Vec::with_capacity(trial_dirs.len());
        for (trial_id, tdir) in &trial_dirs {
            let trace_path = tdir.join("trace.jsonl");
            let trace_jsonl = std::fs::read_to_string(&trace_path).unwrap_or_default();

            // Read the recorded trial before scoring: it carries the staged
            // skill path this trial's trace actually references, which is what
            // `skill_invoked` matches on when the backend has no typed `Skill`
            // tool. Per trial, because isolation gives each one its own scratch
            // directory.
            let recorded: Option<TrialResult> = std::fs::read_to_string(tdir.join("result.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            let staged_skill_path = recorded
                .as_ref()
                .and_then(|r| r.skill_path.as_ref())
                .map(|p| p.to_string_lossy().to_string());

            let ctx = check_context(&summary.agent, staged_skill_path.as_deref());
            let check_results = run_checks_in_context(
                &case_checks,
                &trace_jsonl,
                &summary.skill_project_root,
                &ctx,
            );
            // Same predicate the runner used: a check the backend cannot
            // observe is not a failure, and an optional check is not a gate.
            let all_passed = suite_passes(&check_results);

            // A trial that errored or was skipped at run time (timeout, agent crash,
            // missing agent) has at best a partial trace, and checks passing over a
            // partial trace are not a pass. Keep the recorded verdict and message so
            // `score` can never report more passes than the `run` that wrote the
            // artifacts; only passed/failed trials are re-derived from the checks.
            let (status, error_message) = match &recorded {
                Some(r) if matches!(r.status, CaseStatus::Error | CaseStatus::Skipped) => {
                    (r.status.clone(), r.error_message.clone())
                }
                _ => (
                    if all_passed {
                        CaseStatus::Passed
                    } else {
                        CaseStatus::Failed
                    },
                    None,
                ),
            };

            trials.push(TrialResult {
                trial_id: *trial_id,
                status,
                command_count: recorded.as_ref().and_then(|r| r.command_count),
                input_tokens: recorded.as_ref().and_then(|r| r.input_tokens),
                output_tokens: recorded.as_ref().and_then(|r| r.output_tokens),
                check_results,
                error_message,
                // Facts the run recorded about the process, not verdicts:
                // carried through untouched so a re-score never invents a
                // number the run did not measure.
                exit_code: recorded.as_ref().and_then(|r| r.exit_code),
                terminal: recorded.as_ref().and_then(|r| r.terminal.clone()),
                cost_usd: recorded.as_ref().and_then(|r| r.cost_usd),
                tokens: recorded
                    .as_ref()
                    .map(|r| r.tokens.clone())
                    .unwrap_or_default(),
                skill_path: recorded.as_ref().and_then(|r| r.skill_path.clone()),
            });
        }

        let trial_count = trials.len().max(1) as u32;
        let aggregated = aggregate_trials(&case_summary.id, trials, trial_count, pass_threshold);

        case_summary.trials = aggregated.trials;
        case_summary.pass_count = Some(aggregated.pass_count);
        case_summary.total_trials = Some(aggregated.total_trials);
        case_summary.pass_rate = Some(aggregated.pass_rate);
        case_summary.error_count = Some(aggregated.error_count);
        case_summary.scored_trials = Some(aggregated.scored_trials);
        case_summary.status = aggregated.aggregated_status;

        match case_summary.status {
            CaseStatus::Passed => new_passed += 1,
            CaseStatus::Error => errored_cases.push(case_summary.id.clone()),
            _ => new_failed += 1,
        }
    }

    // Cases with no measurement leave both sides of the rate, exactly as
    // errored trials leave the per-case rate one level down. `passed + failed`
    // is therefore below `total_cases` whenever a case errored; the difference
    // is the outage, and it is named in the exit below rather than averaged in.
    let scored_cases = summary.total_cases.saturating_sub(errored_cases.len());
    summary.passed = new_passed;
    summary.failed = new_failed;
    summary.suite_pass_rate = if scored_cases == 0 {
        Some(0.0)
    } else {
        Some(new_passed as f64 / scored_cases as f64)
    };
    summary.suite_pass = new_failed == 0 && errored_cases.is_empty();
    summary.cases = updated_cases;

    // R11: `eval score` is read-only. The writer owns the artifact and the
    // scorer reads it, so re-scoring a committed fixture cannot dirty the
    // working tree. Everything reported below is a pure function of
    // `result.json` and `trace.jsonl`.

    if use_json {
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        crate::outln!("Re-scoring complete");
        crate::outln!(
            "  result: {}",
            if summary.suite_pass {
                "PASSED"
            } else {
                "FAILED"
            }
        );
        crate::outln!("  cases: {}/{} passed", summary.passed, summary.total_cases);
        if !errored_cases.is_empty() {
            crate::outln!(
                "  unscored: {} case(s) produced no measurement",
                errored_cases.len()
            );
        }
    }

    // A case with no measurement is not a low score, it is the absence of one,
    // and `--no-fail` suppresses "the skill scored badly" rather than "there is
    // no score". Silently exiting zero here would let a total outage read as a
    // clean run, which is the exact failure this whole change exists to close.
    if !errored_cases.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_CASES_UNMEASURED: {} of {} case(s) produced no measurement and were \
             excluded from the score: {}. Re-run them; the reported {}/{} covers only \
             the rest.",
            errored_cases.len(),
            summary.total_cases,
            errored_cases.join(", "),
            summary.passed,
            scored_cases
        )));
    }

    if !summary.suite_pass && !args.no_fail {
        return Err(CliError::Config(format!(
            "Eval suite failed: {}/{} cases passed after re-scoring",
            summary.passed, summary.total_cases
        )));
    }

    Ok(())
}

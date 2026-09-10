//! Eval judge subcommand - score a completed run with the judges its checks
//! file declares (spec eval-judge R13).
//!
//! One judging function, two entry points: `fastskill eval judge --run-dir`
//! judges a finished run in place, and `fastskill eval run --judge` calls the
//! same [`judge_run`] after the run's own scoring. Neither has judging logic of
//! its own — that lives in `aikit_evals::judge::judge_run_dir`, so the two
//! entry points cannot drift into scoring the same run differently.

use crate::commands::common::validate_eval_format_args;
use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::read_summary;
use fastskill_evals::judge::{judge_run_dir, JudgeRunOptions, JudgeRunReport, SuitePassRule};
use fastskill_evals::resolve_eval_config;
use fastskill_evals::suite::load_suite;
use fastskill_evals::EvalSuite;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Arguments for `fastskill eval judge`
#[derive(Debug)]
pub struct JudgeArgs {
    /// Path to the run directory to judge
    pub run_dir: PathBuf,

    /// Checks file to read `[[judge]]` from, instead of the one the run recorded
    pub checks: Option<PathBuf>,

    /// Override every judge's model, and record the override
    pub judge_model: Option<String>,

    /// Concurrent judge requests (default: the run's recorded parallel)
    pub judge_parallel: Option<i64>,

    /// Judge again even when the same request already has a judgment
    pub rejudge: bool,

    /// CI mode: the suite verdict is the case pass rate against the threshold
    pub ci: bool,

    /// Output format: table, json (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    pub no_fail: bool,
}

fn parse_output_format(s: &str) -> Option<OutputFormat> {
    match s {
        "table" => Some(OutputFormat::Table),
        "json" => Some(OutputFormat::Json),
        "grid" => Some(OutputFormat::Grid),
        "xml" => Some(OutputFormat::Xml),
        _ => None,
    }
}

impl IntoCommandSpec for JudgeArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Judge a completed eval run with the judges its checks file declares",
            syntax: Some("eval judge [OPTIONS]"),
            category: Some("quality"),
            examples: vec![
                "fastskill eval judge --run-dir ./eval-runs/2026-09-04T12-00-00Z/claude",
                "fastskill eval judge --run-dir ./eval-runs/latest/claude --judge-model gpt-4.1",
            ],
            args: vec![
                ArgSpec {
                    name: "run-dir",
                    kind: ArgKind::Option,
                    long: Some("run-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Path to the run directory to judge",
                    ..Default::default()
                },
                ArgSpec {
                    name: "checks",
                    kind: ArgKind::Option,
                    long: Some("checks"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Checks file to read judges from, instead of the one the run recorded",
                    ..Default::default()
                },
                ArgSpec {
                    name: "judge-model",
                    kind: ArgKind::Option,
                    long: Some("judge-model"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Override every judge's model; recorded in each judgment",
                    ..Default::default()
                },
                ArgSpec {
                    name: "judge-parallel",
                    kind: ArgKind::Option,
                    long: Some("judge-parallel"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Concurrent judge requests (default: the run's parallel)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "rejudge",
                    kind: ArgKind::Flag,
                    long: Some("rejudge"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Judge again even when the same request already has a judgment",
                    ..Default::default()
                },
                ArgSpec {
                    name: "ci",
                    kind: ArgKind::Flag,
                    long: Some("ci"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "CI mode: pass/fail on suite pass rate vs threshold",
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

impl FromArgValueMap for JudgeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        let string = |key: &str| -> Option<String> {
            map.get(key).and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
        };
        JudgeArgs {
            run_dir: string("run-dir")
                .map(PathBuf::from)
                .unwrap_or_else(|| panic!("fw bug: missing run-dir")),
            checks: string("checks").map(PathBuf::from),
            judge_model: string("judge-model"),
            // Carried raw: the range check below echoes what the user typed.
            judge_parallel: map.get("judge-parallel").and_then(|v| {
                if let ArgValue::Int(i) = v {
                    Some(*i)
                } else {
                    None
                }
            }),
            rejudge: matches!(map.get("rejudge"), Some(ArgValue::Bool(true))),
            ci: matches!(map.get("ci"), Some(ArgValue::Bool(true))),
            format: string("format").as_deref().and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            no_fail: matches!(map.get("no-fail"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Judge one run directory. The single call site of the engine's judging
/// function: `eval judge` and `eval run --judge` both come through here, so
/// there is one place where a judge run is configured (R13).
pub(super) async fn judge_run(
    run_dir: &Path,
    suite: &EvalSuite,
    opts: &JudgeRunOptions,
) -> CliResult<JudgeRunReport> {
    judge_run_dir(run_dir, suite, opts)
        .await
        .map_err(|e| CliError::Config(e.to_string()))
}

/// The suite the run was made from. Judging renders `{{case.<column>}}` out of
/// `prompts.csv`, so it must be the same file the run read — resolved from the
/// skill project the run recorded, never from the current directory.
fn suite_for_run(skill_project_root: &Path) -> CliResult<EvalSuite> {
    let resolution = resolve_project_file(skill_project_root);
    if !resolution.found {
        return Err(CliError::Config(format!(
            "EVAL_CONFIG_MISSING: no skill-project.toml under '{}', the skill project this run \
             recorded; judging needs the prompts.csv the run was made from",
            skill_project_root.display()
        )));
    }
    let project_root = resolution
        .path
        .parent()
        .unwrap_or(skill_project_root)
        .to_path_buf();
    let eval_config = resolve_eval_config(&resolution.path, &project_root)
        .map_err(|e| CliError::Config(e.to_string()))?;
    load_suite(&eval_config.prompts_path).map_err(|e| CliError::Config(e.to_string()))
}

/// Render a judge report as lines. Shared with `eval run --judge` so the two
/// entry points report the same run the same way.
pub(super) fn render_report(report: &JudgeRunReport) {
    crate::outln!("  judges: {}", report.judges.join(", "));
    crate::outln!(
        "  judged: {} · cached: {} · skipped (errored trials): {} · judge errors: {}",
        report.judged,
        report.skipped_cached,
        report.skipped_error_trials,
        report.errors
    );
    crate::outln!(
        "  judge tokens: {} in / {} out / {} total",
        report.tokens.input,
        report.tokens.output,
        report.tokens.total
    );
}

/// Execute the `eval judge` command
pub async fn execute_judge(args: JudgeArgs) -> CliResult<()> {
    let format = validate_eval_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Run directory does not exist: {}",
            args.run_dir.display()
        )));
    }

    let parallel = match args.judge_parallel {
        None => None,
        Some(n) if (1..=1000).contains(&n) => Some(n as u32),
        Some(n) => {
            return Err(CliError::Config(format!(
                "EVAL_JUDGE_PARALLEL_INVALID: --judge-parallel must be between 1 and 1000, got {}",
                n
            )));
        }
    };

    if let Some(checks) = &args.checks {
        if !checks.exists() {
            return Err(CliError::Config(format!(
                "EVAL_CONFIG_MISSING: checks file '{}' does not exist",
                checks.display()
            )));
        }
    }

    let summary = read_summary(&args.run_dir).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Failed to read summary.json: {}",
            e
        ))
    })?;
    if summary.cases.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_EMPTY_SUITE: summary.json in '{}' contains zero cases — nothing to judge",
            args.run_dir.display()
        )));
    }

    let suite = suite_for_run(&summary.skill_project_root)?;

    let opts = JudgeRunOptions {
        checks_override: args.checks.clone(),
        judge_model: args.judge_model.clone(),
        parallel,
        rejudge: args.rejudge,
        // The suite rule `eval run` uses, asked the same way: without `--ci`
        // every scored case must pass, with it the rate must reach the run's
        // recorded threshold.
        suite_rule: if args.ci {
            SuitePassRule::RateAtLeast(summary.pass_threshold.unwrap_or(1.0))
        } else {
            SuitePassRule::AllCases
        },
        ..Default::default()
    };

    let report = judge_run(&args.run_dir, &suite, &opts).await?;

    if use_json {
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else if report.judges.is_empty() {
        crate::outln!("No judges declared; nothing was judged.");
    } else {
        crate::outln!("Judging complete");
        crate::outln!("  run_dir: {}", args.run_dir.display());
        render_report(&report);
        crate::outln!(
            "  result: {}",
            if report.suite_pass {
                "PASSED"
            } else {
                "FAILED"
            }
        );
    }

    // R13: non-zero on any judge error or a failed rewritten verdict, and
    // `--no-fail` suppresses the second reason only. A judge that could not
    // render a judgment left a gap in the measurement; suppressing that would
    // report an outage as a score.
    if report.errors > 0 {
        return Err(CliError::Config(format!(
            "EVAL_JUDGE_ERRORS: {} judgment(s) failed; see judgments.json under {} for the \
             recorded attempts",
            report.errors,
            args.run_dir.display()
        )));
    }
    if !report.suite_pass && !args.no_fail {
        return Err(CliError::Config(
            "Eval suite failed after judging".to_string(),
        ));
    }

    Ok(())
}

//! Eval scorecard subcommand — fold many runs into named, gated metrics.
//!
//! `eval report` describes one run and `eval score` decides one run's verdict.
//! Neither answers the question a benchmark is run to answer: across every run,
//! what is the rate of each individual assertion, what did the judges score,
//! and does each clear the bar set for it?
//!
//! Those are different numbers from a suite pass rate. A consultation case
//! carries both a skill-invocation check and a tool-call ceiling; folding them
//! into one per-case verdict reports a budget overrun as a recall failure. A
//! scorecard keeps each measurement's rate separate and gates each one on its
//! own threshold.
//!
//! The command emits a `fastskill.scorecard/1` document (see [`document`]).
//! That document is what survives the run directories: it names what was
//! measured — target, skill, benchmark, judges — beside the numbers, because a
//! number whose provenance is gone cannot be compared with next month's.
//!
//! Layout:
//! - [`metrics`]: the metrics file, the case patterns and the benchmark hash;
//! - [`observations`]: folding run artifacts into what a metric reads;
//! - [`document`]: the emitted document's types.

pub mod document;
pub mod metrics;
pub mod observations;

#[cfg(test)]
mod fixtures;

use crate::commands::common::validate_eval_format_args;
use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use document::{
    started_at_from_path, BenchmarkIdentity, CaseRow, RunEntry, Scorecard, SkillIdentity,
    TargetEntry, AIKIT_EVALS_VERSION, SCORECARD_SCHEMA,
};
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::{read_summary, SummaryResult};
use metrics::{benchmark_sha256, load_metrics};
use observations::{absorb, evaluate, unclaimed, Observations};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Arguments for `fastskill eval scorecard`
#[derive(Debug)]
pub struct ScorecardArgs {
    /// Directory searched recursively for run directories
    pub root: PathBuf,

    /// TOML file declaring the metrics and their thresholds
    pub metrics: PathBuf,

    /// Output format: table, json (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,

    /// Report the numbers without failing on a metric below its threshold
    pub no_fail: bool,

    /// Fold judgments from more than one judge identity into one metric
    pub allow_mixed_judges: bool,

    /// Fold runs against more than one (agent, model) pair into one scorecard
    pub allow_mixed_targets: bool,

    /// Fold runs that measure the same case id more than once
    pub allow_duplicate_cases: bool,
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

impl IntoCommandSpec for ScorecardArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Fold many eval runs into named, gated metrics",
            syntax: Some("eval scorecard --root <DIR> --metrics <FILE> [OPTIONS]"),
            category: Some("quality"),
            examples: vec![
                "fastskill eval scorecard --root ./eval-runs --metrics ./evals/metrics.toml",
            ],
            args: vec![
                ArgSpec {
                    name: "root",
                    kind: ArgKind::Option,
                    long: Some("root"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Directory searched recursively for run directories",
                    ..Default::default()
                },
                ArgSpec {
                    name: "metrics",
                    kind: ArgKind::Option,
                    long: Some("metrics"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "TOML file declaring the metrics and their thresholds",
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
                    help: "Report the numbers without failing on a metric below its threshold",
                    ..Default::default()
                },
                ArgSpec {
                    name: "allow-mixed-judges",
                    kind: ArgKind::Flag,
                    long: Some("allow-mixed-judges"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Fold judgments from more than one judge identity into one metric",
                    ..Default::default()
                },
                ArgSpec {
                    name: "allow-mixed-targets",
                    kind: ArgKind::Flag,
                    long: Some("allow-mixed-targets"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Fold runs against more than one (agent, model) pair into one scorecard",
                    ..Default::default()
                },
                ArgSpec {
                    name: "allow-duplicate-cases",
                    kind: ArgKind::Flag,
                    long: Some("allow-duplicate-cases"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Fold runs that measure the same case id more than once",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

fn required_path(map: &HashMap<String, ArgValue>, key: &str) -> PathBuf {
    match map.get(key) {
        Some(ArgValue::Str(s)) => PathBuf::from(s),
        _ => panic!("fw bug: missing {key}"),
    }
}

fn flag(map: &HashMap<String, ArgValue>, key: &str) -> bool {
    matches!(map.get(key), Some(ArgValue::Bool(true)))
}

impl FromArgValueMap for ScorecardArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ScorecardArgs {
            root: required_path(map, "root"),
            metrics: required_path(map, "metrics"),
            format: map
                .get("format")
                .and_then(|v| match v {
                    ArgValue::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .and_then(parse_output_format),
            json: flag(map, "json"),
            no_fail: flag(map, "no-fail"),
            allow_mixed_judges: flag(map, "allow-mixed-judges"),
            allow_mixed_targets: flag(map, "allow-mixed-targets"),
            allow_duplicate_cases: flag(map, "allow-duplicate-cases"),
        }
    }
}

/// Every directory under `root` holding a `summary.json`, in a stable order.
fn run_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.file_name() == "summary.json")
        .filter_map(|e| e.path().parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// The `(agent, model)` pairs the runs were measured against, in first-seen
/// order, each with the number of runs that used it (R2).
fn distinct_targets(runs: &[RunEntry]) -> Vec<TargetEntry> {
    let mut out: Vec<TargetEntry> = Vec::new();
    for run in runs {
        match out
            .iter_mut()
            .find(|t| t.agent == run.agent && t.model == run.model)
        {
            Some(existing) => existing.runs += 1,
            None => out.push(TargetEntry {
                agent: run.agent.clone(),
                model: run.model.clone(),
                runs: 1,
            }),
        }
    }
    out
}

/// Case ids measured by more than one run under the root (R4).
///
/// Not a bug in itself — re-running one case to tighten its interval is a
/// legitimate thing to do — but the totals then count that case twice, and a
/// reader who does not know it happened reads a weighted average as a plain
/// one.
fn duplicate_case_ids(cases: &[CaseRow]) -> Vec<String> {
    let mut runs_per_case: BTreeMap<&str, Vec<&Path>> = BTreeMap::new();
    for row in cases {
        let dirs = runs_per_case.entry(&row.case_id).or_default();
        if !dirs.contains(&row.run_dir.as_path()) {
            dirs.push(&row.run_dir);
        }
    }
    runs_per_case
        .into_iter()
        .filter(|(_, dirs)| dirs.len() > 1)
        .map(|(id, dirs)| format!("{} ({} runs)", id, dirs.len()))
        .collect()
}

/// The skill the numbers are about (R2).
///
/// The revision is copied from the artifacts and only when every run agrees:
/// two runs against different skill revisions do not have one revision, and
/// naming either would attribute half the numbers to code that did not produce
/// them. Never recomputed from the working tree — the skill on disk now is not
/// the skill that ran.
fn skill_identity(summaries: &[SummaryResult]) -> SkillIdentity {
    let Some(first) = summaries.first() else {
        return SkillIdentity::default();
    };
    let agreed = |get: &dyn Fn(&SummaryResult) -> Option<String>| -> Option<String> {
        let value = get(first)?;
        summaries
            .iter()
            .all(|s| get(s).as_deref() == Some(value.as_str()))
            .then_some(value)
    };
    SkillIdentity {
        path: Some(first.skill_project_root.clone()),
        git_sha: agreed(&|s| s.skill_git_sha.clone()),
        dirty: agreed(&|s| s.skill_dirty.map(|d| d.to_string()))
            .and_then(|d| d.parse::<bool>().ok()),
    }
}

fn render_table(card: &Scorecard) {
    crate::outln!("Eval Scorecard");
    let target = match card.targets.len() {
        1 => match (&card.agent, &card.model) {
            (Some(agent), Some(model)) => format!("{} / {}", agent, model),
            (Some(agent), None) => agent.clone(),
            _ => "unknown".to_string(),
        },
        n => format!("{} targets", n),
    };
    crate::outln!(
        "  target: {}   skill: {}   benchmark: {}",
        target,
        card.skill
            .git_sha
            .as_deref()
            .map(|sha| format!("{}{}", &sha[..sha.len().min(8)], {
                if card.skill.dirty == Some(true) {
                    "-dirty"
                } else {
                    ""
                }
            }))
            .unwrap_or_else(|| "unrecorded".to_string()),
        card.benchmark
            .sha256
            .as_deref()
            .map(|sha| sha[..sha.len().min(12)].to_string())
            .unwrap_or_else(|| "no suites declared".to_string())
    );
    crate::outln!(
        "  {} run(s), {} case(s), {} trial(s) — {} scored, {} errored",
        card.totals.runs,
        card.totals.cases,
        card.totals.trials,
        card.totals.scored_trials,
        card.totals.error_trials
    );
    crate::outln!("");
    for metric in &card.metrics {
        let value = match (metric.rate, metric.p95_tool_calls, metric.score) {
            (Some(rate), _, _) => format!("{:.1}%", rate * 100.0),
            (_, Some(p95), _) => p95.to_string(),
            (_, _, Some(score)) => format!("{:.2}", score),
            _ => "-".to_string(),
        };
        crate::outln!(
            "  {:<28} {:>7}   ({}/{} observed, {} case(s))   {} -> {}{}",
            metric.name,
            value,
            metric.passed,
            metric.observed,
            metric.cases,
            metric.threshold,
            metric.verdict,
            match (metric.mixed_judges, metric.mixed_targets) {
                (true, true) => "  [mixed judges, mixed targets]",
                (true, false) => "  [mixed judges]",
                (false, true) => "  [mixed targets]",
                (false, false) => "",
            }
        );
    }
    crate::outln!("");
    match card.totals.cost_usd {
        Some(usd) => crate::outln!(
            "  cost: ${:.2} vendor-reported over {} trial(s) ({} reported none)",
            usd,
            card.totals.scored_trials - card.totals.trials_without_cost,
            card.totals.trials_without_cost
        ),
        None => crate::outln!("  cost: not reported by this backend"),
    }
    if !card.judges.is_empty() {
        for judge in &card.judges {
            crate::outln!(
                "  judge: {} ({}) — {} @ {}",
                judge.name,
                &judge.judge_hash[..judge.judge_hash.len().min(12)],
                judge.identity.model,
                judge.identity.endpoint_host
            );
        }
        crate::outln!(
            "  judge tokens: {} in / {} out ({} total), {} error(s), {} excluded trial(s)",
            card.totals.judge_tokens.input,
            card.totals.judge_tokens.output,
            card.totals.judge_tokens.total,
            card.totals.judge_errors,
            card.totals.judge_excluded_trials
        );
    }
    if card.totals.not_observable_checks > 0 {
        crate::outln!(
            "  not observable: {} check result(s) excluded — the backend cannot produce the evidence",
            card.totals.not_observable_checks
        );
    }
    if !card.totals.unmeasured_cases.is_empty() {
        crate::outln!(
            "  unmeasured: {} case(s) whose every trial errored: {}",
            card.totals.unmeasured_cases.len(),
            card.totals.unmeasured_cases.join(", ")
        );
    }
    if !card.unclaimed_checks.is_empty() {
        crate::outln!(
            "  WARNING: check result(s) no metric reports: {}",
            card.unclaimed_checks.join(", ")
        );
    }
}

/// Execute the `eval scorecard` command
pub async fn execute_scorecard(args: ScorecardArgs) -> CliResult<()> {
    let format = validate_eval_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    let metrics_file = load_metrics(&args.metrics)?;
    let specs = &metrics_file.metrics;

    if !args.root.exists() {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_NO_RUNS: root directory does not exist: {}",
            args.root.display()
        )));
    }
    let dirs = run_dirs(&args.root);
    if dirs.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_NO_RUNS: no summary.json found under '{}'",
            args.root.display()
        )));
    }

    let mut obs = Observations::default();
    let mut summaries = Vec::new();
    let mut runs = Vec::new();
    for dir in &dirs {
        let summary = read_summary(dir).map_err(|e| {
            CliError::Config(format!(
                "EVAL_ARTIFACTS_CORRUPT: failed to read summary.json in '{}': {}",
                dir.display(),
                e
            ))
        })?;
        runs.push(RunEntry {
            run_dir: dir.clone(),
            started_at: started_at_from_path(dir),
            agent: summary.agent.clone(),
            model: summary.model.clone(),
        });
        absorb(&mut obs, &summary, dir)?;
        summaries.push(summary);
    }

    let targets = distinct_targets(&runs);
    let mixed_targets = targets.len() > 1;
    let duplicates = duplicate_case_ids(&obs.cases);

    let mut reports = Vec::new();
    let mut empty = Vec::new();
    for spec in specs {
        match evaluate(spec, &obs, mixed_targets) {
            Some(report) => reports.push(report),
            None => empty.push(spec.name.clone()),
        }
    }
    let mixed_judges: Vec<String> = reports
        .iter()
        .filter(|m| m.mixed_judges)
        .map(|m| m.name.clone())
        .collect();

    let single = (!mixed_targets).then(|| targets.first()).flatten();
    let card = Scorecard {
        schema: SCORECARD_SCHEMA,
        generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        agent: single.map(|t| t.agent.clone()),
        model: single.and_then(|t| t.model.clone()),
        targets,
        skill: skill_identity(&summaries),
        benchmark: BenchmarkIdentity {
            path: args.metrics.clone(),
            sha256: benchmark_sha256(&args.metrics, &metrics_file.suites)?,
        },
        runs,
        fastskill_version: env!("CARGO_PKG_VERSION"),
        aikit_evals_version: AIKIT_EVALS_VERSION,
        judges: obs.judges.values().cloned().collect(),
        unclaimed_checks: unclaimed(specs, &obs),
        metrics: reports,
        totals: std::mem::take(&mut obs.totals),
        cases: std::mem::take(&mut obs.cases),
    };

    if use_json {
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&card).unwrap_or_default()
        );
    } else {
        render_table(&card);
    }

    // The three mixed-measurement guards (R4). Each names the offending values
    // and each has an override, because the fix is sometimes "yes, I meant to"
    // — but the default is to refuse, because a mean over two different
    // measurements is not a measurement of either. `--no-fail` says "report the
    // numbers rather than gate on them"; it does not say "the numbers mean
    // something they do not", so it suppresses none of these.
    if mixed_targets && !args.allow_mixed_targets {
        let named: Vec<String> = card
            .targets
            .iter()
            .map(|t| match &t.model {
                Some(model) => format!("{}/{} ({} run(s))", t.agent, model, t.runs),
                None => format!("{} ({} run(s))", t.agent, t.runs),
            })
            .collect();
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_MIXED_TARGETS: runs under '{}' measured {} different targets: {}. \
             Point --root at one target, or pass --allow-mixed-targets to fold them.",
            args.root.display(),
            card.targets.len(),
            named.join(", ")
        )));
    }
    if !duplicates.is_empty() && !args.allow_duplicate_cases {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_DUPLICATE_CASES: {} case id(s) are measured by more than one run: {}. \
             Pass --allow-duplicate-cases to count every occurrence.",
            duplicates.len(),
            duplicates.join(", ")
        )));
    }
    if !mixed_judges.is_empty() && !args.allow_mixed_judges {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_MIXED_JUDGES: {} metric(s) fold judgments from more than one judge \
             identity: {}. Re-judge with one identity, or pass --allow-mixed-judges.",
            mixed_judges.len(),
            mixed_judges.join(", ")
        )));
    }

    // A metric that matched nothing is a broken scorecard, not a passing one,
    // and `--no-fail` does not suppress it: the flag says "report the numbers
    // rather than gate on them", and there are no numbers to report here.
    if !empty.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_EMPTY_METRIC: {} metric(s) matched no observed result: {}. \
             Check the case patterns, check names and judge names against the artifacts.",
            empty.len(),
            empty.join(", ")
        )));
    }

    let below: Vec<&str> = card
        .metrics
        .iter()
        .filter(|m| m.verdict != "PASS")
        .map(|m| m.name.as_str())
        .collect();
    if !below.is_empty() && !args.no_fail {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_BELOW_THRESHOLD: {} metric(s) did not clear their threshold: {}",
            below.len(),
            below.join(", ")
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use fastskill_evals::artifacts::CaseStatus;

    fn run_entry(dir: &str, agent: &str, model: Option<&str>) -> RunEntry {
        RunEntry {
            run_dir: PathBuf::from(dir),
            started_at: None,
            agent: agent.to_string(),
            model: model.map(str::to_string),
        }
    }

    #[test]
    fn run_dirs_finds_every_summary_under_the_root() {
        let root = tempfile::tempdir().expect("tempdir");
        for leaf in ["a/claude", "b/codex"] {
            let dir = root.path().join(leaf);
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(dir.join("summary.json"), "{}").expect("write");
        }
        std::fs::write(root.path().join("notes.txt"), "x").expect("write");
        let found = run_dirs(root.path());
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.join("summary.json").exists()));
    }

    /// A mean over two agents is a measurement of neither, so the default is to
    /// refuse rather than to average.
    #[test]
    fn two_agents_under_one_root_are_two_targets() {
        let runs = vec![
            run_entry("/r/1", "claude", Some("opus")),
            run_entry("/r/2", "claude", Some("opus")),
            run_entry("/r/3", "codex", Some("gpt-5")),
        ];
        let targets = distinct_targets(&runs);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].runs, 2, "runs against one target are counted");
        assert_eq!(targets[1].agent, "codex");
    }

    /// Same agent, different model, is a different target: the model is what
    /// the numbers are about.
    #[test]
    fn one_agent_on_two_models_is_two_targets() {
        let runs = vec![
            run_entry("/r/1", "claude", Some("opus")),
            run_entry("/r/2", "claude", Some("sonnet")),
        ];
        assert_eq!(distinct_targets(&runs).len(), 2);
    }

    #[test]
    fn a_case_measured_by_two_runs_is_named() {
        let mut rows = Vec::new();
        for dir in ["/r/1", "/r/2"] {
            let mut obs = Observations::default();
            absorb(
                &mut obs,
                &summary(vec![
                    case(
                        "op-init",
                        vec![trial(
                            1,
                            CaseStatus::Passed,
                            vec![check("skill_invoked", true)],
                        )],
                    ),
                    case(
                        "op-only-once",
                        vec![trial(
                            1,
                            CaseStatus::Passed,
                            vec![check("skill_invoked", true)],
                        )],
                    ),
                ]),
                Path::new(dir),
            )
            .expect("no judgments");
            rows.extend(obs.cases);
        }
        // Both cases appear twice in `rows`, but `op-only-once` is filtered
        // below only if it also appears in two *runs* — it does, so this
        // fixture must name both. Trim it to one run to prove the filter is on
        // run count, not row count.
        rows.retain(|r| r.case_id != "op-only-once" || r.run_dir == Path::new("/r/1"));
        assert_eq!(
            duplicate_case_ids(&rows),
            vec!["op-init (2 runs)".to_string()]
        );
    }

    /// R2: two runs against different skill revisions do not have one revision.
    #[test]
    fn a_skill_revision_is_reported_only_when_every_run_agrees() {
        let mut one = summary(vec![]);
        one.skill_git_sha = Some("abc1234".to_string());
        one.skill_dirty = Some(false);
        let mut two = summary(vec![]);
        two.skill_git_sha = Some("abc1234".to_string());
        two.skill_dirty = Some(false);
        assert_eq!(
            skill_identity(&[one, two]).git_sha.as_deref(),
            Some("abc1234")
        );

        let mut a = summary(vec![]);
        a.skill_git_sha = Some("abc1234".to_string());
        let mut b = summary(vec![]);
        b.skill_git_sha = Some("def5678".to_string());
        assert_eq!(skill_identity(&[a, b]).git_sha, None);
    }

    /// A run predating the fields yields null rather than a value invented now.
    #[test]
    fn a_run_that_recorded_no_skill_revision_reports_none() {
        let identity = skill_identity(&[summary(vec![])]);
        assert_eq!(identity.git_sha, None);
        assert_eq!(identity.dirty, None);
        assert!(identity.path.is_some());
    }
}

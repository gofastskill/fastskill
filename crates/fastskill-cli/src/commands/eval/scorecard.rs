//! Eval scorecard subcommand — fold many runs into named, gated metrics.
//!
//! `eval report` describes one run and `eval score` decides one run's verdict.
//! Neither answers the question a benchmark sweep is run to answer: across every
//! run, what is the rate of each individual assertion, and does it clear the bar
//! set for it?
//!
//! Those are different numbers from a suite pass rate. A consultation case
//! carries both a skill-invocation check and a tool-call ceiling; folding them
//! into one per-case verdict reports a budget overrun as a recall failure. A
//! scorecard keeps each check type's rate separate and gates each one on its own
//! threshold.
//!
//! ## Why the folding cannot be left to a post-processor
//!
//! A trial with outcome `error` still carries check results. The checks ran, over
//! an empty or truncated trace, and every negative expectation and every tool-call
//! ceiling passed vacuously. Any reader that folds `check_results` without first
//! dropping errored trials reports an outage as perfect restraint. The same holds
//! for a check the backend could not observe, which records `passed: false` for
//! older readers and must not be counted as a failure by anything that
//! understands the field.
//!
//! Both facts are recorded in the artifact. Honouring them is not optional, so it
//! belongs in the tool that ships with the engine rather than in a script beside
//! the cases.

use crate::commands::common::validate_eval_format_args;
use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::{read_summary, CaseStatus, SummaryResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
}

// ── the metrics file ─────────────────────────────────────────────────────────

/// What a metric measures, and the bar it has to clear.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum MetricKind {
    /// Passing check results over observed check results, for the named check
    /// types. The rate is over trials, not cases: a case passing three of five
    /// trials contributes 3/5, which is the whole reason for running more than
    /// one trial.
    CheckRate { checks: Vec<String>, min_rate: f64 },
    /// The 95th percentile of tool calls per trial. A ceiling, not a floor.
    ToolCallsP95 { max: usize },
}

#[derive(Debug, Deserialize)]
struct MetricSpec {
    name: String,
    /// Case-id patterns, `*` matching any run of characters. An empty list
    /// means every case, which is the only way to say "all" — omitting the
    /// field entirely does the same.
    #[serde(default)]
    cases: Vec<String>,
    #[serde(flatten)]
    kind: MetricKind,
}

#[derive(Debug, Deserialize)]
struct ScorecardToml {
    #[serde(rename = "metric", default)]
    metrics: Vec<MetricSpec>,
}

/// Match a case id against a `*`-wildcard pattern.
///
/// Greedy two-pointer scan with backtracking to the last `*`, which is the
/// standard linear-space algorithm for this grammar. There is no `?` and no
/// character class: case ids are slugs, and a richer grammar would be a
/// dependency and a surprise rather than a feature.
fn matches_pattern(pattern: &str, id: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (pattern.chars().collect(), id.chars().collect());
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut resume) = (None, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            resume = si;
            pi += 1;
        } else if let Some(star_at) = star {
            pi = star_at + 1;
            resume += 1;
            si = resume;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

impl MetricSpec {
    fn covers(&self, case_id: &str) -> bool {
        self.cases.is_empty() || self.cases.iter().any(|p| matches_pattern(p, case_id))
    }
}

// ── what a sweep adds up to ──────────────────────────────────────────────────

/// One metric's result. `observed` is the denominator that was actually
/// measured, never the number of trials attempted.
#[derive(Debug, Serialize)]
struct MetricReport {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    p95_tool_calls: Option<usize>,
    passed: usize,
    observed: usize,
    cases: usize,
    threshold: String,
    verdict: &'static str,
}

/// Everything the sweep recorded that is reported but not gated.
#[derive(Debug, Serialize, Default)]
struct SweepTotals {
    runs: usize,
    cases: usize,
    trials: usize,
    scored_trials: usize,
    error_trials: usize,
    not_observable_checks: usize,
    /// Vendor-reported only. `None` when no trial reported a cost, which is a
    /// different statement from `Some(0.0)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_usd: Option<f64>,
    trials_without_cost: usize,
    /// Cases every one of whose trials errored, so they carry no measurement.
    unmeasured_cases: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Scorecard {
    metrics: Vec<MetricReport>,
    totals: SweepTotals,
    /// Check results no metric claims. A check that runs on every trial and is
    /// reported nowhere is an assertion nobody reads.
    unclaimed_checks: Vec<String>,
}

/// One observed check result, flattened out of the run tree.
struct Observation {
    case_id: String,
    check_name: String,
    passed: bool,
}

/// One scored trial's tool-call count.
struct ToolCount {
    case_id: String,
    calls: usize,
}

#[derive(Default)]
struct Sweep {
    observations: Vec<Observation>,
    tool_counts: Vec<ToolCount>,
    totals: SweepTotals,
}

/// Fold one run's summary into the sweep.
///
/// Errored trials are dropped before their check results are read. They carry
/// results, and those results are vacuous — see the module docs.
fn absorb(sweep: &mut Sweep, summary: &SummaryResult) {
    sweep.totals.runs += 1;
    for case in &summary.cases {
        sweep.totals.cases += 1;
        let mut scored_here = 0usize;
        for trial in &case.trials {
            sweep.totals.trials += 1;
            if trial.status == CaseStatus::Error {
                sweep.totals.error_trials += 1;
                continue;
            }
            scored_here += 1;
            sweep.totals.scored_trials += 1;
            match trial.cost_usd {
                Some(usd) => {
                    *sweep.totals.cost_usd.get_or_insert(0.0) += usd;
                }
                None => sweep.totals.trials_without_cost += 1,
            }
            if let Some(calls) = trial.command_count {
                sweep.tool_counts.push(ToolCount {
                    case_id: case.id.clone(),
                    calls,
                });
            }
            for result in &trial.check_results {
                if result.not_observable.is_some() {
                    sweep.totals.not_observable_checks += 1;
                    continue;
                }
                sweep.observations.push(Observation {
                    case_id: case.id.clone(),
                    check_name: result.check_name.clone(),
                    passed: result.passed,
                });
            }
        }
        if scored_here == 0 && !case.trials.is_empty() {
            sweep.totals.unmeasured_cases.push(case.id.clone());
        }
    }
}

fn percentile95(mut values: Vec<usize>) -> Option<usize> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let last = values.len() - 1;
    let index = ((0.95 * last as f64).round() as usize).min(last);
    Some(values[index])
}

/// Evaluate one metric over the sweep. `None` means the metric matched nothing,
/// which is a failure rather than a line to omit: a mistyped case pattern would
/// otherwise make a gate quietly disappear.
fn evaluate(spec: &MetricSpec, sweep: &Sweep) -> Option<MetricReport> {
    match &spec.kind {
        MetricKind::CheckRate { checks, min_rate } => {
            let wanted: BTreeSet<&str> = checks.iter().map(String::as_str).collect();
            let hits: Vec<&Observation> = sweep
                .observations
                .iter()
                .filter(|o| spec.covers(&o.case_id) && wanted.contains(o.check_name.as_str()))
                .collect();
            if hits.is_empty() {
                return None;
            }
            let passed = hits.iter().filter(|o| o.passed).count();
            let observed = hits.len();
            let rate = passed as f64 / observed as f64;
            let cases: BTreeSet<&str> = hits.iter().map(|o| o.case_id.as_str()).collect();
            Some(MetricReport {
                name: spec.name.clone(),
                rate: Some(rate),
                p95_tool_calls: None,
                passed,
                observed,
                cases: cases.len(),
                threshold: format!(">= {:.0}%", min_rate * 100.0),
                verdict: if rate >= *min_rate {
                    "PASS"
                } else {
                    "BELOW THRESHOLD"
                },
            })
        }
        MetricKind::ToolCallsP95 { max } => {
            let hits: Vec<&ToolCount> = sweep
                .tool_counts
                .iter()
                .filter(|t| spec.covers(&t.case_id))
                .collect();
            let value = percentile95(hits.iter().map(|t| t.calls).collect())?;
            let cases: BTreeSet<&str> = hits.iter().map(|t| t.case_id.as_str()).collect();
            Some(MetricReport {
                name: spec.name.clone(),
                rate: None,
                p95_tool_calls: Some(value),
                passed: hits.iter().filter(|t| t.calls <= *max).count(),
                observed: hits.len(),
                cases: cases.len(),
                threshold: format!("<= {}", max),
                verdict: if value <= *max {
                    "PASS"
                } else {
                    "OVER CEILING"
                },
            })
        }
    }
}

/// Check results that no `check_rate` metric both covers by case and names by
/// type. Reported as `case-id/check-name` groups, deduplicated by check name.
fn unclaimed(specs: &[MetricSpec], sweep: &Sweep) -> Vec<String> {
    let mut by_name: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for observation in &sweep.observations {
        let claimed = specs.iter().any(|spec| match &spec.kind {
            MetricKind::CheckRate { checks, .. } => {
                spec.covers(&observation.case_id)
                    && checks.iter().any(|c| c == &observation.check_name)
            }
            MetricKind::ToolCallsP95 { .. } => false,
        });
        if !claimed {
            by_name
                .entry(&observation.check_name)
                .or_default()
                .insert(&observation.case_id);
        }
    }
    by_name
        .into_iter()
        .map(|(name, cases)| format!("{} ({} case(s))", name, cases.len()))
        .collect()
}

// ── the command ──────────────────────────────────────────────────────────────

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
                "fastskill eval scorecard --root ./sweep --metrics ./evals/metrics.toml",
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
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            no_fail: matches!(map.get("no-fail"), Some(ArgValue::Bool(true))),
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

fn load_metrics(path: &Path) -> CliResult<Vec<MetricSpec>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CliError::Config(format!(
            "EVAL_SCORECARD_CONFIG: cannot read metrics file '{}': {}",
            path.display(),
            e
        ))
    })?;
    let parsed: ScorecardToml = toml::from_str(&text).map_err(|e| {
        CliError::Config(format!(
            "EVAL_SCORECARD_CONFIG: cannot parse metrics file '{}': {}",
            path.display(),
            e
        ))
    })?;
    if parsed.metrics.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_CONFIG: metrics file '{}' declares no [[metric]] entries",
            path.display()
        )));
    }
    Ok(parsed.metrics)
}

fn render_table(card: &Scorecard) {
    crate::outln!("Eval Scorecard");
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
        let value = match (metric.rate, metric.p95_tool_calls) {
            (Some(rate), _) => format!("{:.1}%", rate * 100.0),
            (_, Some(p95)) => p95.to_string(),
            _ => "-".to_string(),
        };
        crate::outln!(
            "  {:<28} {:>7}   ({}/{} observed, {} case(s))   {} -> {}",
            metric.name,
            value,
            metric.passed,
            metric.observed,
            metric.cases,
            metric.threshold,
            metric.verdict
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

    let specs = load_metrics(&args.metrics)?;

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

    let mut sweep = Sweep::default();
    for dir in &dirs {
        let summary = read_summary(dir).map_err(|e| {
            CliError::Config(format!(
                "EVAL_ARTIFACTS_CORRUPT: failed to read summary.json in '{}': {}",
                dir.display(),
                e
            ))
        })?;
        absorb(&mut sweep, &summary);
    }

    let mut reports = Vec::new();
    let mut empty = Vec::new();
    for spec in &specs {
        match evaluate(spec, &sweep) {
            Some(report) => reports.push(report),
            None => empty.push(spec.name.clone()),
        }
    }

    let card = Scorecard {
        metrics: reports,
        totals: std::mem::take(&mut sweep.totals),
        unclaimed_checks: unclaimed(&specs, &sweep),
    };

    if use_json {
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&card).unwrap_or_default()
        );
    } else {
        render_table(&card);
    }

    // A metric that matched nothing is a broken scorecard, not a passing one,
    // and `--no-fail` does not suppress it: the flag says "report the numbers
    // rather than gate on them", and there are no numbers to report here.
    if !empty.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_SCORECARD_EMPTY_METRIC: {} metric(s) matched no observed check result: {}. \
             Check the case patterns and check names against the artifacts.",
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
    use super::*;
    use fastskill_evals::artifacts::{CaseSummary, TrialResult};
    use fastskill_evals::checks::{CheckResult, NotObservable};

    fn check(name: &str, passed: bool) -> CheckResult {
        CheckResult {
            check_name: name.to_string(),
            passed,
            required: true,
            message: None,
            not_observable: None,
        }
    }

    fn trial(id: u32, status: CaseStatus, results: Vec<CheckResult>) -> TrialResult {
        TrialResult {
            trial_id: id,
            status,
            command_count: Some(3),
            input_tokens: None,
            output_tokens: None,
            check_results: results,
            error_message: None,
            exit_code: Some(0),
            terminal: None,
            cost_usd: Some(0.25),
            tokens: Default::default(),
            skill_path: None,
        }
    }

    fn case(id: &str, trials: Vec<TrialResult>) -> CaseSummary {
        CaseSummary {
            id: id.to_string(),
            status: CaseStatus::Passed,
            command_count: Some(3),
            input_tokens: None,
            output_tokens: None,
            pass_count: None,
            total_trials: None,
            pass_rate: None,
            error_count: None,
            scored_trials: None,
            should_trigger: None,
            trials,
        }
    }

    fn summary(cases: Vec<CaseSummary>) -> SummaryResult {
        SummaryResult {
            suite_pass: true,
            suite_pass_rate: None,
            agent: "codex".to_string(),
            model: None,
            total_cases: cases.len(),
            passed: cases.len(),
            failed: 0,
            trials_per_case: None,
            parallel: None,
            pass_threshold: None,
            run_dir: PathBuf::from("/tmp/run"),
            checks_path: None,
            skill_project_root: PathBuf::from("/tmp"),
            isolation: None,
            cases,
        }
    }

    fn rate_metric(name: &str, cases: &[&str], checks: &[&str], min_rate: f64) -> MetricSpec {
        MetricSpec {
            name: name.to_string(),
            cases: cases.iter().map(|s| s.to_string()).collect(),
            kind: MetricKind::CheckRate {
                checks: checks.iter().map(|s| s.to_string()).collect(),
                min_rate,
            },
        }
    }

    #[test]
    fn wildcards_anchor_at_both_ends() {
        assert!(matches_pattern("op-*", "op-init"));
        assert!(!matches_pattern("op-*", "c-op-init"));
        assert!(matches_pattern("*-init", "op-init"));
        assert!(matches_pattern("op-init", "op-init"));
        assert!(!matches_pattern("op-init", "op-initialize"));
        assert!(matches_pattern("*", "anything"));
        assert!(matches_pattern("a*b*c", "azzbzzc"));
        assert!(!matches_pattern("a*b*c", "azzbzz"));
    }

    /// The defect this whole command exists to prevent: an errored trial carries
    /// check results, every negative expectation in them passed vacuously, and a
    /// reader that folds them reports an outage as a clean sweep.
    #[test]
    fn errored_trials_never_reach_a_rate() {
        let mut sweep = Sweep::default();
        absorb(
            &mut sweep,
            &summary(vec![case(
                "op-init",
                vec![
                    trial(1, CaseStatus::Passed, vec![check("skill_invoked", true)]),
                    trial(2, CaseStatus::Error, vec![check("skill_invoked", true)]),
                    trial(3, CaseStatus::Error, vec![check("skill_invoked", true)]),
                ],
            )]),
        );
        let spec = rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85);
        let report = evaluate(&spec, &sweep).expect("metric matched");
        assert_eq!(report.observed, 1, "two errored trials must be dropped");
        assert_eq!(report.passed, 1);
        assert_eq!(sweep.totals.error_trials, 2);
        assert_eq!(sweep.totals.scored_trials, 1);
    }

    #[test]
    fn a_case_whose_every_trial_errored_is_named() {
        let mut sweep = Sweep::default();
        absorb(
            &mut sweep,
            &summary(vec![case(
                "op-dead",
                vec![trial(1, CaseStatus::Error, vec![])],
            )]),
        );
        assert_eq!(sweep.totals.unmeasured_cases, vec!["op-dead".to_string()]);
    }

    #[test]
    fn not_observable_results_are_excluded_not_failed() {
        let mut result = check("skill_invoked", false);
        result.not_observable = Some(NotObservable {
            reason: "no structured tools".to_string(),
        });
        let mut sweep = Sweep::default();
        absorb(
            &mut sweep,
            &summary(vec![case(
                "op-init",
                vec![
                    trial(1, CaseStatus::Passed, vec![result]),
                    trial(2, CaseStatus::Passed, vec![check("skill_invoked", true)]),
                ],
            )]),
        );
        let spec = rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85);
        let report = evaluate(&spec, &sweep).expect("metric matched");
        assert_eq!(
            report.observed, 1,
            "unobservable result is not a denominator"
        );
        assert_eq!(report.passed, 1);
        assert_eq!(sweep.totals.not_observable_checks, 1);
    }

    /// Two checks on the same trial keep their own rates. Folding them into one
    /// per-case verdict would report a budget overrun as a recall failure.
    #[test]
    fn each_check_type_keeps_its_own_rate() {
        let mut sweep = Sweep::default();
        absorb(
            &mut sweep,
            &summary(vec![case(
                "op-init",
                vec![trial(
                    1,
                    CaseStatus::Failed,
                    vec![check("skill_invoked", true), check("max_tool_calls", false)],
                )],
            )]),
        );
        let open = evaluate(
            &rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85),
            &sweep,
        )
        .expect("matched");
        let budget = evaluate(
            &rate_metric("Budget", &["op-*"], &["max_tool_calls"], 0.90),
            &sweep,
        )
        .expect("matched");
        assert_eq!(open.verdict, "PASS");
        assert_eq!(budget.verdict, "BELOW THRESHOLD");
    }

    #[test]
    fn case_patterns_partition_the_sweep() {
        let mut sweep = Sweep::default();
        absorb(
            &mut sweep,
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
                    "off-docker",
                    vec![trial(
                        1,
                        CaseStatus::Failed,
                        vec![check("skill_invoked", false)],
                    )],
                ),
            ]),
        );
        let consult = evaluate(
            &rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85),
            &sweep,
        )
        .expect("matched");
        let restraint = evaluate(
            &rate_metric("Restraint", &["off-*"], &["skill_invoked"], 0.90),
            &sweep,
        )
        .expect("matched");
        assert_eq!((consult.observed, consult.passed), (1, 1));
        assert_eq!((restraint.observed, restraint.passed), (1, 0));
    }

    /// A mistyped pattern must not silently delete a gate.
    #[test]
    fn a_metric_matching_nothing_reports_no_data() {
        let mut sweep = Sweep::default();
        absorb(
            &mut sweep,
            &summary(vec![case(
                "op-init",
                vec![trial(
                    1,
                    CaseStatus::Passed,
                    vec![check("skill_invoked", true)],
                )],
            )]),
        );
        let spec = rate_metric("Typo", &["typo-*"], &["skill_invoked"], 0.85);
        assert!(evaluate(&spec, &sweep).is_none());
    }

    #[test]
    fn unclaimed_checks_are_named() {
        let mut sweep = Sweep::default();
        absorb(
            &mut sweep,
            &summary(vec![case(
                "op-init",
                vec![trial(
                    1,
                    CaseStatus::Passed,
                    vec![
                        check("skill_invoked", true),
                        check("command_contains", true),
                    ],
                )],
            )]),
        );
        let specs = vec![rate_metric(
            "Skill-open",
            &["op-*"],
            &["skill_invoked"],
            0.85,
        )];
        let orphans = unclaimed(&specs, &sweep);
        assert_eq!(orphans, vec!["command_contains (1 case(s))".to_string()]);
    }

    #[test]
    fn p95_is_a_ceiling_over_scored_trials() {
        let mut sweep = Sweep::default();
        sweep.tool_counts = (1..=20)
            .map(|calls| ToolCount {
                case_id: "op-init".to_string(),
                calls,
            })
            .collect();
        let spec = MetricSpec {
            name: "Efficiency".to_string(),
            cases: vec!["op-*".to_string()],
            kind: MetricKind::ToolCallsP95 { max: 25 },
        };
        let report = evaluate(&spec, &sweep).expect("matched");
        assert_eq!(report.p95_tool_calls, Some(19));
        assert_eq!(report.verdict, "PASS");

        let strict = MetricSpec {
            name: "Efficiency".to_string(),
            cases: vec!["op-*".to_string()],
            kind: MetricKind::ToolCallsP95 { max: 5 },
        };
        assert_eq!(
            evaluate(&strict, &sweep).expect("matched").verdict,
            "OVER CEILING"
        );
    }

    #[test]
    fn cost_is_absent_rather_than_zero_when_no_vendor_reported_one() {
        let mut silent = trial(1, CaseStatus::Passed, vec![check("skill_invoked", true)]);
        silent.cost_usd = None;
        let mut sweep = Sweep::default();
        absorb(&mut sweep, &summary(vec![case("op-init", vec![silent])]));
        assert_eq!(sweep.totals.cost_usd, None);
        assert_eq!(sweep.totals.trials_without_cost, 1);
    }

    #[test]
    fn a_metrics_file_needs_at_least_one_metric() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.toml");
        std::fs::write(&path, "# nothing here\n").unwrap();
        let err = load_metrics(&path).unwrap_err().to_string();
        assert!(err.contains("EVAL_SCORECARD_CONFIG"), "{err}");
        assert!(err.contains("no [[metric]] entries"), "{err}");
    }

    #[test]
    fn metrics_parse_both_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.toml");
        std::fs::write(
            &path,
            r#"
[[metric]]
name = "Skill-open rate"
kind = "check_rate"
cases = ["op-*"]
checks = ["skill_invoked"]
min_rate = 0.85

[[metric]]
name = "Efficiency"
kind = "tool_calls_p95"
cases = ["op-*"]
max = 25
"#,
        )
        .unwrap();
        let specs = load_metrics(&path).expect("parsed");
        assert_eq!(specs.len(), 2);
        assert!(matches!(specs[0].kind, MetricKind::CheckRate { .. }));
        assert!(matches!(
            specs[1].kind,
            MetricKind::ToolCallsP95 { max: 25 }
        ));
    }

    #[test]
    fn run_dirs_finds_every_summary_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        for suite in ["consultation", "restraint"] {
            let run = dir.path().join(suite).join("2026-09-03T00-00-00Z/codex");
            std::fs::create_dir_all(&run).unwrap();
            std::fs::write(run.join("summary.json"), "{}").unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        assert_eq!(run_dirs(dir.path()).len(), 2);
    }
}

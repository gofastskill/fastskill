//! Folding run artifacts into what a metric can be evaluated over.
//!
//! ## Why the folding cannot be left to a post-processor
//!
//! A trial with outcome `error` still carries check results. The checks ran,
//! over an empty or truncated trace, and every negative expectation and every
//! tool-call ceiling passed vacuously. Any reader that folds `check_results`
//! without first dropping errored trials reports an outage as perfect
//! restraint. The same holds for a check the backend could not observe, which
//! records `passed: false` for older readers and must not be counted as a
//! failure by anything that understands the field.
//!
//! Both facts are recorded in the artifact. Honouring them is not optional, so
//! it belongs in the tool that ships with the engine rather than in a script
//! beside the cases.

use super::document::{
    criterion_rows, CaseRow, CheckRow, JudgeEntry, JudgmentRow, MetricReport, ScorecardTotals,
};
use super::metrics::{MetricKind, MetricSpec};
use crate::error::{CliError, CliResult};
use fastskill_evals::artifacts::{CaseStatus, SummaryResult};
use fastskill_evals::judge::{latest_for, read_judgments, Judgment};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A `judge:<name>` row carries a judge's `overall`; deterministic checks do
/// not. The prefix is the engine's, not this command's invention.
const JUDGE_ROW_PREFIX: &str = "judge:";

/// One observed check result, flattened out of the run tree.
pub struct Observation {
    pub case_id: String,
    pub check_name: String,
    pub passed: bool,
    /// `false` on an advisory judge row, which never gates and so must never
    /// move a rate (spec eval-scorecard-report R3).
    pub required: bool,
}

/// One scored trial's tool-call count.
pub struct ToolCount {
    pub case_id: String,
    pub calls: usize,
}

/// The latest judgment of one judge on one trial, reduced to what a
/// `judge_score` metric reads.
pub struct JudgeObservation {
    pub case_id: String,
    pub judge: String,
    pub judge_hash: String,
    /// Normalised per-criterion scores plus `overall`.
    pub scores: BTreeMap<String, f64>,
}

/// Everything the runs under one root recorded, in the shapes the metrics read.
#[derive(Default)]
pub struct Observations {
    pub observations: Vec<Observation>,
    pub tool_counts: Vec<ToolCount>,
    pub judgments: Vec<JudgeObservation>,
    /// Distinct judge identities, keyed by `judge_hash` (R2).
    pub judges: BTreeMap<String, JudgeEntry>,
    pub cases: Vec<CaseRow>,
    pub totals: ScorecardTotals,
}

/// Fold one run's summary into the observations, and build its `cases[]` rows.
///
/// Errored trials are dropped before their check results are read. They carry
/// results, and those results are vacuous — see the module docs. That is also
/// why `cases[].checks` holds rows for scored trials only: the document must
/// not hand a reader the same trap in a new shape.
pub fn absorb(obs: &mut Observations, summary: &SummaryResult, run_dir: &Path) -> CliResult<()> {
    obs.totals.runs += 1;
    for case in &summary.cases {
        obs.totals.cases += 1;
        let mut row = CaseRow {
            case_id: case.id.clone(),
            run_dir: run_dir.to_path_buf(),
            status: case.status.clone(),
            trials: case.trials.len(),
            scored_trials: 0,
            error_count: 0,
            judge_excluded_count: 0,
            checks: Vec::new(),
            judgments: Vec::new(),
        };
        for trial in &case.trials {
            obs.totals.trials += 1;
            if trial.status == CaseStatus::Error {
                obs.totals.error_trials += 1;
                row.error_count += 1;
                continue;
            }
            if trial.judge_excluded {
                obs.totals.judge_excluded_trials += 1;
                row.judge_excluded_count += 1;
            }
            row.scored_trials += 1;
            obs.totals.scored_trials += 1;
            match trial.cost_usd {
                Some(usd) => {
                    *obs.totals.cost_usd.get_or_insert(0.0) += usd;
                }
                None => obs.totals.trials_without_cost += 1,
            }
            if let Some(calls) = trial.command_count {
                obs.tool_counts.push(ToolCount {
                    case_id: case.id.clone(),
                    calls,
                });
            }
            for result in &trial.check_results {
                row.checks.push(CheckRow {
                    trial_id: trial.trial_id,
                    name: result.check_name.clone(),
                    passed: result.passed,
                    observed: result.not_observable.is_none(),
                    not_observable: result.not_observable.as_ref().map(|n| n.reason.clone()),
                    score: result.score,
                });
                if result.not_observable.is_some() {
                    obs.totals.not_observable_checks += 1;
                    continue;
                }
                obs.observations.push(Observation {
                    case_id: case.id.clone(),
                    check_name: result.check_name.clone(),
                    passed: result.passed,
                    required: result.required,
                });
            }
            absorb_judgments(obs, &mut row, run_dir, &case.id, trial.trial_id)?;
        }
        if row.scored_trials == 0 && !case.trials.is_empty() {
            obs.totals.unmeasured_cases.push(case.id.clone());
        }
        obs.cases.push(row);
    }
    Ok(())
}

/// Read one trial's judgments and fold the latest per judge.
///
/// Every attempt of every judgment counts towards the token totals: that is
/// what was spent. Only the latest judgment per judge counts towards a score,
/// which is what `latest_for` means.
fn absorb_judgments(
    obs: &mut Observations,
    row: &mut CaseRow,
    run_dir: &Path,
    case_id: &str,
    trial_id: u32,
) -> CliResult<()> {
    let trial_dir = run_dir.join(case_id).join(format!("trial-{}", trial_id));
    let all = read_judgments(&trial_dir).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: failed to read judgments in '{}': {}",
            trial_dir.display(),
            e
        ))
    })?;
    for judgment in &all {
        obs.totals.judge_tokens.add(&judgment.usage);
    }
    let mut names: Vec<&str> = Vec::new();
    for judgment in &all {
        if !names.contains(&judgment.judge.as_str()) {
            names.push(&judgment.judge);
        }
    }
    for name in names {
        let Some(latest) = latest_for(&all, name) else {
            continue;
        };
        record_latest(obs, row, case_id, trial_id, latest);
    }
    Ok(())
}

fn record_latest(
    obs: &mut Observations,
    row: &mut CaseRow,
    case_id: &str,
    trial_id: u32,
    latest: &Judgment,
) {
    obs.judges
        .entry(latest.judge_hash.clone())
        .or_insert_with(|| JudgeEntry {
            name: latest.judge.clone(),
            judge_hash: latest.judge_hash.clone(),
            identity: latest.identity.clone(),
        });
    row.judgments.push(JudgmentRow {
        trial_id,
        judge: latest.judge.clone(),
        judge_hash: latest.judge_hash.clone(),
        overall: latest.overall(),
        criteria: criterion_rows(latest),
        error: latest.error.clone(),
        judged_at: latest.judged_at.clone(),
    });
    if latest.error.is_some() {
        obs.totals.judge_errors += 1;
    }
    if let Some(scores) = &latest.scores {
        obs.judgments.push(JudgeObservation {
            case_id: case_id.to_string(),
            judge: latest.judge.clone(),
            judge_hash: latest.judge_hash.clone(),
            scores: scores.clone(),
        });
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

/// An advisory judge row never gates, so counting it in a `check_rate` would
/// let a judge nobody is gating on move a number somebody is (R3).
fn counts_towards_a_rate(observation: &Observation) -> bool {
    !observation.check_name.starts_with(JUDGE_ROW_PREFIX) || observation.required
}

/// Evaluate one metric. `None` means the metric matched nothing, which is a
/// failure rather than a line to omit: a mistyped case pattern would otherwise
/// make a gate quietly disappear.
pub fn evaluate(
    spec: &MetricSpec,
    obs: &Observations,
    mixed_targets: bool,
) -> Option<MetricReport> {
    let report = match &spec.kind {
        MetricKind::CheckRate { checks, min_rate } => {
            let wanted: BTreeSet<&str> = checks.iter().map(String::as_str).collect();
            let hits: Vec<&Observation> = obs
                .observations
                .iter()
                .filter(|o| {
                    spec.covers(&o.case_id)
                        && wanted.contains(o.check_name.as_str())
                        && counts_towards_a_rate(o)
                })
                .collect();
            if hits.is_empty() {
                return None;
            }
            let passed = hits.iter().filter(|o| o.passed).count();
            let observed = hits.len();
            let rate = passed as f64 / observed as f64;
            let cases: BTreeSet<&str> = hits.iter().map(|o| o.case_id.as_str()).collect();
            MetricReport {
                name: spec.name.clone(),
                rate: Some(rate),
                p95_tool_calls: None,
                score: None,
                passed,
                observed,
                cases: cases.len(),
                threshold: format!(">= {:.0}%", min_rate * 100.0),
                verdict: if rate >= *min_rate {
                    "PASS"
                } else {
                    "BELOW THRESHOLD"
                },
                mixed_judges: false,
                mixed_targets,
            }
        }
        MetricKind::ToolCallsP95 { max } => {
            let hits: Vec<&ToolCount> = obs
                .tool_counts
                .iter()
                .filter(|t| spec.covers(&t.case_id))
                .collect();
            let value = percentile95(hits.iter().map(|t| t.calls).collect())?;
            let cases: BTreeSet<&str> = hits.iter().map(|t| t.case_id.as_str()).collect();
            MetricReport {
                name: spec.name.clone(),
                rate: None,
                p95_tool_calls: Some(value),
                score: None,
                passed: hits.iter().filter(|t| t.calls <= *max).count(),
                observed: hits.len(),
                cases: cases.len(),
                threshold: format!("<= {}", max),
                verdict: if value <= *max {
                    "PASS"
                } else {
                    "OVER CEILING"
                },
                mixed_judges: false,
                mixed_targets,
            }
        }
        MetricKind::JudgeScore {
            judges,
            criterion,
            min_score,
        } => {
            let wanted: BTreeSet<&str> = judges.iter().map(String::as_str).collect();
            let key = criterion.as_deref().unwrap_or("overall");
            // A judgment that does not carry the named criterion is not a zero:
            // the criterion belongs to a different judge, and averaging it in
            // as absent would invent a score nobody rendered.
            let hits: Vec<(&JudgeObservation, f64)> = obs
                .judgments
                .iter()
                .filter(|j| spec.covers(&j.case_id) && wanted.contains(j.judge.as_str()))
                .filter_map(|j| j.scores.get(key).map(|v| (j, *v)))
                .collect();
            if hits.is_empty() {
                return None;
            }
            let value = hits.iter().map(|(_, v)| *v).sum::<f64>() / hits.len() as f64;
            let cases: BTreeSet<&str> = hits.iter().map(|(j, _)| j.case_id.as_str()).collect();
            let hashes: BTreeSet<&str> = hits.iter().map(|(j, _)| j.judge_hash.as_str()).collect();
            MetricReport {
                name: spec.name.clone(),
                rate: None,
                p95_tool_calls: None,
                score: Some(value),
                passed: hits.iter().filter(|(_, v)| *v >= *min_score).count(),
                observed: hits.len(),
                cases: cases.len(),
                threshold: format!(">= {:.2}", min_score),
                verdict: if value >= *min_score {
                    "PASS"
                } else {
                    "BELOW THRESHOLD"
                },
                mixed_judges: hashes.len() > 1,
                mixed_targets,
            }
        }
    };
    Some(report)
}

/// Check results that no metric both covers by case and names.
///
/// Reported as `case-id/check-name` groups, deduplicated by check name. A
/// `judge:<name>` row is claimed by a `judge_score` metric that names that
/// judge, which is how an advisory judge stays reported without gating.
pub fn unclaimed(specs: &[MetricSpec], obs: &Observations) -> Vec<String> {
    let mut by_name: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for observation in &obs.observations {
        let judge = observation
            .check_name
            .strip_prefix(JUDGE_ROW_PREFIX)
            .unwrap_or_default();
        let claimed = specs.iter().any(|spec| {
            spec.covers(&observation.case_id)
                && match &spec.kind {
                    MetricKind::CheckRate { checks, .. } => {
                        checks.iter().any(|c| c == &observation.check_name)
                    }
                    MetricKind::ToolCallsP95 { .. } => false,
                    MetricKind::JudgeScore { judges, .. } => {
                        !judge.is_empty() && judges.iter().any(|j| j == judge)
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;
    use fastskill_evals::checks::NotObservable;

    /// The defect this whole command exists to prevent: an errored trial carries
    /// check results, every negative expectation in them passed vacuously, and a
    /// reader that folds them reports an outage as a perfect score.
    #[test]
    fn errored_trials_never_reach_a_rate() {
        let obs = fold(&summary(vec![case(
            "op-init",
            vec![
                trial(1, CaseStatus::Passed, vec![check("skill_invoked", true)]),
                trial(2, CaseStatus::Error, vec![check("skill_invoked", true)]),
                trial(3, CaseStatus::Error, vec![check("skill_invoked", true)]),
            ],
        )]));
        let spec = rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85);
        let report = evaluate(&spec, &obs, false).expect("metric matched");
        assert_eq!(report.observed, 1, "two errored trials must be dropped");
        assert_eq!(report.passed, 1);
        assert_eq!(obs.totals.error_trials, 2);
        assert_eq!(obs.totals.scored_trials, 1);
        assert_eq!(
            obs.cases[0].checks.len(),
            1,
            "a case row must not re-offer the vacuous results"
        );
        assert_eq!(obs.cases[0].error_count, 2);
    }

    #[test]
    fn a_case_whose_every_trial_errored_is_named() {
        let obs = fold(&summary(vec![case(
            "op-dead",
            vec![trial(1, CaseStatus::Error, vec![])],
        )]));
        assert_eq!(obs.totals.unmeasured_cases, vec!["op-dead".to_string()]);
    }

    #[test]
    fn not_observable_results_are_excluded_not_failed() {
        let mut result = check("skill_invoked", false);
        result.not_observable = Some(NotObservable {
            reason: "no structured tools".to_string(),
        });
        let obs = fold(&summary(vec![case(
            "op-init",
            vec![
                trial(1, CaseStatus::Passed, vec![result]),
                trial(2, CaseStatus::Passed, vec![check("skill_invoked", true)]),
            ],
        )]));
        let spec = rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85);
        let report = evaluate(&spec, &obs, false).expect("metric matched");
        assert_eq!(
            report.observed, 1,
            "unobservable result is not a denominator"
        );
        assert_eq!(report.passed, 1);
        assert_eq!(obs.totals.not_observable_checks, 1);
        let row = obs.cases[0]
            .checks
            .iter()
            .find(|c| !c.observed)
            .expect("the row keeps the unobservable result");
        assert_eq!(row.not_observable.as_deref(), Some("no structured tools"));
    }

    /// Two checks on the same trial keep their own rates. Folding them into one
    /// per-case verdict would report a budget overrun as a recall failure.
    #[test]
    fn each_check_type_keeps_its_own_rate() {
        let obs = fold(&summary(vec![case(
            "op-init",
            vec![trial(
                1,
                CaseStatus::Failed,
                vec![check("skill_invoked", true), check("max_tool_calls", false)],
            )],
        )]));
        let open = evaluate(
            &rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85),
            &obs,
            false,
        )
        .expect("matched");
        let budget = evaluate(
            &rate_metric("Budget", &["op-*"], &["max_tool_calls"], 0.90),
            &obs,
            false,
        )
        .expect("matched");
        assert_eq!(open.verdict, "PASS");
        assert_eq!(budget.verdict, "BELOW THRESHOLD");
    }

    #[test]
    fn case_patterns_partition_the_observations() {
        let obs = fold(&summary(vec![
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
        ]));
        let consult = evaluate(
            &rate_metric("Skill-open", &["op-*"], &["skill_invoked"], 0.85),
            &obs,
            false,
        )
        .expect("matched");
        let restraint = evaluate(
            &rate_metric("Restraint", &["off-*"], &["skill_invoked"], 0.90),
            &obs,
            false,
        )
        .expect("matched");
        assert_eq!((consult.observed, consult.passed), (1, 1));
        assert_eq!((restraint.observed, restraint.passed), (1, 0));
    }

    /// A mistyped pattern must not silently delete a gate.
    #[test]
    fn a_metric_matching_nothing_reports_no_data() {
        let obs = fold(&summary(vec![case(
            "op-init",
            vec![trial(
                1,
                CaseStatus::Passed,
                vec![check("skill_invoked", true)],
            )],
        )]));
        let spec = rate_metric("Typo", &["typo-*"], &["skill_invoked"], 0.85);
        assert!(evaluate(&spec, &obs, false).is_none());
    }

    #[test]
    fn unclaimed_checks_are_named() {
        let obs = fold(&summary(vec![case(
            "op-init",
            vec![trial(
                1,
                CaseStatus::Passed,
                vec![
                    check("skill_invoked", true),
                    check("command_contains", true),
                ],
            )],
        )]));
        let specs = vec![rate_metric(
            "Skill-open",
            &["op-*"],
            &["skill_invoked"],
            0.85,
        )];
        assert_eq!(
            unclaimed(&specs, &obs),
            vec!["command_contains (1 case(s))".to_string()]
        );
    }

    /// A judge row is reported by the metric that names the judge, not by a
    /// metric that names the row's synthesised check name.
    #[test]
    fn a_judge_score_metric_claims_the_judge_row() {
        let obs = fold(&summary(vec![case(
            "c-1",
            vec![trial(
                1,
                CaseStatus::Passed,
                vec![judge_check("quality", true, true, Some(0.8))],
            )],
        )]));
        let specs = vec![judge_metric("Quality", &["c-*"], &["quality"], None, 0.7)];
        assert!(unclaimed(&specs, &obs).is_empty());
        assert_eq!(
            unclaimed(&[], &obs),
            vec!["judge:quality (1 case(s))".to_string()]
        );
    }

    /// R3: an advisory judge never gates, so it must never move a `check_rate`.
    #[test]
    fn an_advisory_judge_row_never_moves_a_check_rate() {
        let obs = fold(&summary(vec![case(
            "c-1",
            vec![
                trial(
                    1,
                    CaseStatus::Passed,
                    vec![judge_check("advice", true, false, Some(0.1))],
                ),
                trial(
                    2,
                    CaseStatus::Failed,
                    vec![judge_check("gate", false, true, Some(0.1))],
                ),
            ],
        )]));
        let both = rate_metric("Judges", &["c-*"], &["judge:advice", "judge:gate"], 0.90);
        let report = evaluate(&both, &obs, false).expect("matched");
        assert_eq!(
            (report.observed, report.passed),
            (1, 0),
            "only the gated row counts"
        );
        let advisory_only = rate_metric("Advisory", &["c-*"], &["judge:advice"], 0.90);
        assert!(
            evaluate(&advisory_only, &obs, false).is_none(),
            "an advisory-only rate has nothing to measure"
        );
    }

    /// Fold a run whose trials carry judgments on disk, which is the only place
    /// per-criterion scores and reasoning exist: the `judge:<name>` check row
    /// carries `overall` and nothing else.
    fn fold_with_judgments(
        summary: &SummaryResult,
        stage: &dyn Fn(&Path),
    ) -> (Observations, tempfile::TempDir) {
        let root = tempfile::tempdir().expect("tempdir");
        stage(root.path());
        let mut obs = Observations::default();
        absorb(&mut obs, summary, root.path()).expect("judgments readable");
        (obs, root)
    }

    fn two_scored_trials() -> SummaryResult {
        summary(vec![case(
            "c-1",
            vec![
                trial(
                    1,
                    CaseStatus::Passed,
                    vec![judge_check("quality", true, true, Some(0.6))],
                ),
                trial(
                    2,
                    CaseStatus::Passed,
                    vec![judge_check("quality", true, true, Some(1.0))],
                ),
            ],
        )])
    }

    #[test]
    fn a_judge_score_metric_averages_the_latest_judgment_per_trial() {
        let (obs, _root) = fold_with_judgments(&two_scored_trials(), &|root| {
            // The superseded judgment must not move the mean: `--rejudge`
            // appends, it does not overwrite, so a re-judged trial has both.
            stage_judgment(
                root,
                "c-1",
                1,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("clarity", 0.0), ("overall", 0.0)]),
                    None,
                ),
            );
            stage_judgment(
                root,
                "c-1",
                1,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("clarity", 0.5), ("overall", 0.6)]),
                    None,
                ),
            );
            stage_judgment(
                root,
                "c-1",
                2,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("clarity", 1.0), ("overall", 1.0)]),
                    None,
                ),
            );
        });
        let report = evaluate(
            &judge_metric("Quality", &["c-*"], &["quality"], None, 0.7),
            &obs,
            false,
        )
        .expect("metric matched");
        assert_eq!(report.observed, 2, "one judgment per trial, not per record");
        assert_eq!(report.score, Some(0.8));
        assert_eq!(report.passed, 1, "only the 1.0 trial cleared 0.7");
        assert_eq!(report.verdict, "PASS");
        assert!(!report.mixed_judges);
        assert_eq!(
            obs.totals.judge_tokens.total, 360,
            "every attempt was paid for, superseded ones included"
        );
    }

    /// A criterion is scored on its own, not folded into `overall` first.
    #[test]
    fn a_named_criterion_is_scored_instead_of_overall() {
        let (obs, _root) = fold_with_judgments(&two_scored_trials(), &|root| {
            stage_judgment(
                root,
                "c-1",
                1,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("clarity", 0.2), ("overall", 0.6)]),
                    None,
                ),
            );
            stage_judgment(
                root,
                "c-1",
                2,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("clarity", 0.4), ("overall", 1.0)]),
                    None,
                ),
            );
        });
        let report = evaluate(
            &judge_metric("Clarity", &["c-*"], &["quality"], Some("clarity"), 0.7),
            &obs,
            false,
        )
        .expect("metric matched");
        let score = report.score.expect("a criterion score");
        assert!(
            (score - 0.3).abs() < 1e-9,
            "the mean of clarity 0.2 and 0.4 is 0.3, not overall's 0.8; got {score}"
        );
        assert_eq!(report.verdict, "BELOW THRESHOLD");
    }

    /// R4: two judge identities in one metric is a mean over two different
    /// instruments. It is reported, and the command refuses it upstream.
    #[test]
    fn two_judge_identities_in_one_metric_are_flagged() {
        let (obs, _root) = fold_with_judgments(&two_scored_trials(), &|root| {
            stage_judgment(
                root,
                "c-1",
                1,
                &judgment("quality", "h1", Some(vec![("overall", 0.6)]), None),
            );
            stage_judgment(
                root,
                "c-1",
                2,
                &judgment(
                    "quality",
                    "h2-different-model",
                    Some(vec![("overall", 1.0)]),
                    None,
                ),
            );
        });
        let report = evaluate(
            &judge_metric("Quality", &["c-*"], &["quality"], None, 0.7),
            &obs,
            false,
        )
        .expect("metric matched");
        assert!(report.mixed_judges, "two judge_hash values must be flagged");
        assert_eq!(obs.judges.len(), 2, "both identities are recorded");
    }

    /// R3: a judge error contributes nothing to the mean — not a zero — and is
    /// counted so the reader knows the denominator shrank.
    #[test]
    fn a_judge_error_contributes_nothing_and_is_counted() {
        let (obs, _root) = fold_with_judgments(&two_scored_trials(), &|root| {
            stage_judgment(
                root,
                "c-1",
                1,
                &judgment("quality", "h1", None, Some("upstream 503")),
            );
            stage_judgment(
                root,
                "c-1",
                2,
                &judgment("quality", "h1", Some(vec![("overall", 1.0)]), None),
            );
        });
        let report = evaluate(
            &judge_metric("Quality", &["c-*"], &["quality"], None, 0.7),
            &obs,
            false,
        )
        .expect("metric matched");
        assert_eq!(report.observed, 1, "the errored judgment is not a zero");
        assert_eq!(report.score, Some(1.0));
        assert_eq!(obs.totals.judge_errors, 1);
        let errored = obs.cases[0]
            .judgments
            .iter()
            .find(|j| j.error.is_some())
            .expect("the row keeps the error");
        assert_eq!(errored.error.as_deref(), Some("upstream 503"));
        assert!(errored.criteria.is_empty());
    }

    /// Two judges with different rubrics: naming a criterion only one of them
    /// scores must measure that one, not average the other in as a zero.
    #[test]
    fn a_judgment_that_does_not_score_the_named_criterion_is_not_a_zero() {
        let (obs, _root) = fold_with_judgments(&two_scored_trials(), &|root| {
            stage_judgment(
                root,
                "c-1",
                1,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("clarity", 0.9), ("overall", 0.9)]),
                    None,
                ),
            );
            stage_judgment(
                root,
                "c-1",
                2,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("brevity", 0.1), ("overall", 0.1)]),
                    None,
                ),
            );
        });
        let report = evaluate(
            &judge_metric("Clarity", &["c-*"], &["quality"], Some("clarity"), 0.7),
            &obs,
            false,
        )
        .expect("metric matched");
        assert_eq!(report.observed, 1, "only one judgment scored clarity");
        assert_eq!(report.score, Some(0.9));
        assert_eq!(report.verdict, "PASS");
    }

    /// The reasoning is what makes a score arguable, so it reaches `cases[]`
    /// in full rather than being summarised or dropped.
    #[test]
    fn a_case_row_carries_the_reasoning_behind_every_criterion() {
        let (obs, _root) = fold_with_judgments(&two_scored_trials(), &|root| {
            stage_judgment(
                root,
                "c-1",
                1,
                &judgment(
                    "quality",
                    "h1",
                    Some(vec![("clarity", 0.5), ("overall", 0.6)]),
                    None,
                ),
            );
        });
        let row = &obs.cases[0].judgments[0];
        assert_eq!(row.trial_id, 1);
        assert_eq!(row.overall, Some(0.6));
        assert_eq!(row.criteria.len(), 1);
        assert_eq!(row.criteria[0].name, "clarity");
        assert_eq!(
            row.criteria[0].reasoning.as_deref(),
            Some("clarity held up under the trial's own output.")
        );
        assert_eq!(row.criteria[0].answer, Some(serde_json::json!(4)));
    }

    #[test]
    fn p95_is_a_ceiling_over_scored_trials() {
        let obs = Observations {
            tool_counts: (1..=20)
                .map(|calls| ToolCount {
                    case_id: "op-init".to_string(),
                    calls,
                })
                .collect(),
            ..Default::default()
        };
        let spec = p95_metric("Efficiency", &["op-*"], 25);
        let report = evaluate(&spec, &obs, false).expect("matched");
        assert_eq!(report.p95_tool_calls, Some(19));
        assert_eq!(report.verdict, "PASS");
        assert_eq!(
            evaluate(&p95_metric("Efficiency", &["op-*"], 5), &obs, false)
                .expect("matched")
                .verdict,
            "OVER CEILING"
        );
    }

    #[test]
    fn cost_is_absent_rather_than_zero_when_no_vendor_reported_one() {
        let mut silent = trial(1, CaseStatus::Passed, vec![check("skill_invoked", true)]);
        silent.cost_usd = None;
        let obs = fold(&summary(vec![case("op-init", vec![silent])]));
        assert_eq!(obs.totals.cost_usd, None);
        assert_eq!(obs.totals.trials_without_cost, 1);
    }
}

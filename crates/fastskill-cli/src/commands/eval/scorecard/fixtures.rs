//! Artifact fixtures shared by the scorecard's unit tests.
//!
//! Built by hand rather than deserialised from a golden file: a test that
//! constructs the struct fails to compile when the engine adds a field, which
//! is the point at which somebody has to decide what the scorecard does with
//! it.

use super::metrics::{MetricKind, MetricSpec};
use super::observations::{absorb, Observations};
use fastskill_evals::artifacts::{CaseStatus, CaseSummary, SummaryResult, TrialResult};
use fastskill_evals::checks::CheckResult;
use fastskill_evals::judge::{
    append_judgment, AttemptRecord, Judgment, JudgmentIdentity, JudgmentUsage, JUDGMENT_SCHEMA,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn check(name: &str, passed: bool) -> CheckResult {
    CheckResult {
        check_name: name.to_string(),
        passed,
        required: true,
        message: None,
        not_observable: None,
        score: None,
    }
}

/// The row `flatten_row` writes for a judge: `required` is the judge's gating,
/// and `score` its `overall`.
pub fn judge_check(name: &str, passed: bool, gated: bool, score: Option<f64>) -> CheckResult {
    CheckResult {
        check_name: format!("judge:{}", name),
        passed,
        required: gated,
        message: None,
        not_observable: None,
        score,
    }
}

pub fn trial(id: u32, status: CaseStatus, results: Vec<CheckResult>) -> TrialResult {
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
        judge_excluded: false,
    }
}

pub fn case(id: &str, trials: Vec<TrialResult>) -> CaseSummary {
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
        judge_excluded_count: None,
        scores: Default::default(),
        trials,
    }
}

pub fn summary(cases: Vec<CaseSummary>) -> SummaryResult {
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
        judge_errors: None,
        judge_skipped_trials: None,
        judge_tokens: None,
        judge_cost_usd: None,
        skill_git_sha: None,
        skill_dirty: None,
        cases,
    }
}

/// Fold one summary whose trials left no judgment files on disk.
///
/// The run directory does not exist, and `read_judgments` reports that as "no
/// judgments" rather than as an error — the same answer it gives for a run that
/// predates the judge tier.
pub fn fold(summary: &SummaryResult) -> Observations {
    let mut obs = Observations::default();
    absorb(&mut obs, summary, Path::new("/tmp/run")).expect("no judgments to read");
    obs
}

pub fn rate_metric(name: &str, cases: &[&str], checks: &[&str], min_rate: f64) -> MetricSpec {
    MetricSpec {
        name: name.to_string(),
        cases: cases.iter().map(|s| s.to_string()).collect(),
        kind: MetricKind::CheckRate {
            checks: checks.iter().map(|s| s.to_string()).collect(),
            min_rate,
        },
    }
}

pub fn p95_metric(name: &str, cases: &[&str], max: usize) -> MetricSpec {
    MetricSpec {
        name: name.to_string(),
        cases: cases.iter().map(|s| s.to_string()).collect(),
        kind: MetricKind::ToolCallsP95 { max },
    }
}

pub fn judge_metric(
    name: &str,
    cases: &[&str],
    judges: &[&str],
    criterion: Option<&str>,
    min_score: f64,
) -> MetricSpec {
    MetricSpec {
        name: name.to_string(),
        cases: cases.iter().map(|s| s.to_string()).collect(),
        kind: MetricKind::JudgeScore {
            judges: judges.iter().map(|s| s.to_string()).collect(),
            criterion: criterion.map(str::to_string),
            min_score,
        },
    }
}

/// One judgment record, as `judge` would have appended it.
pub fn judgment(
    judge: &str,
    hash: &str,
    scores: Option<Vec<(&str, f64)>>,
    error: Option<&str>,
) -> Judgment {
    let reply = scores.as_ref().map(|s| {
        let criteria: Vec<Value> = s
            .iter()
            .filter(|(name, _)| *name != "overall")
            .map(|(name, _)| {
                serde_json::json!({
                    "name": name,
                    "answer": 4,
                    "reasoning": format!("{} held up under the trial's own output.", name),
                })
            })
            .collect();
        serde_json::json!({ "criteria": criteria, "notes": null }).to_string()
    });
    Judgment {
        schema: JUDGMENT_SCHEMA.to_string(),
        judge: judge.to_string(),
        judge_hash: hash.to_string(),
        cache_key: format!("{}-{}", hash, judge),
        identity: JudgmentIdentity {
            model: "judge-1".to_string(),
            model_reported: None,
            endpoint_host: "api.example.com".to_string(),
            temperature: 0.0,
            top_p: None,
            max_tokens: 1024,
        },
        attempts: reply
            .map(|text| {
                vec![AttemptRecord {
                    kind: aikit_sdk::AttemptKind::Validation,
                    request: serde_json::json!({}),
                    response_text: Some(text),
                    finish_reason: None,
                    usage: None,
                    error: None,
                }]
            })
            .unwrap_or_default(),
        scores: scores.map(|s| s.into_iter().map(|(n, v)| (n.to_string(), v)).collect()),
        error: error.map(str::to_string),
        usage: JudgmentUsage {
            input: 100,
            output: 20,
            total: 120,
        },
        cost_usd: None,
        truncated: vec![],
        judged_at: "2026-09-04T12:00:00Z".to_string(),
    }
}

/// Append a judgment where `absorb` will look for it.
pub fn stage_judgment(run_dir: &Path, case_id: &str, trial_id: u32, record: &Judgment) {
    let dir = run_dir.join(case_id).join(format!("trial-{}", trial_id));
    std::fs::create_dir_all(&dir).expect("trial dir");
    append_judgment(&dir, record).expect("append judgment");
}

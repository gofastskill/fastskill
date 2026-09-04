//! The `fastskill.scorecard/1` document (spec eval-scorecard-report R1, R2).
//!
//! The run directories a benchmark leaves behind are large, live in scratch
//! space and get deleted. This document is what survives: it is committed,
//! pasted into a pull request and compared with last month's. So it carries
//! everything — the identity of what was measured, and every case row in full.
//! Nothing here is summarised; the reader decides what to fold.
//!
//! Every key the scorecard emitted before this document existed keeps its
//! name, type and meaning. New keys sit beside them (ADR 0020).

use fastskill_evals::artifacts::CaseStatus;
use fastskill_evals::judge::{Judgment, JudgmentIdentity, JudgmentUsage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCORECARD_SCHEMA: &str = "fastskill.scorecard/1";

/// The `aikit-evals` build this binary was compiled against, from the
/// workspace lockfile at build time (see `build.rs`).
pub const AIKIT_EVALS_VERSION: &str = env!("FASTSKILL_AIKIT_EVALS_VERSION");

fn is_false(b: &bool) -> bool {
    !*b
}

/// One `(agent, model)` pair seen under the root, and how many runs used it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetEntry {
    pub agent: String,
    pub model: Option<String>,
    pub runs: usize,
}

/// One run folded into the scorecard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEntry {
    pub run_dir: PathBuf,
    /// The run id in the directory path, as RFC 3339. `None` when no path
    /// segment is one — the file's mtime is not a start time and is not used.
    pub started_at: Option<String>,
    pub agent: String,
    pub model: Option<String>,
}

/// Which skill produced the numbers (R2). Copied from `summary.json`, never
/// resolved from the working tree at scorecard time: the skill on disk now is
/// not the skill that ran.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillIdentity {
    pub path: Option<PathBuf>,
    pub git_sha: Option<String>,
    pub dirty: Option<bool>,
}

/// Which question was asked (R2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkIdentity {
    pub path: PathBuf,
    pub sha256: Option<String>,
}

/// One distinct judge identity seen across the selected judgments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeEntry {
    pub name: String,
    pub judge_hash: String,
    pub identity: JudgmentIdentity,
}

/// One check result as recorded, with the fields a reader needs to avoid
/// counting it wrongly: `observed` is false exactly when the backend could not
/// produce the evidence, and `passed` is then meaningless.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRow {
    pub trial_id: u32,
    pub name: String,
    pub passed: bool,
    pub observed: bool,
    pub not_observable: Option<String>,
    /// A judge's `overall` on a `judge:<name>` row; `None` on every
    /// deterministic check.
    pub score: Option<f64>,
}

/// One criterion of one judgment: the normalised score the engine computed,
/// and the answer and reasoning the judge gave, in full.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionRow {
    pub name: String,
    pub score: f64,
    pub answer: Option<Value>,
    pub reasoning: Option<String>,
}

/// The latest judgment of one judge on one trial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgmentRow {
    pub trial_id: u32,
    pub judge: String,
    pub judge_hash: String,
    pub overall: Option<f64>,
    pub criteria: Vec<CriterionRow>,
    pub error: Option<String>,
    pub judged_at: String,
}

/// One `(run, case)` row. The case id alone is not a key: the same case can
/// appear in several runs under one root, and each occurrence keeps its own
/// `run_dir` (R4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRow {
    pub case_id: String,
    pub run_dir: PathBuf,
    pub status: CaseStatus,
    pub trials: usize,
    pub scored_trials: usize,
    pub error_count: usize,
    pub judge_excluded_count: usize,
    pub checks: Vec<CheckRow>,
    pub judgments: Vec<JudgmentRow>,
}

/// One metric's result. `observed` is the denominator that was actually
/// measured, never the number of trials attempted.
#[derive(Debug, Serialize, Deserialize)]
pub struct MetricReport {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95_tool_calls: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub passed: usize,
    pub observed: usize,
    pub cases: usize,
    pub threshold: String,
    pub verdict: Cow<'static, str>,
    /// Set when this metric folded judgments from more than one judge identity
    /// and `--allow-mixed-judges` let it through (R4).
    #[serde(default, skip_serializing_if = "is_false")]
    pub mixed_judges: bool,
    /// Set when the runs folded into this scorecard carried more than one
    /// `(agent, model)` pair and `--allow-mixed-targets` let it through (R4).
    #[serde(default, skip_serializing_if = "is_false")]
    pub mixed_targets: bool,
}

/// Everything the scorecard recorded that is reported but not gated.
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ScorecardTotals {
    pub runs: usize,
    pub cases: usize,
    pub trials: usize,
    pub scored_trials: usize,
    pub error_trials: usize,
    pub not_observable_checks: usize,
    /// Vendor-reported only. `None` when no trial reported a cost, which is a
    /// different statement from `Some(0.0)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    pub trials_without_cost: usize,
    /// Cases every one of whose trials errored, so they carry no measurement.
    pub unmeasured_cases: Vec<String>,
    /// Latest judgments that recorded an error instead of scores.
    pub judge_errors: usize,
    /// Non-errored trials a gated judge could not judge, as the run recorded
    /// them. They are outside every measurement, exactly like errors.
    pub judge_excluded_trials: usize,
    /// Tokens spent on every judgment attempt in every run folded here.
    pub judge_tokens: JudgeTokens,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JudgeTokens {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

impl JudgeTokens {
    /// Every attempt counts, not just the one that validated: a judgment that
    /// needed a retry cost both calls.
    pub fn add(&mut self, usage: &JudgmentUsage) {
        self.input += usage.input;
        self.output += usage.output;
        self.total += usage.total;
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Scorecard {
    pub schema: Cow<'static, str>,
    pub generated_at: String,
    pub targets: Vec<TargetEntry>,
    /// Set only when `targets` has one entry: with two, there is no single
    /// agent this scorecard is about, and naming one would be a lie (R4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub skill: SkillIdentity,
    pub benchmark: BenchmarkIdentity,
    pub runs: Vec<RunEntry>,
    pub fastskill_version: Cow<'static, str>,
    pub aikit_evals_version: Cow<'static, str>,
    pub judges: Vec<JudgeEntry>,
    pub metrics: Vec<MetricReport>,
    pub totals: ScorecardTotals,
    /// Check results no metric claims. A check that runs on every trial and is
    /// reported nowhere is an assertion nobody reads.
    pub unclaimed_checks: Vec<String>,
    pub cases: Vec<CaseRow>,
}

/// The run id a run directory carries, as RFC 3339.
///
/// `eval run` allocates `<output>/<YYYY-MM-DDTHH-MM-SSZ>[-N]/<agent>`, so the
/// start time is recorded in the path and nowhere else in the artifact. A path
/// with no such segment yields `None` rather than a guess: a file mtime says
/// when a file was last written, which is not when the run started.
pub fn started_at_from_path(run_dir: &Path) -> Option<String> {
    for component in run_dir.components().rev() {
        let name = component.as_os_str().to_string_lossy();
        // `2026-09-04T12-00-00Z`, optionally with the `-2` collision suffix.
        let stem = name.split_once("Z-").map(|(s, _)| s).unwrap_or_else(|| {
            name.strip_suffix('Z')
                .map(|_| name.as_ref())
                .unwrap_or(name.as_ref())
        });
        let stem = stem.strip_suffix('Z').unwrap_or(stem);
        let bytes = stem.as_bytes();
        if bytes.len() != 19 || bytes[10] != b'T' {
            continue;
        }
        let shaped = bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            10 => *b == b'T',
            13 | 16 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
        if shaped {
            let (date, time) = stem.split_at(10);
            return Some(format!("{}T{}Z", date, time[1..].replace('-', ":")));
        }
    }
    None
}

/// The criteria of one judgment, with the reasoning the judge wrote.
///
/// The normalised scores live on the judgment; the answer and the reasoning
/// live only in the reply text of the attempt that validated. Pairing them
/// here is what lets a reader see *why* a score is what it is.
pub fn criterion_rows(judgment: &Judgment) -> Vec<CriterionRow> {
    let scores = match &judgment.scores {
        Some(scores) => scores,
        None => return Vec::new(),
    };
    let replies = validated_reply(judgment);
    scores
        .iter()
        .filter(|(name, _)| name.as_str() != "overall")
        .map(|(name, score)| {
            let entry = replies.get(name.as_str());
            CriterionRow {
                name: name.clone(),
                score: *score,
                answer: entry.and_then(|e| e.get("answer").cloned()),
                reasoning: entry
                    .and_then(|e| e.get("reasoning"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }
        })
        .collect()
}

/// The `criteria` entries of the last attempt that produced a reply the schema
/// accepted, keyed by criterion name.
fn validated_reply(judgment: &Judgment) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let text = judgment
        .attempts
        .iter()
        .rev()
        .find(|a| a.error.is_none())
        .and_then(|a| a.response_text.as_deref());
    let Some(text) = text else { return out };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return out;
    };
    let Some(entries) = value.get("criteria").and_then(Value::as_array) else {
        return out;
    };
    for entry in entries {
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            out.insert(name.to_string(), entry.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastskill_evals::judge::{AttemptRecord, JudgmentUsage, JUDGMENT_SCHEMA};

    fn judgment_with(reply: Option<&str>, scores: Option<Vec<(&str, f64)>>) -> Judgment {
        Judgment {
            schema: JUDGMENT_SCHEMA.to_string(),
            judge: "quality".to_string(),
            judge_hash: "hash".to_string(),
            cache_key: "key".to_string(),
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
                        response_text: Some(text.to_string()),
                        finish_reason: None,
                        usage: None,
                        error: None,
                    }]
                })
                .unwrap_or_default(),
            scores: scores.map(|s| {
                s.into_iter()
                    .map(|(n, v)| (n.to_string(), v))
                    .collect::<BTreeMap<_, _>>()
            }),
            error: None,
            usage: JudgmentUsage::default(),
            cost_usd: None,
            truncated: vec![],
            judged_at: "2026-09-04T12:00:00Z".to_string(),
        }
    }

    #[test]
    fn a_run_directory_carries_its_start_time_in_its_path() {
        assert_eq!(
            started_at_from_path(Path::new("./eval-runs/2026-09-04T12-00-00Z/claude")),
            Some("2026-09-04T12:00:00Z".to_string())
        );
        assert_eq!(
            started_at_from_path(Path::new("/x/2026-09-04T12-00-00Z-3/codex")),
            Some("2026-09-04T12:00:00Z".to_string())
        );
        assert_eq!(started_at_from_path(Path::new("/tmp/run/claude")), None);
        assert_eq!(
            started_at_from_path(Path::new("/tmp/2026-09-04-not-a-run/claude")),
            None
        );
    }

    /// The whole reason `cases[]` exists: a score without its reasoning is a
    /// number nobody can argue with.
    #[test]
    fn criterion_rows_pair_the_score_with_the_reasoning_that_produced_it() {
        let judgment = judgment_with(
            Some(
                r#"{"criteria":[{"name":"clarity","reasoning":"The answer names every flag.","answer":4}]}"#,
            ),
            Some(vec![("clarity", 0.75), ("overall", 0.75)]),
        );
        let rows = criterion_rows(&judgment);
        assert_eq!(rows.len(), 1, "overall is not a criterion row");
        assert_eq!(rows[0].name, "clarity");
        assert_eq!(rows[0].score, 0.75);
        assert_eq!(rows[0].answer, Some(serde_json::json!(4)));
        assert_eq!(
            rows[0].reasoning.as_deref(),
            Some("The answer names every flag.")
        );
    }

    #[test]
    fn a_judgment_without_a_readable_reply_still_reports_its_scores() {
        let judgment = judgment_with(None, Some(vec![("clarity", 1.0), ("overall", 1.0)]));
        let rows = criterion_rows(&judgment);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reasoning, None);
        assert_eq!(rows[0].answer, None);
    }

    #[test]
    fn an_errored_judgment_has_no_criterion_rows() {
        assert!(criterion_rows(&judgment_with(None, None)).is_empty());
    }
}

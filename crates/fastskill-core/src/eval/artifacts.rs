//! Artifact layout and persistence for eval runs

use crate::eval::checks::CheckResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Status of a single eval case
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaseStatus {
    Passed,
    Failed,
    Error,
    Skipped,
}

impl std::fmt::Display for CaseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaseStatus::Passed => write!(f, "passed"),
            CaseStatus::Failed => write!(f, "failed"),
            CaseStatus::Error => write!(f, "error"),
            CaseStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// Per-case result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub id: String,
    pub status: CaseStatus,
    pub command_count: Option<usize>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub check_results: Vec<CheckResult>,
    pub error_message: Option<String>,
}

/// Per-trial result for a case
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialResult {
    pub trial_id: u32,
    pub status: CaseStatus,
    pub command_count: Option<usize>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub check_results: Vec<CheckResult>,
    pub error_message: Option<String>,
}

/// Aggregated results for a case across multiple trials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseTrialsResult {
    pub id: String,
    pub trials: Vec<TrialResult>,
    pub aggregated_status: CaseStatus,
    pub pass_count: u32,
    pub total_trials: u32,
    pub pass_rate: f64,
}

/// Aggregated run summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryResult {
    pub suite_pass: bool,
    #[serde(default)]
    pub suite_pass_rate: Option<f64>,
    pub agent: String,
    pub model: Option<String>,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    #[serde(default)]
    pub trials_per_case: Option<u32>,
    #[serde(default)]
    pub parallel: Option<u32>,
    #[serde(default)]
    pub pass_threshold: Option<f64>,
    pub run_dir: PathBuf,
    pub checks_path: Option<PathBuf>,
    pub skill_project_root: PathBuf,
    pub cases: Vec<CaseSummary>,
}

/// Per-case summary entry in summary.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseSummary {
    pub id: String,
    pub status: CaseStatus,
    pub command_count: Option<usize>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub pass_count: Option<u32>,
    #[serde(default)]
    pub total_trials: Option<u32>,
    #[serde(default)]
    pub pass_rate: Option<f64>,
    #[serde(default)]
    pub trials: Vec<TrialResult>,
}

/// All artifacts from a completed run
#[derive(Debug)]
pub struct RunArtifacts {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub summary: SummaryResult,
    pub case_results: Vec<CaseResult>,
}

/// Errors during artifact writing/reading
#[derive(Debug, Error)]
pub enum ArtifactsError {
    #[error("EVAL_ARTIFACTS_CORRUPT: IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("EVAL_ARTIFACTS_CORRUPT: JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("EVAL_ARTIFACTS_CORRUPT: Missing required field: {0}")]
    MissingField(String),
}

/// Allocate a run directory under output_dir using ISO 8601 timestamp format
/// Appends numeric suffix if directory already exists
pub fn allocate_run_dir(output_dir: &Path, run_id: &str) -> Result<PathBuf, ArtifactsError> {
    let base = output_dir.join(run_id);
    if !base.exists() {
        std::fs::create_dir_all(&base)?;
        return Ok(base);
    }

    // Append numeric suffix
    for i in 2..=999 {
        let candidate = output_dir.join(format!("{}-{}", run_id, i));
        if !candidate.exists() {
            std::fs::create_dir_all(&candidate)?;
            return Ok(candidate);
        }
    }

    // Fallback: use the base (it exists, will overwrite)
    Ok(base)
}

/// Write per-trial artifacts (stdout.txt, stderr.txt, trace.jsonl, result.json) under:
/// `{run_dir}/{case_id}/trial-{trial_id}/`
pub fn write_trial_artifacts(
    run_dir: &Path,
    case_id: &str,
    trial_id: u32,
    stdout: &[u8],
    stderr: &[u8],
    trace_jsonl: &str,
    result: &TrialResult,
) -> Result<PathBuf, ArtifactsError> {
    let trial_dir = run_dir.join(case_id).join(format!("trial-{}", trial_id));
    std::fs::create_dir_all(&trial_dir)?;

    std::fs::write(trial_dir.join("stdout.txt"), stdout)?;
    std::fs::write(trial_dir.join("stderr.txt"), stderr)?;
    std::fs::write(trial_dir.join("trace.jsonl"), trace_jsonl)?;

    let result_json = serde_json::to_string_pretty(result)?;
    std::fs::write(trial_dir.join("result.json"), result_json)?;

    Ok(trial_dir)
}

/// Write `{run_dir}/{case_id}/aggregated.json`
pub fn write_case_trials_summary(
    run_dir: &Path,
    case_id: &str,
    trials_result: &CaseTrialsResult,
) -> Result<(), ArtifactsError> {
    let case_dir = run_dir.join(case_id);
    std::fs::create_dir_all(&case_dir)?;
    let aggregated_json = serde_json::to_string_pretty(trials_result)?;
    std::fs::write(case_dir.join("aggregated.json"), aggregated_json)?;
    Ok(())
}

fn case_result_to_trial(case: &CaseResult, trial_id: u32) -> TrialResult {
    TrialResult {
        trial_id,
        status: case.status.clone(),
        command_count: case.command_count,
        input_tokens: case.input_tokens,
        output_tokens: case.output_tokens,
        check_results: case.check_results.clone(),
        error_message: case.error_message.clone(),
    }
}

/// Write per-case artifacts for backwards-compatible callers.
///
/// Artifacts are written as trial 1 under `{run_dir}/{case_id}/trial-1/`, and an
/// aggregated `{run_dir}/{case_id}/aggregated.json` is also created.
pub fn write_case_artifacts(
    run_dir: &Path,
    case_id: &str,
    stdout: &[u8],
    stderr: &[u8],
    trace_jsonl: &str,
    result: &CaseResult,
) -> Result<PathBuf, ArtifactsError> {
    let trial = case_result_to_trial(result, 1);
    let trial_dir =
        write_trial_artifacts(run_dir, case_id, 1, stdout, stderr, trace_jsonl, &trial)?;

    let pass_count = if result.status == CaseStatus::Passed {
        1
    } else {
        0
    };
    let pass_rate = pass_count as f64;
    let aggregated = CaseTrialsResult {
        id: result.id.clone(),
        trials: vec![trial],
        aggregated_status: result.status.clone(),
        pass_count,
        total_trials: 1,
        pass_rate,
    };
    write_case_trials_summary(run_dir, case_id, &aggregated)?;

    Ok(trial_dir)
}

/// Write summary.json
pub fn write_summary(run_dir: &Path, summary: &SummaryResult) -> Result<(), ArtifactsError> {
    let summary_json = serde_json::to_string_pretty(summary)?;
    std::fs::write(run_dir.join("summary.json"), summary_json)?;
    Ok(())
}

/// Read summary.json from a run directory
pub fn read_summary(run_dir: &Path) -> Result<SummaryResult, ArtifactsError> {
    let summary_path = run_dir.join("summary.json");
    let content = std::fs::read_to_string(&summary_path)?;
    let summary: SummaryResult = serde_json::from_str(&content)?;
    Ok(summary)
}

/// Read case result.json files from a run directory
pub fn read_case_results(run_dir: &Path) -> Result<Vec<CaseResult>, ArtifactsError> {
    let mut results = Vec::new();

    let entries = std::fs::read_dir(run_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let aggregated_path = path.join("aggregated.json");
            if aggregated_path.exists() {
                let content = std::fs::read_to_string(&aggregated_path)?;
                let aggregated: CaseTrialsResult = serde_json::from_str(&content)?;
                results.push(CaseResult {
                    id: aggregated.id,
                    status: aggregated.aggregated_status,
                    command_count: None,
                    input_tokens: None,
                    output_tokens: None,
                    check_results: vec![],
                    error_message: None,
                });
                continue;
            }

            // Legacy layout fallback: `{case_id}/result.json`
            let result_path = path.join("result.json");
            if result_path.exists() {
                let content = std::fs::read_to_string(&result_path)?;
                let result: CaseResult = serde_json::from_str(&content)?;
                results.push(result);
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_allocate_run_dir_creates_new() {
        let dir = TempDir::new().unwrap();
        let run_dir = allocate_run_dir(dir.path(), "2026-04-01T14-00-00Z").unwrap();
        assert!(run_dir.exists());
        assert!(run_dir.ends_with("2026-04-01T14-00-00Z"));
    }

    #[test]
    fn test_allocate_run_dir_suffix_on_conflict() {
        let dir = TempDir::new().unwrap();
        let run_dir1 = allocate_run_dir(dir.path(), "2026-04-01T14-00-00Z").unwrap();
        let run_dir2 = allocate_run_dir(dir.path(), "2026-04-01T14-00-00Z").unwrap();
        assert_ne!(run_dir1, run_dir2);
        assert!(run_dir2.to_string_lossy().contains("-2"));
    }

    #[test]
    fn test_write_and_read_summary() {
        let dir = TempDir::new().unwrap();
        let summary = SummaryResult {
            suite_pass: true,
            suite_pass_rate: Some(1.0),
            agent: "codex".to_string(),
            model: None,
            total_cases: 2,
            passed: 2,
            failed: 0,
            trials_per_case: Some(1),
            parallel: None,
            pass_threshold: Some(1.0),
            run_dir: dir.path().to_path_buf(),
            checks_path: None,
            skill_project_root: dir.path().to_path_buf(),
            cases: vec![],
        };

        write_summary(dir.path(), &summary).unwrap();
        let read = read_summary(dir.path()).unwrap();
        assert_eq!(read.total_cases, 2);
        assert!(read.suite_pass);
    }
}

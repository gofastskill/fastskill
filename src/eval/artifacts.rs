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

/// Aggregated run summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryResult {
    pub suite_pass: bool,
    pub agent: String,
    pub model: Option<String>,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
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

/// Write per-case artifacts (stdout.txt, stderr.txt, trace.jsonl, result.json)
pub fn write_case_artifacts(
    run_dir: &Path,
    case_id: &str,
    stdout: &[u8],
    stderr: &[u8],
    trace_jsonl: &str,
    result: &CaseResult,
) -> Result<PathBuf, ArtifactsError> {
    let case_dir = run_dir.join(case_id);
    std::fs::create_dir_all(&case_dir)?;

    std::fs::write(case_dir.join("stdout.txt"), stdout)?;
    std::fs::write(case_dir.join("stderr.txt"), stderr)?;
    std::fs::write(case_dir.join("trace.jsonl"), trace_jsonl)?;

    let result_json = serde_json::to_string_pretty(result)?;
    std::fs::write(case_dir.join("result.json"), result_json)?;

    Ok(case_dir)
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
            agent: "codex".to_string(),
            model: None,
            total_cases: 2,
            passed: 2,
            failed: 0,
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

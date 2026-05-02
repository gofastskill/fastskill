//! Deterministic check engine for eval artifact scoring

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

use crate::trace::{TraceEvent, TracePayload};

/// A deterministic check definition loaded from checks.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum CheckDefinition {
    /// Check whether a pattern appears (or doesn't appear) in the trace
    #[serde(rename = "trigger_expectation")]
    TriggerExpectation {
        pattern: String,
        /// true = pattern must appear; false = pattern must NOT appear
        expected: bool,
        #[serde(default = "default_required")]
        required: bool,
    },
    /// Check whether the trace contains a line with this pattern
    #[serde(rename = "command_contains")]
    CommandContains {
        pattern: String,
        #[serde(default = "default_required")]
        required: bool,
    },
    /// Check whether a file exists in the working directory after execution
    #[serde(rename = "file_exists")]
    FileExists {
        path: PathBuf,
        #[serde(default = "default_required")]
        required: bool,
    },
    /// Check that the number of raw_json trace lines does not exceed a limit
    #[serde(rename = "max_command_count")]
    MaxCommandCount {
        limit: usize,
        #[serde(default = "default_required")]
        required: bool,
    },
}

fn default_required() -> bool {
    true
}

impl CheckDefinition {
    pub fn name(&self) -> &str {
        match self {
            CheckDefinition::TriggerExpectation { .. } => "trigger_expectation",
            CheckDefinition::CommandContains { .. } => "command_contains",
            CheckDefinition::FileExists { .. } => "file_exists",
            CheckDefinition::MaxCommandCount { .. } => "max_command_count",
        }
    }

    pub fn is_required(&self) -> bool {
        match self {
            CheckDefinition::TriggerExpectation { required, .. } => *required,
            CheckDefinition::CommandContains { required, .. } => *required,
            CheckDefinition::FileExists { required, .. } => *required,
            CheckDefinition::MaxCommandCount { required, .. } => *required,
        }
    }
}

/// Result of a single check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_name: String,
    pub passed: bool,
    pub message: Option<String>,
}

/// TOML file structure for checks configuration
#[derive(Debug, Deserialize)]
pub struct ChecksToml {
    #[serde(rename = "check", default)]
    pub checks: Vec<CheckDefinition>,
}

/// Errors loading checks configuration
#[derive(Debug, Error)]
pub enum ChecksError {
    #[error("EVAL_CHECKS_INVALID: Failed to read checks file: {0}")]
    Io(#[from] std::io::Error),
    #[error("EVAL_CHECKS_INVALID: Failed to parse checks TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

/// Load check definitions from a TOML file
pub fn load_checks(path: &std::path::Path) -> Result<Vec<CheckDefinition>, ChecksError> {
    let content = std::fs::read_to_string(path)?;
    let parsed: ChecksToml = toml::from_str(&content)?;
    Ok(parsed.checks)
}

/// Run all checks against captured stdout content and working directory
pub fn run_checks(
    checks: &[CheckDefinition],
    stdout_content: &str,
    trace_jsonl: &str,
    working_dir: &std::path::Path,
) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|check| run_single_check(check, stdout_content, trace_jsonl, working_dir))
        .collect()
}

/// Count trace events with payload type `raw_json`.
pub fn count_raw_json_events(trace_jsonl: &str) -> usize {
    trace_jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<TraceEvent>(line).ok())
        .filter(|event| matches!(event.payload, TracePayload::RawJson { .. }))
        .count()
}

fn run_single_check(
    check: &CheckDefinition,
    stdout_content: &str,
    trace_jsonl: &str,
    working_dir: &std::path::Path,
) -> CheckResult {
    match check {
        CheckDefinition::TriggerExpectation {
            pattern, expected, ..
        } => {
            // Literal substring matching in stdout + trace
            let combined = format!("{}\n{}", stdout_content, trace_jsonl);
            let found = combined.contains(pattern.as_str());
            let passed = found == *expected;
            let message = if passed {
                None
            } else if *expected {
                Some(format!("Pattern '{}' not found but was expected", pattern))
            } else {
                Some(format!("Pattern '{}' found but was not expected", pattern))
            };
            CheckResult {
                check_name: "trigger_expectation".to_string(),
                passed,
                message,
            }
        }
        CheckDefinition::CommandContains { pattern, .. } => {
            let combined = format!("{}\n{}", stdout_content, trace_jsonl);
            let passed = combined.contains(pattern.as_str());
            let message = if passed {
                None
            } else {
                Some(format!("Pattern '{}' not found in output", pattern))
            };
            CheckResult {
                check_name: "command_contains".to_string(),
                passed,
                message,
            }
        }
        CheckDefinition::FileExists { path, .. } => {
            let full_path = working_dir.join(path);
            let passed = full_path.exists();
            let message = if passed {
                None
            } else {
                Some(format!("File '{}' does not exist", path.display()))
            };
            CheckResult {
                check_name: "file_exists".to_string(),
                passed,
                message,
            }
        }
        CheckDefinition::MaxCommandCount { limit, .. } => {
            // Count raw_json trace lines
            let count = count_raw_json_events(trace_jsonl);
            let passed = count <= *limit;
            let message = if passed {
                None
            } else {
                Some(format!("Command count {} exceeds limit {}", count, limit))
            };
            CheckResult {
                check_name: "max_command_count".to_string(),
                passed,
                message,
            }
        }
    }
}

/// Aggregate check results: suite passes if all required checks pass
pub fn suite_passes(results: &[CheckResult]) -> bool {
    results.iter().all(|r| r.passed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_trigger_expectation_passes_when_pattern_found() {
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: true,
            required: true,
        };
        let results = run_checks(&[check], "fastskill triggered", "", Path::new("/tmp"));
        assert!(results[0].passed);
    }

    #[test]
    fn test_trigger_expectation_fails_when_pattern_missing() {
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: true,
            required: true,
        };
        let results = run_checks(&[check], "nothing here", "", Path::new("/tmp"));
        assert!(!results[0].passed);
    }

    #[test]
    fn test_trigger_expectation_negative_passes_when_pattern_absent() {
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: false,
            required: true,
        };
        let results = run_checks(&[check], "no match", "", Path::new("/tmp"));
        assert!(results[0].passed);
    }

    #[test]
    fn test_file_exists_check_passes() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("output.txt");
        std::fs::write(&file_path, "content").unwrap();

        let check = CheckDefinition::FileExists {
            path: PathBuf::from("output.txt"),
            required: true,
        };
        let results = run_checks(&[check], "", "", dir.path());
        assert!(results[0].passed);
    }

    #[test]
    fn test_file_exists_check_fails() {
        let dir = TempDir::new().unwrap();
        let check = CheckDefinition::FileExists {
            path: PathBuf::from("missing.txt"),
            required: true,
        };
        let results = run_checks(&[check], "", "", dir.path());
        assert!(!results[0].passed);
    }

    #[test]
    fn test_max_command_count_passes() {
        let check = CheckDefinition::MaxCommandCount {
            limit: 5,
            required: true,
        };
        let trace = r#"{"seq":0,"payload":{"type":"raw_json","data":{"cmd":"a"}}}
{"seq":1,"payload":{"type":"raw_json","data":{"cmd":"b"}}}
{"seq":2,"payload":{"type":"raw_line","line":"ok"}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(results[0].passed);
    }

    #[test]
    fn test_max_command_count_fails() {
        let check = CheckDefinition::MaxCommandCount {
            limit: 1,
            required: true,
        };
        let trace = r#"{"seq":0,"payload":{"type":"raw_json","data":{"cmd":"a"}}}
{"seq":1,"payload":{"type":"raw_json","data":{"cmd":"b"}}}
{"seq":2,"payload":{"type":"raw_json","data":{"cmd":"c"}}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(!results[0].passed);
    }

    #[test]
    fn test_count_raw_json_events_ignores_substring_only() {
        let trace = r#"{"seq":0,"payload":{"type":"raw_line","line":"mentions raw_json text"}}
{"seq":1,"payload":{"type":"raw_json","data":{"cmd":"x"}}}"#;
        assert_eq!(count_raw_json_events(trace), 1);
    }

    #[test]
    fn test_suite_passes_all_passed() {
        let results = vec![
            CheckResult {
                check_name: "a".to_string(),
                passed: true,
                message: None,
            },
            CheckResult {
                check_name: "b".to_string(),
                passed: true,
                message: None,
            },
        ];
        assert!(suite_passes(&results));
    }

    #[test]
    fn test_suite_passes_any_failed() {
        let results = vec![
            CheckResult {
                check_name: "a".to_string(),
                passed: true,
                message: None,
            },
            CheckResult {
                check_name: "b".to_string(),
                passed: false,
                message: Some("failed".to_string()),
            },
        ];
        assert!(!suite_passes(&results));
    }

    #[test]
    fn test_load_checks_file_not_found() {
        let path = Path::new("/nonexistent/path/checks.toml");
        let result = load_checks(path);
        assert!(matches!(result, Err(ChecksError::Io(_))));
    }

    #[test]
    fn test_load_checks_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let checks_file = dir.path().join("checks.toml");
        std::fs::write(&checks_file, "this is not valid toml [[[[").unwrap();
        let result = load_checks(&checks_file);
        assert!(matches!(result, Err(ChecksError::Parse(_))));
    }
}

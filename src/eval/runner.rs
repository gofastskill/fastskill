//! Eval runner implementation using aikit-sdk

use crate::eval::artifacts::{CaseResult, CaseStatus};
use crate::eval::checks::{run_checks, CheckDefinition};
use crate::eval::suite::EvalCase;
use crate::eval::trace::{stdout_to_trace, trace_to_jsonl};
use aikit_sdk::{run_agent, RunOptions};
use std::path::PathBuf;
use thiserror::Error;

/// Options for running a single eval case
#[derive(Debug, Clone)]
pub struct CaseRunOptions {
    /// Agent key (e.g. "codex", "claude")
    pub agent_key: String,
    /// Optional model override
    pub model: Option<String>,
    /// Skill project root (used as working directory when no workspace_subdir)
    pub project_root: PathBuf,
    /// Timeout in seconds
    pub timeout_seconds: u64,
}

/// Raw output from running a case
#[derive(Debug)]
pub struct CaseRunOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

/// Errors during case execution
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("EVAL_AGENT_UNAVAILABLE: Agent '{0}' is not available")]
    AgentUnavailable(String),
    #[error("EVAL_CASE_TIMEOUT: Case timed out after {0}s")]
    Timeout(u64),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

/// Run a single eval case using aikit_sdk::run_agent via spawn_blocking.
///
/// This is the sole agent execution path per spec (acceptance criterion 29).
/// Uses spawn_blocking because run_agent is synchronous.
pub async fn run_eval_case(
    case: &EvalCase,
    opts: &CaseRunOptions,
    checks: &[CheckDefinition],
) -> (CaseRunOutput, CaseResult) {
    let agent_key = opts.agent_key.clone();
    let model = opts.model.clone();
    let prompt = case.prompt.clone();

    // Determine working directory
    let working_dir = match &case.workspace_subdir {
        Some(subdir) => opts.project_root.join(subdir),
        None => opts.project_root.clone(),
    };

    // Build RunOptions using available aikit-sdk API
    let run_opts = RunOptions {
        model: model.clone(),
        yolo: true,
        stream: false,
    };

    // Run agent in a blocking thread to avoid blocking the async runtime
    let result =
        tokio::task::spawn_blocking(move || run_agent(&agent_key, &prompt, run_opts)).await;

    let run_output = match result {
        Ok(Ok(run_result)) => {
            let exit_code = run_result.exit_code();
            CaseRunOutput {
                stdout: run_result.stdout,
                stderr: run_result.stderr,
                exit_code,
                timed_out: false,
            }
        }
        Ok(Err(e)) => CaseRunOutput {
            stdout: vec![],
            stderr: format!("Agent execution failed: {}", e).into_bytes(),
            exit_code: None,
            timed_out: false,
        },
        Err(e) => CaseRunOutput {
            stdout: vec![],
            stderr: format!("spawn_blocking failed: {}", e).into_bytes(),
            exit_code: None,
            timed_out: false,
        },
    };

    // Build trace from stdout
    let trace_events = stdout_to_trace(&run_output.stdout);
    let trace_jsonl = trace_to_jsonl(&trace_events);
    let stdout_str = String::from_utf8_lossy(&run_output.stdout).to_string();

    // Count raw_json events
    let command_count = trace_events
        .iter()
        .filter(|e| matches!(&e.payload, crate::eval::trace::TracePayload::RawJson { .. }))
        .count();

    // Run deterministic checks
    let check_results = run_checks(checks, &stdout_str, &trace_jsonl, &working_dir);
    let all_passed = check_results.iter().all(|r| r.passed);

    let status = if run_output.timed_out {
        CaseStatus::Error
    } else if checks.is_empty() {
        // No checks defined: pass if exit code is 0
        if run_output.exit_code == Some(0) {
            CaseStatus::Passed
        } else {
            CaseStatus::Failed
        }
    } else if all_passed {
        CaseStatus::Passed
    } else {
        CaseStatus::Failed
    };

    let case_result = CaseResult {
        id: case.id.clone(),
        status,
        command_count: Some(command_count),
        input_tokens: None,
        output_tokens: None,
        check_results,
        error_message: None,
    };

    (run_output, case_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_run_options_builder() {
        let opts = CaseRunOptions {
            agent_key: "codex".to_string(),
            model: Some("gpt-4".to_string()),
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 300,
        };
        assert_eq!(opts.agent_key, "codex");
        assert_eq!(opts.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_runner_error_display() {
        let err = RunnerError::AgentUnavailable("codex".to_string());
        assert!(err.to_string().contains("codex"));
        assert!(err.to_string().contains("EVAL_AGENT_UNAVAILABLE"));
    }
}

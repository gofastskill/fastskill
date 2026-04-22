//! Eval runner implementation using aikit-sdk

use crate::eval::artifacts::{CaseResult, CaseStatus, CaseTrialsResult, TrialResult};
use crate::eval::checks::{count_raw_json_events, run_checks, CheckDefinition};
use crate::eval::suite::EvalCase;
use crate::eval::trace::{agent_events_to_trace, trace_to_jsonl, TraceEvent, TracePayload};
use aikit_sdk::{run_agent_events, AgentEvent, RunOptions};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

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
    /// Per-case trial aggregation pass threshold (0.0-1.0)
    pub pass_threshold: f64,
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

/// Abstraction over eval case execution (default: aikit-backed).
#[async_trait]
pub trait EvalRunner: Send + Sync {
    /// Run one case, produce stdout/stderr capture, scored result, and canonical trace JSONL.
    async fn run_case(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
    ) -> (CaseRunOutput, CaseResult, String);

    /// Run multiple trials for one case, returning the aggregated result.
    async fn run_case_trials(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
        trial_count: u32,
        max_parallelism: Option<u32>,
    ) -> CaseTrialsResult;
}

/// Default runner: `aikit_sdk::run_agent_events` inside `spawn_blocking` with SDK timeout/cwd.
#[derive(Debug, Clone, Copy, Default)]
pub struct AikitEvalRunner;

/// Result of agent execution within spawn_blocking
struct AgentExecutionResult {
    result: Result<aikit_sdk::RunResult, aikit_sdk::RunError>,
    events: Vec<AgentEvent>,
}

#[async_trait]
impl EvalRunner for AikitEvalRunner {
    async fn run_case(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
    ) -> (CaseRunOutput, CaseResult, String) {
        self.run_case_inner(case, opts, checks).await
    }

    async fn run_case_trials(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
        trial_count: u32,
        max_parallelism: Option<u32>,
    ) -> CaseTrialsResult {
        let max_parallel = max_parallelism
            .unwrap_or_else(|| num_cpus::get().max(1) as u32)
            .max(1) as usize;
        let semaphore = Arc::new(Semaphore::new(max_parallel));
        let mut join_set: JoinSet<TrialResult> = JoinSet::new();

        for trial_id in 1..=trial_count {
            let permit = Arc::clone(&semaphore);
            let case_clone = case.clone();
            let opts_clone = opts.clone();
            let checks_vec = checks.to_vec();
            let runner = *self;

            join_set.spawn(async move {
                let Ok(_permit) = permit.acquire().await else {
                    return TrialResult {
                        trial_id,
                        status: CaseStatus::Error,
                        command_count: None,
                        input_tokens: None,
                        output_tokens: None,
                        check_results: vec![],
                        error_message: Some(
                            "EVAL_PARALLEL_EXHAUSTION: semaphore closed".to_string(),
                        ),
                    };
                };
                let (_output, case_result, _trace) = runner
                    .run_case_inner(&case_clone, &opts_clone, &checks_vec)
                    .await;
                TrialResult {
                    trial_id,
                    status: case_result.status,
                    command_count: case_result.command_count,
                    input_tokens: case_result.input_tokens,
                    output_tokens: case_result.output_tokens,
                    check_results: case_result.check_results,
                    error_message: case_result.error_message,
                }
            });
        }

        let mut trials = Vec::with_capacity(trial_count as usize);
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(trial) => trials.push(trial),
                Err(e) => {
                    // Join errors are treated as failed trials.
                    let next_id = (trials.len() as u32) + 1;
                    trials.push(TrialResult {
                        trial_id: next_id,
                        status: CaseStatus::Error,
                        command_count: None,
                        input_tokens: None,
                        output_tokens: None,
                        check_results: vec![],
                        error_message: Some(format!("EVAL_PARALLEL_EXHAUSTION: {}", e)),
                    });
                }
            }
        }

        trials.sort_by_key(|t| t.trial_id);
        let pass_count = trials
            .iter()
            .filter(|t| t.status == CaseStatus::Passed)
            .count() as u32;
        let total_trials = trial_count.max(1);
        let pass_rate = pass_count as f64 / total_trials as f64;
        let aggregated_status = if pass_rate >= opts.pass_threshold {
            CaseStatus::Passed
        } else {
            CaseStatus::Failed
        };

        CaseTrialsResult {
            id: case.id.clone(),
            trials,
            aggregated_status,
            pass_count,
            total_trials,
            pass_rate,
        }
    }
}

impl AikitEvalRunner {
    async fn run_case_inner(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
    ) -> (CaseRunOutput, CaseResult, String) {
        let agent_key = opts.agent_key.clone();
        let model = opts.model.clone();
        let prompt = case.prompt.clone();
        let timeout_secs = opts.timeout_seconds;

        let working_dir = match &case.workspace_subdir {
            Some(subdir) => opts.project_root.join(subdir),
            None => opts.project_root.clone(),
        };

        let mut run_opts = RunOptions::new()
            .with_yolo(true)
            .with_stream(true)
            .with_timeout(Duration::from_secs(timeout_secs))
            .with_current_dir(working_dir.clone());
        if let Some(model_name) = model {
            if !model_name.trim().is_empty() {
                run_opts = run_opts.with_model(model_name);
            }
        }

        let spawn_result = tokio::task::spawn_blocking(move || {
            let mut events: Vec<AgentEvent> = Vec::new();
            let result = run_agent_events(&agent_key, &prompt, run_opts, |ev| {
                events.push(ev.clone());
            });
            AgentExecutionResult { result, events }
        });

        let (run_output, trace_events) = match spawn_result.await {
            Ok(exec_result) => match exec_result.result {
                Ok(run_result) => {
                    let exit_code = run_result.exit_code();
                    let output = CaseRunOutput {
                        stdout: run_result.stdout,
                        stderr: run_result.stderr,
                        exit_code,
                        timed_out: false,
                    };
                    let trace = agent_events_to_trace(&exec_result.events);
                    (output, trace)
                }
                Err(aikit_sdk::RunError::TimedOut {
                    timeout, stderr, ..
                }) => {
                    let mut trace = agent_events_to_trace(&exec_result.events);
                    trace.push(TraceEvent {
                        seq: trace.len(),
                        payload: TracePayload::Timeout,
                    });
                    let output = CaseRunOutput {
                        stdout: vec![],
                        stderr,
                        exit_code: None,
                        timed_out: true,
                    };
                    if output.stderr.is_empty() {
                        let fallback = format!("Case timed out after {}s", timeout.as_secs());
                        let output = CaseRunOutput {
                            stdout: vec![],
                            stderr: fallback.into_bytes(),
                            exit_code: None,
                            timed_out: true,
                        };
                        (output, trace)
                    } else {
                        (output, trace)
                    }
                }
                Err(e) => {
                    let trace = agent_events_to_trace(&exec_result.events);
                    let output = CaseRunOutput {
                        stdout: vec![],
                        stderr: format!("Agent execution failed: {}", e).into_bytes(),
                        exit_code: None,
                        timed_out: false,
                    };
                    (output, trace)
                }
            },
            Err(e) => {
                let output = CaseRunOutput {
                    stdout: vec![],
                    stderr: format!("spawn_blocking failed: {}", e).into_bytes(),
                    exit_code: None,
                    timed_out: false,
                };
                (output, vec![])
            }
        };

        let trace_jsonl = trace_to_jsonl(&trace_events);
        let stdout_str = String::from_utf8_lossy(&run_output.stdout).to_string();
        let command_count = count_raw_json_events(&trace_jsonl);
        let check_results = run_checks(checks, &stdout_str, &trace_jsonl, &working_dir);
        let all_passed = check_results.iter().all(|r| r.passed);

        let status = if run_output.timed_out {
            CaseStatus::Error
        } else if checks.is_empty() {
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
            error_message: if run_output.timed_out {
                Some(format!(
                    "EVAL_CASE_TIMEOUT: Case timed out after {}s",
                    timeout_secs
                ))
            } else {
                None
            },
        };

        (run_output, case_result, trace_jsonl)
    }
}

/// Run a single eval case using the default [`AikitEvalRunner`].
///
/// Sole agent execution path per spec (acceptance criterion 29).
pub async fn run_eval_case(
    case: &EvalCase,
    opts: &CaseRunOptions,
    checks: &[CheckDefinition],
) -> (CaseRunOutput, CaseResult, String) {
    AikitEvalRunner.run_case(case, opts, checks).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::artifacts::CaseStatus;

    /// Stub runner for trait wiring tests (no aikit).
    struct StubEvalRunner;

    #[async_trait]
    impl EvalRunner for StubEvalRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            _opts: &CaseRunOptions,
            _checks: &[CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            let trace_jsonl =
                r#"{"seq":0,"payload":{"type":"raw_line","line":"stub"}}"#.to_string();
            let out = CaseRunOutput {
                stdout: b"ok".to_vec(),
                stderr: vec![],
                exit_code: Some(0),
                timed_out: false,
            };
            let result = CaseResult {
                id: case.id.clone(),
                status: CaseStatus::Passed,
                command_count: Some(0),
                input_tokens: None,
                output_tokens: None,
                check_results: vec![],
                error_message: None,
            };
            (out, result, trace_jsonl)
        }

        async fn run_case_trials(
            &self,
            case: &EvalCase,
            opts: &CaseRunOptions,
            checks: &[CheckDefinition],
            trial_count: u32,
            _max_parallelism: Option<u32>,
        ) -> CaseTrialsResult {
            let mut trials = Vec::new();
            for trial_id in 1..=trial_count {
                let (_out, result, _trace) = self.run_case(case, opts, checks).await;
                trials.push(TrialResult {
                    trial_id,
                    status: result.status,
                    command_count: result.command_count,
                    input_tokens: result.input_tokens,
                    output_tokens: result.output_tokens,
                    check_results: result.check_results,
                    error_message: result.error_message,
                });
            }
            let pass_count = trials
                .iter()
                .filter(|t| t.status == CaseStatus::Passed)
                .count() as u32;
            let total_trials = trial_count.max(1);
            let pass_rate = pass_count as f64 / total_trials as f64;
            let aggregated_status = if pass_rate >= opts.pass_threshold {
                CaseStatus::Passed
            } else {
                CaseStatus::Failed
            };
            CaseTrialsResult {
                id: case.id.clone(),
                trials,
                aggregated_status,
                pass_count,
                total_trials,
                pass_rate,
            }
        }
    }

    #[tokio::test]
    async fn test_eval_runner_trait_stub_returns_expected_trace() {
        let case = EvalCase {
            id: "c1".to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
        };
        let opts = CaseRunOptions {
            agent_key: "agent".to_string(),
            model: None,
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 1,
            pass_threshold: 1.0,
        };
        let runner = StubEvalRunner;
        let (out, res, trace) = runner.run_case(&case, &opts, &[]).await;
        assert_eq!(out.exit_code, Some(0));
        assert_eq!(res.id, "c1");
        assert!(trace.contains("raw_line"));
    }

    #[test]
    fn test_case_run_options_builder() {
        let opts = CaseRunOptions {
            agent_key: "codex".to_string(),
            model: Some("gpt-4".to_string()),
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 300,
            pass_threshold: 1.0,
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

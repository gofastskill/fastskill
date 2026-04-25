//! Standalone eval infrastructure for agent task evaluation.
//!
//! `evals-core` provides the building blocks for running agent-based evaluation
//! cases, scoring results, writing run artifacts, and loading check definitions.
//! It is designed to be consumed by skill-management tools (like `fastskill`)
//! and agent frameworks (like `aikit`) without pulling in HTTP server, database,
//! or skill-management dependencies.
//!
//! # Supported use cases
//! - Agent task evaluation: run a prompt against an agent and score the result
//! - Skills evaluation: validate that a skill triggers correctly given structured prompts
//!
//! # Re-exported items
//! All public types are available at the crate root:
//! [`EvalCase`], [`EvalSuite`], [`EvalRunner`], [`AikitEvalRunner`],
//! [`CaseRunOptions`], [`CaseRunOutput`], [`CaseResult`], [`CaseStatus`],
//! [`TrialResult`], [`CaseTrialsResult`], [`SummaryResult`], [`CheckDefinition`],
//! [`CheckResult`], [`EvalConfig`], [`EvalConfigInput`], [`TraceEvent`],
//! [`TracePayload`], [`RunnerError`], [`SuiteError`], [`EvalConfigError`],
//! [`ChecksError`], [`ArtifactsError`].

pub mod artifacts;
pub mod checks;
pub mod config;
pub mod runner;
pub mod suite;
pub mod trace;

pub use artifacts::{
    allocate_run_dir, read_case_results, read_summary, write_case_artifacts,
    write_case_trials_summary, write_summary, write_trial_artifacts, ArtifactsError, CaseResult,
    CaseStatus, CaseSummary, CaseTrialsResult, RunArtifacts, SummaryResult, TrialResult,
};
pub use checks::{
    count_raw_json_events, load_checks, run_checks, suite_passes, CheckDefinition, CheckResult,
    ChecksError, ChecksToml,
};
pub use config::{resolve_from_input, EvalConfig, EvalConfigError, EvalConfigInput};
pub use runner::{
    run_eval_case, AikitEvalRunner, CaseRunOptions, CaseRunOutput, EvalRunner, RunnerError,
};
pub use suite::{load_suite, EvalCase, EvalSuite, SuiteError};
pub use trace::{agent_events_to_trace, stdout_to_trace, trace_to_jsonl, TraceEvent, TracePayload};

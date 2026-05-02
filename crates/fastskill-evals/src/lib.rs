//! Eval runner and trace infrastructure for FastSkill.
//!
//! `fastskill-evals` owns all eval runner and trace conversion code that depends on
//! `aikit-sdk`. It is intentionally separate from `fastskill-core` so that consumers
//! needing only skill resolution do not pay the compile-time cost of the eval runner.
//!
//! # Crate boundaries
//! - `fastskill-evals` MAY depend on `fastskill-core` (for `SkillProjectToml`).
//! - `fastskill-core` MUST NOT depend on `fastskill-evals`.
//! - `fastskill-agent-runtime` MUST NOT depend on `fastskill-evals`.
//!
//! # Re-exported items
//! All public types are available at the crate root:
//! [`EvalCase`], [`EvalSuite`], [`EvalRunner`], [`AikitEvalRunner`],
//! [`CaseRunOptions`], [`CaseRunOutput`], [`CaseResult`], [`CaseStatus`],
//! [`TrialResult`], [`CaseTrialsResult`], [`SummaryResult`], [`CheckDefinition`],
//! [`CheckResult`], [`EvalConfig`], [`EvalConfigInput`], [`TraceEvent`],
//! [`TracePayload`], [`RunnerError`], [`SuiteError`], [`EvalConfigError`],
//! [`ChecksError`], [`ArtifactsError`], [`resolve_eval_config`].

pub mod artifacts;
pub mod checks;
pub mod config;
pub mod config_adapter;
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
pub use config_adapter::resolve_eval_config;
pub use runner::{
    run_eval_case, AikitEvalRunner, CaseRunOptions, CaseRunOutput, EvalRunner, RunnerError,
};
pub use suite::{load_suite, EvalCase, EvalSuite, SuiteError};
pub use trace::{agent_events_to_trace, stdout_to_trace, trace_to_jsonl, TraceEvent, TracePayload};

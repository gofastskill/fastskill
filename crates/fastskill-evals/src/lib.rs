//! Thin adapter: re-exports aikit-evals eval infrastructure and adds
//! fastskill-specific config resolution from skill-project.toml.
//!
//! # Crate boundaries
//! - `fastskill-evals` MAY depend on `fastskill-core` (for `SkillProjectToml`).
//! - `fastskill-core` MUST NOT depend on `fastskill-evals`.
//! - `fastskill-agent-runtime` MUST NOT depend on `fastskill-evals`.

pub mod config_adapter;

// Re-export submodules so that path-based callers (fastskill-cli) continue to work unchanged.
pub use aikit_evals::artifacts;
pub use aikit_evals::checks;
pub use aikit_evals::config;
pub use aikit_evals::runner;
pub use aikit_evals::suite;
pub use aikit_evals::trace;

// Re-export all public items at the crate root for convenience.
pub use aikit_evals::{
    agent_events_to_trace, allocate_run_dir, count_raw_json_events, load_checks, load_suite,
    read_case_results, read_summary, resolve_from_input, run_checks, run_eval_case,
    stdout_to_trace, suite_passes, trace_to_jsonl, write_case_artifacts, write_case_trials_summary,
    write_summary, write_trial_artifacts, AikitEvalRunner, ArtifactsError, CaseResult,
    CaseRunOptions, CaseRunOutput, CaseStatus, CaseSummary, CaseTrialsResult, CheckDefinition,
    CheckResult, ChecksError, ChecksToml, EvalCase, EvalConfig, EvalConfigError, EvalConfigInput,
    EvalRunner, EvalSuite, RunArtifacts, RunnerError, SuiteError, SummaryResult, TraceEvent,
    TracePayload, TrialResult,
};
pub use config_adapter::resolve_eval_config;

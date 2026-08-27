//! Eval run subcommand - case execution orchestration

use crate::commands::common::{runtime_selection_error_to_cli, validate_eval_format_args};
use crate::error::{CliError, CliResult};
use crate::runtime_selector::RuntimeSelectionInput;
use aikit_sdk::is_agent_available;
use chrono::Utc;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::manifest::SkillProjectToml;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::{
    allocate_run_dir, write_case_trials_summary, write_summary, write_trial_artifacts, CaseStatus,
    CaseSummary, CaseTrialsResult, IsolationReport, ScopeFidelity, SummaryResult, TrialResult,
};
use fastskill_evals::checks::load_checks;
use fastskill_evals::resolve_eval_config;
use fastskill_evals::runner::{
    AikitEvalRunner, CaseRunOptions, EvalRunner, IsolationMode, SkillSource,
};
use fastskill_evals::suite::load_suite;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Arguments for `fastskill eval run`
#[derive(Debug)]
pub struct RunArgs {
    /// Target runtime(s) for this operation; repeatable (mutually exclusive with --all)
    pub agent: Vec<String>,

    /// Target all runtimes discovered by aikit (mutually exclusive with --agent)
    pub all: bool,

    /// Output directory for artifacts (required)
    pub output_dir: PathBuf,

    /// Optional model override forwarded to the agent
    pub model: Option<String>,

    /// Filter: run only the case with this ID
    pub case: Option<String>,

    /// Filter: run only cases with this tag
    pub tag: Option<String>,

    /// Output format: table, json (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    pub no_fail: bool,

    /// Override trials per case from config.
    ///
    /// Stored as the raw parsed integer (not clamped to `u32`) so the
    /// execute-time range check can echo exactly what the user typed — e.g. a
    /// negative `-3` is reported as `-3`, not a wrapped `u32::MAX`.
    pub trials: Option<i64>,

    /// Enable CI mode: exit non-zero if suite pass rate below threshold
    pub ci: bool,

    /// Override pass threshold (0.0-1.0)
    pub threshold: Option<f64>,

    /// Disable environment isolation: run cases in the project root against
    /// the ambient agent environment (legacy behaviour). Scores are then
    /// influenced by whatever skills are installed on this machine.
    pub no_isolation: bool,
}

fn parse_output_format(s: &str) -> Option<fastskill_core::OutputFormat> {
    match s {
        "table" => Some(fastskill_core::OutputFormat::Table),
        "json" => Some(fastskill_core::OutputFormat::Json),
        "grid" => Some(fastskill_core::OutputFormat::Grid),
        "xml" => Some(fastskill_core::OutputFormat::Xml),
        _ => None,
    }
}

impl IntoCommandSpec for RunArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Run eval cases against an agent",
            syntax: Some("eval run [OPTIONS]"),
            category: Some("quality"),
            examples: vec![
                "fastskill eval run --agent claude --output-dir ./eval-runs",
                "fastskill eval run --all --output-dir ./eval-runs --ci --threshold 0.9",
            ],
            args: vec![
                ArgSpec {
                    name: "agent",
                    kind: ArgKind::Option,
                    short: Some('a'),
                    long: Some("agent"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Target runtime(s); repeatable (mutually exclusive with --all)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "all",
                    kind: ArgKind::Flag,
                    long: Some("all"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Target all runtimes (mutually exclusive with --agent)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "output-dir",
                    kind: ArgKind::Option,
                    long: Some("output-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Output directory for artifacts (required)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "model",
                    kind: ArgKind::Option,
                    long: Some("model"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Optional model override forwarded to the agent",
                    ..Default::default()
                },
                ArgSpec {
                    name: "case",
                    kind: ArgKind::Option,
                    long: Some("case"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Filter: run only the case with this ID",
                    ..Default::default()
                },
                ArgSpec {
                    name: "tag",
                    kind: ArgKind::Option,
                    long: Some("tag"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Filter: run only cases with this tag",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Shorthand for --format json",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-fail",
                    kind: ArgKind::Flag,
                    long: Some("no-fail"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Do not fail with non-zero exit code on suite failure",
                    ..Default::default()
                },
                ArgSpec {
                    name: "trials",
                    kind: ArgKind::Option,
                    long: Some("trials"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Override trials per case from config (1-1000)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "ci",
                    kind: ArgKind::Flag,
                    long: Some("ci"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "CI mode: pass/fail on suite pass rate vs threshold",
                    ..Default::default()
                },
                ArgSpec {
                    name: "threshold",
                    kind: ArgKind::Option,
                    long: Some("threshold"),
                    value_type: ArgValueType::Float,
                    cardinality: Cardinality::Optional,
                    help: "Override pass threshold (0.0-1.0)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-isolation",
                    kind: ArgKind::Flag,
                    long: Some("no-isolation"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Run in the project root against the ambient agent environment \
                           (disables per-case scratch-workspace isolation)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for RunArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        RunArgs {
            agent: match map.get("agent") {
                Some(ArgValue::List(items)) => items
                    .iter()
                    .filter_map(|i| {
                        if let ArgValue::Str(s) = i {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => vec![],
            },
            all: matches!(map.get("all"), Some(ArgValue::Bool(true))),
            output_dir: map
                .get("output-dir")
                .map(|v| {
                    if let ArgValue::Str(s) = v {
                        PathBuf::from(s)
                    } else {
                        panic!("fw bug")
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing output-dir")),
            model: map.get("model").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            case: map.get("case").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            tag: map.get("tag").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            no_fail: matches!(map.get("no-fail"), Some(ArgValue::Bool(true))),
            trials: map.get("trials").and_then(|v| {
                if let ArgValue::Int(i) = v {
                    // Carry the raw value through unchanged; the execute-time
                    // range check validates [1, 1000] and echoes the honest
                    // value the user typed on failure.
                    Some(*i)
                } else {
                    None
                }
            }),
            ci: matches!(map.get("ci"), Some(ArgValue::Bool(true))),
            threshold: map.get("threshold").and_then(|v| match v {
                ArgValue::Float(f) => Some(*f),
                ArgValue::Int(i) => Some(*i as f64),
                _ => None,
            }),
            no_isolation: matches!(map.get("no-isolation"), Some(ArgValue::Bool(true))),
        }
    }
}

impl From<&RunArgs> for RuntimeSelectionInput {
    fn from(args: &RunArgs) -> Self {
        RuntimeSelectionInput {
            agents: args.agent.clone(),
            all: args.all,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::items_after_test_module)]
mod tests {
    use super::*;
    use cli_framework::spec::arg_spec::{ArgKind, ArgValueType};

    fn base_map() -> HashMap<String, ArgValue> {
        let mut m = HashMap::new();
        // Value is incidental: it's parsed into a PathBuf field but no test in
        // this module performs filesystem I/O on it, so a platform-neutral
        // relative path is fine (avoids a Unix-only absolute path).
        m.insert(
            "output-dir".to_string(),
            ArgValue::Str("eval-out".to_string()),
        );
        m
    }

    /// The four flags must be registered in the command spec, otherwise the
    /// dispatcher never populates them and `from_arg_value_map` reads defaults.
    #[test]
    fn test_command_spec_registers_ci_flags() {
        let spec = RunArgs::command_spec();
        let by_name = |name: &str| spec.args.iter().find(|a| a.name == name).cloned();

        let no_fail = by_name("no-fail").expect("--no-fail registered");
        assert_eq!(no_fail.kind, ArgKind::Flag);

        let ci = by_name("ci").expect("--ci registered");
        assert_eq!(ci.kind, ArgKind::Flag);

        let trials = by_name("trials").expect("--trials registered");
        assert_eq!(trials.kind, ArgKind::Option);
        assert_eq!(trials.value_type, ArgValueType::Int);

        let threshold = by_name("threshold").expect("--threshold registered");
        assert_eq!(threshold.kind, ArgKind::Option);
        assert_eq!(threshold.value_type, ArgValueType::Float);
    }

    #[test]
    fn test_flags_default_when_absent() {
        let args = RunArgs::from_arg_value_map(&base_map());
        assert!(!args.no_fail);
        assert!(!args.ci);
        assert_eq!(args.trials, None);
        assert_eq!(args.threshold, None);
    }

    #[test]
    fn test_flags_parsed_from_map() {
        let mut m = base_map();
        m.insert("no-fail".to_string(), ArgValue::Bool(true));
        m.insert("ci".to_string(), ArgValue::Bool(true));
        m.insert("trials".to_string(), ArgValue::Int(7));
        m.insert("threshold".to_string(), ArgValue::Float(0.75));

        let args = RunArgs::from_arg_value_map(&m);
        assert!(args.no_fail);
        assert!(args.ci);
        assert_eq!(args.trials, Some(7));
        assert_eq!(args.threshold, Some(0.75));
    }

    #[test]
    fn test_threshold_accepts_integer_value() {
        let mut m = base_map();
        m.insert("threshold".to_string(), ArgValue::Int(1));
        let args = RunArgs::from_arg_value_map(&m);
        assert_eq!(args.threshold, Some(1.0));
    }

    #[test]
    fn test_negative_trials_preserved_for_honest_range_error() {
        let mut m = base_map();
        m.insert("trials".to_string(), ArgValue::Int(-3));
        let args = RunArgs::from_arg_value_map(&m);
        // The raw value is carried through unchanged so the execute-time
        // [1, 1000] range check can echo the honest value the user typed
        // (`-3`) rather than a wrapped `u32::MAX`.
        assert_eq!(args.trials, Some(-3));
    }

    /// spec 016: --no-isolation must be registered as a flag, default to
    /// false (isolated is the default), and parse when present.
    #[test]
    fn test_no_isolation_flag_registered_and_parsed() {
        let spec = RunArgs::command_spec();
        let flag = spec
            .args
            .iter()
            .find(|a| a.name == "no-isolation")
            .expect("--no-isolation registered");
        assert_eq!(flag.kind, ArgKind::Flag);

        let args = RunArgs::from_arg_value_map(&base_map());
        assert!(!args.no_isolation, "isolation must be the default");

        let mut m = base_map();
        m.insert("no-isolation".to_string(), ArgValue::Bool(true));
        let args = RunArgs::from_arg_value_map(&m);
        assert!(args.no_isolation);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod isolation_mode_tests {
    use super::*;
    use tempfile::TempDir;

    fn write_project(dir: &std::path::Path, with_id: bool, with_skill_md: bool) -> PathBuf {
        let project_file = dir.join("skill-project.toml");
        let metadata = if with_id {
            "[metadata]\nid = \"greeting-helper\"\nversion = \"1.0.0\"\n\n"
        } else {
            ""
        };
        std::fs::write(
            &project_file,
            format!("{metadata}[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 60\nfail_on_missing_agent = false\n"),
        )
        .unwrap();
        if with_skill_md {
            std::fs::write(
                dir.join("SKILL.md"),
                "---\nname: greeting-helper\n---\nbody\n",
            )
            .unwrap();
        }
        project_file
    }

    #[test]
    fn test_default_resolves_isolated_with_skill_identity() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), true, true);

        let mode = resolve_isolation_mode(false, &project_file, dir.path()).unwrap();
        match mode {
            IsolationMode::Isolated { skill_name, source } => {
                assert_eq!(skill_name, "greeting-helper");
                assert_eq!(source, SkillSource::Dir(dir.path().to_path_buf()));
            }
            IsolationMode::Inherit => panic!("default must be Isolated, got Inherit"),
        }
    }

    #[test]
    fn test_no_isolation_resolves_inherit_without_needing_skill_md() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), false, false);

        let mode = resolve_isolation_mode(true, &project_file, dir.path()).unwrap();
        assert_eq!(mode, IsolationMode::Inherit);
    }

    #[test]
    fn test_missing_skill_md_is_loud_error_not_silent_inherit() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), true, false);

        let err = resolve_isolation_mode(false, &project_file, dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("EVAL_ISOLATION_NO_SKILL"),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("--no-isolation"));
    }

    #[test]
    fn test_missing_metadata_id_is_loud_error() {
        let dir = TempDir::new().unwrap();
        let project_file = write_project(dir.path(), false, true);

        let err = resolve_isolation_mode(false, &project_file, dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("EVAL_ISOLATION_NO_SKILL"),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("[metadata].id"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod isolation_render_tests {
    use super::*;

    fn report(requested: bool) -> IsolationReport {
        IsolationReport {
            requested,
            project_scope: ScopeFidelity::Isolated,
            user_scope: ScopeFidelity::Isolated,
            mechanism: Some("--setting-sources project".to_string()),
            agent_version: None,
            ambient_skills: vec![],
            workspace_root: None,
            degrade_reason: None,
        }
    }

    #[test]
    fn test_render_no_report_is_unknown_never_a_claim() {
        let line = render_isolation_line(None);
        assert!(line.contains("unknown"), "got: {line}");
        assert!(
            !line.contains("isolated,"),
            "must not claim fidelity: {line}"
        );
    }

    #[test]
    fn test_render_isolated_reports_scopes_and_mechanism() {
        let line = render_isolation_line(Some(&report(true)));
        assert!(line.contains("project scope isolated"), "got: {line}");
        assert!(line.contains("user scope isolated"), "got: {line}");
        assert!(line.contains("--setting-sources project"), "got: {line}");
    }

    #[test]
    fn test_render_not_requested_says_off_and_warns() {
        let line = render_isolation_line(Some(&report(false)));
        assert!(line.contains("off"), "got: {line}");
        assert!(line.contains("ambient"), "got: {line}");
    }

    #[test]
    fn test_render_degraded_reason_is_surfaced() {
        let mut r = report(true);
        r.user_scope = ScopeFidelity::Inherited;
        r.degrade_reason = Some("opencode has no skills path".to_string());
        let line = render_isolation_line(Some(&r));
        assert!(line.contains("degraded"), "got: {line}");
        assert!(line.contains("opencode has no skills path"), "got: {line}");
        assert!(line.contains("user scope inherited"), "got: {line}");
    }
}

/// End-to-end locks on the isolation wiring: what `execute_run_with_runner`
/// actually hands the runner, and what lands in summary.json.
///
/// These tests `set_current_dir` into a temp project (the project file is
/// resolved from the cwd; there is no injection seam). That is process-global
/// state — safe under nextest's process-per-test model, which is what CI runs.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod isolation_e2e_tests {
    use super::*;
    use fastskill_evals::artifacts::CaseResult;
    use fastskill_evals::runner::CaseRunOutput;
    use fastskill_evals::suite::EvalCase;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Captures every `CaseRunOptions` it is called with and reports a
    /// passing case plus a canned isolation report.
    struct CapturingRunner {
        seen: Mutex<Vec<CaseRunOptions>>,
    }

    #[async_trait::async_trait]
    impl EvalRunner for CapturingRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            opts: &CaseRunOptions,
            _checks: &[fastskill_evals::checks::CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            self.seen.lock().unwrap().push(opts.clone());
            let requested = matches!(opts.isolation, IsolationMode::Isolated { .. });
            (
                CaseRunOutput {
                    stdout: b"ok".to_vec(),
                    stderr: vec![],
                    exit_code: Some(0),
                    timed_out: false,
                    workspace: None,
                    isolation: Some(IsolationReport {
                        requested,
                        project_scope: ScopeFidelity::Isolated,
                        user_scope: ScopeFidelity::Isolated,
                        mechanism: Some("fake-mechanism".to_string()),
                        agent_version: None,
                        ambient_skills: vec![],
                        workspace_root: None,
                        degrade_reason: None,
                    }),
                },
                CaseResult {
                    id: case.id.clone(),
                    status: CaseStatus::Passed,
                    command_count: None,
                    input_tokens: None,
                    output_tokens: None,
                    check_results: vec![],
                    error_message: None,
                },
                String::new(),
            )
        }

        async fn run_case_trials(
            &self,
            _case: &EvalCase,
            _opts: &CaseRunOptions,
            _checks: &[fastskill_evals::checks::CheckDefinition],
            _trial_count: u32,
            _max_parallelism: Option<u32>,
        ) -> CaseTrialsResult {
            unreachable!("execute_run_with_runner drives run_case directly")
        }
    }

    fn scaffold_project(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join("evals")).unwrap();
        std::fs::write(
            dir.join("evals/prompts.csv"),
            "id,prompt,should_trigger\ncase-1,say hello,true\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("skill-project.toml"),
            "[metadata]\nid = \"greeting-helper\"\nversion = \"1.0.0\"\n\n\
             [tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\n\
             timeout_seconds = 60\nfail_on_missing_agent = false\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: greeting-helper\n---\nbody\n",
        )
        .unwrap();
    }

    fn run_args(output_dir: PathBuf, no_isolation: bool) -> RunArgs {
        RunArgs {
            agent: vec!["aikit".to_string()],
            all: false,
            output_dir,
            model: None,
            case: None,
            tag: None,
            format: None,
            json: false,
            no_fail: false,
            trials: None,
            ci: false,
            threshold: None,
            no_isolation,
        }
    }

    async fn run_and_capture(no_isolation: bool) -> (Vec<CaseRunOptions>, SummaryResult) {
        let project = TempDir::new().unwrap();
        scaffold_project(project.path());
        std::env::set_current_dir(project.path()).unwrap();

        let runner = Arc::new(CapturingRunner {
            seen: Mutex::new(vec![]),
        });
        let out_dir = project.path().join("eval-out");
        execute_run_with_runner(run_args(out_dir.clone(), no_isolation), Arc::clone(&runner))
            .await
            .unwrap();

        let seen = runner.seen.lock().unwrap().clone();

        // Exactly one run dir with one agent subdir was allocated.
        let run_dir = std::fs::read_dir(&out_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("aikit");
        let summary: SummaryResult =
            serde_json::from_str(&std::fs::read_to_string(run_dir.join("summary.json")).unwrap())
                .unwrap();
        (seen, summary)
    }

    #[tokio::test]
    async fn test_default_run_hands_runner_isolated_mode_with_retention() {
        let (seen, summary) = run_and_capture(false).await;

        assert!(!seen.is_empty());
        for opts in &seen {
            match &opts.isolation {
                IsolationMode::Isolated { skill_name, source } => {
                    assert_eq!(skill_name, "greeting-helper");
                    assert!(matches!(source, SkillSource::Dir(_)));
                }
                IsolationMode::Inherit => {
                    panic!("default eval run must isolate, runner got Inherit")
                }
            }
            let retain = opts
                .retain_workspace_in
                .as_ref()
                .expect("failed-workspace retention dir must be set");
            assert!(retain.ends_with("workspaces"), "got: {}", retain.display());
        }

        let iso = summary
            .isolation
            .expect("summary.json must carry the isolation report");
        assert!(iso.requested);
        assert_eq!(iso.mechanism.as_deref(), Some("fake-mechanism"));
    }

    #[tokio::test]
    async fn test_no_isolation_hands_runner_inherit() {
        let (seen, summary) = run_and_capture(true).await;

        assert!(!seen.is_empty());
        for opts in &seen {
            assert_eq!(
                opts.isolation,
                IsolationMode::Inherit,
                "--no-isolation must reach the runner as Inherit"
            );
        }
        let iso = summary.isolation.expect("report still recorded");
        assert!(!iso.requested);
    }
}

/// Execute the `eval run` command using the default aikit-backed runner.
pub async fn execute_run(args: RunArgs) -> CliResult<()> {
    execute_run_with_runner(args, Arc::new(AikitEvalRunner::new())).await
}

/// One-line human rendering of the run's isolation contract (spec 016 D6).
///
/// `None` means the runner reported nothing — rendered as "unknown", never as
/// a claim in either direction.
fn render_isolation_line(iso: Option<&IsolationReport>) -> String {
    let Some(iso) = iso else {
        return "isolation: unknown (no report from runner)".to_string();
    };
    let scope = |s: &ScopeFidelity| match s {
        ScopeFidelity::Isolated => "isolated",
        ScopeFidelity::Inherited => "inherited",
        ScopeFidelity::Unsupported => "unsupported",
    };
    let mut line = if iso.requested {
        format!(
            "isolation: project scope {}, user scope {}",
            scope(&iso.project_scope),
            scope(&iso.user_scope)
        )
    } else {
        "isolation: off (--no-isolation) — scores reflect this machine's ambient skills".to_string()
    };
    if let Some(mechanism) = &iso.mechanism {
        line.push_str(&format!(" (via {})", mechanism));
    }
    if let Some(reason) = &iso.degrade_reason {
        line.push_str(&format!(" — degraded: {}", reason));
    }
    line
}

/// Resolve the isolation mode for this run (spec 016).
///
/// Default is `Isolated`: the skill next to `skill-project.toml` is deployed
/// alone into a per-case scratch workspace, so scores measure the skill under
/// test instead of whatever else is installed on this machine. The skill
/// identity comes from the project itself — `SKILL.md` beside the project
/// file, named by `[metadata].id`. Both are required for isolation and their
/// absence is a loud config error, not a silent fallback to the ambient
/// environment.
fn resolve_isolation_mode(
    no_isolation: bool,
    project_file: &std::path::Path,
    project_root: &std::path::Path,
) -> CliResult<IsolationMode> {
    if no_isolation {
        return Ok(IsolationMode::Inherit);
    }

    let skill_md = project_root.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(CliError::Config(format!(
            "EVAL_ISOLATION_NO_SKILL: eval runs isolated by default, which needs a SKILL.md \
             next to '{}' to deploy as the skill under test. Add one, or pass --no-isolation \
             to run against the ambient agent environment.",
            project_file.display()
        )));
    }

    let content = std::fs::read_to_string(project_file).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ISOLATION_NO_SKILL: cannot read '{}': {}",
            project_file.display(),
            e
        ))
    })?;
    let toml: SkillProjectToml = toml::from_str(&content)
        .map_err(|e| CliError::Config(format!("EVAL_CONFIG_INVALID: {}", e)))?;
    let skill_name = toml
        .metadata
        .as_ref()
        .and_then(|m| m.id.clone())
        .ok_or_else(|| {
            CliError::Config(format!(
                "EVAL_ISOLATION_NO_SKILL: isolation needs [metadata].id in '{}' to name the \
                 skill under test. Add it, or pass --no-isolation.",
                project_file.display()
            ))
        })?;

    Ok(IsolationMode::Isolated {
        skill_name,
        source: SkillSource::Dir(project_root.to_path_buf()),
    })
}

/// Execute `eval run` with an injectable [`EvalRunner`] (tests or future adapters).
pub async fn execute_run_with_runner<R: EvalRunner + 'static>(
    args: RunArgs,
    runner: Arc<R>,
) -> CliResult<()> {
    let format = validate_eval_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    // Resolve runtime selection first so missing --agent is caught before project-file checks.
    let input = RuntimeSelectionInput::from(&args);
    let selection = crate::runtime_selector::resolve_runtime_selection(&input)
        .map_err(runtime_selection_error_to_cli)?;

    let runtimes = match selection {
        Some(sel) => sel.runtimes,
        None => {
            return Err(CliError::Config(
                "RUNTIME_NO_SELECTION: No runtime selected. Use --agent <id> or --all to \
                 specify a target runtime."
                    .to_string(),
            ));
        }
    };

    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    let resolution = resolve_project_file(&current_dir);
    if !resolution.found {
        return Err(CliError::Config(
            "EVAL_CONFIG_MISSING: No skill-project.toml found. Run 'fastskill init' first."
                .to_string(),
        ));
    }

    let project_root = resolution
        .path
        .parent()
        .unwrap_or(&current_dir)
        .to_path_buf();

    let eval_config = resolve_eval_config(&resolution.path, &project_root)
        .map_err(|e| CliError::Config(e.to_string()))?;

    let isolation = resolve_isolation_mode(args.no_isolation, &resolution.path, &project_root)?;

    // Validate against the raw parsed value so the error echoes exactly what the
    // user typed (e.g. a negative `-3`), not a wrapped/clamped integer.
    let trials_raw = args
        .trials
        .unwrap_or(i64::from(eval_config.trials_per_case));
    if !(1..=1000).contains(&trials_raw) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_TRIALS_CONFIG: trials must be in range [1, 1000], got {}",
            trials_raw
        )));
    }
    // Safe: validated to be within [1, 1000] above.
    let trials_per_case = trials_raw as u32;

    let pass_threshold = args.threshold.unwrap_or(eval_config.pass_threshold);
    if !(0.0..=1.0).contains(&pass_threshold) {
        return Err(CliError::Config(format!(
            "EVAL_INVALID_THRESHOLD: threshold must be in range [0.0, 1.0], got {}",
            pass_threshold
        )));
    }

    // Load suite and apply filters (same for all runtimes).
    let mut suite =
        load_suite(&eval_config.prompts_path).map_err(|e| CliError::Config(e.to_string()))?;

    // Reject a suite that parsed to zero cases before any filtering (the
    // --case/--tag filters below already guard their own empty results). The
    // default verdict is `failed == 0`, so an empty suite — a header-only CSV
    // or a wrong prompts path pointing at a template — would otherwise report
    // `0/0 passed · PASSED` and exit 0, green-lighting CI while running
    // nothing at all.
    if suite.cases.is_empty() {
        return Err(CliError::Config(format!(
            "EVAL_EMPTY_SUITE: suite '{}' contains zero cases",
            eval_config.prompts_path.display()
        )));
    }

    if let Some(ref case_id) = args.case {
        suite = suite.filter_by_id(case_id);
        if suite.cases.is_empty() {
            return Err(CliError::Config(format!(
                "No case found with id '{}'",
                case_id
            )));
        }
    }
    if let Some(ref tag) = args.tag {
        suite = suite.filter_by_tag(tag);
        if suite.cases.is_empty() {
            return Err(CliError::Config(format!(
                "No cases found with tag '{}'",
                tag
            )));
        }
    }

    // Load checks if configured.
    let checks = if let Some(ref checks_path) = eval_config.checks_path {
        load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?
    } else {
        vec![]
    };

    let total_trial_runs =
        (suite.cases.len() as u64) * (trials_per_case as u64) * (runtimes.len() as u64);
    if total_trial_runs >= 100 && !use_json {
        eprintln!(
            "warning: EVAL_COST_WARNING: running {} case(s) × {} trial(s) × {} agent(s) = {} total trial runs",
            suite.cases.len(),
            trials_per_case,
            runtimes.len(),
            total_trial_runs
        );
    }

    // Allocate run base directory.
    let run_id = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    std::fs::create_dir_all(&args.output_dir).map_err(|e| {
        CliError::Config(format!(
            "Failed to create output directory '{}': {}",
            args.output_dir.display(),
            e
        ))
    })?;
    let run_dir_base =
        allocate_run_dir(&args.output_dir, &run_id).map_err(|e| CliError::Config(e.to_string()))?;

    let mut all_summaries: Vec<SummaryResult> = Vec::new();
    let mut any_agent_failed = false;

    for agent_key in &runtimes {
        // Per-agent subdirectory.
        let run_dir = run_dir_base.join(agent_key);
        std::fs::create_dir_all(&run_dir).map_err(|e| {
            CliError::Config(format!(
                "Failed to create run directory '{}': {}",
                run_dir.display(),
                e
            ))
        })?;

        // Check agent availability.
        if eval_config.fail_on_missing_agent && !is_agent_available(agent_key) {
            return Err(CliError::Config(format!(
                "EVAL_AGENT_UNAVAILABLE: Agent '{}' is not available. Install it first.",
                agent_key
            )));
        }

        let run_opts = CaseRunOptions {
            agent_key: agent_key.clone(),
            model: args.model.clone(),
            project_root: project_root.clone(),
            timeout_seconds: eval_config.timeout_seconds,
            pass_threshold,
            isolation: isolation.clone(),
            // A failed case's scratch workspace is moved here so it survives
            // for debugging; successful workspaces are deleted.
            retain_workspace_in: Some(run_dir.join("workspaces")),
        };

        if !use_json {
            eprintln!(
                "Running {} eval case(s) with agent '{}' ({} trial(s) per case)...",
                suite.cases.len(),
                agent_key,
                trials_per_case
            );
        }

        let mut case_summaries = Vec::new();
        // First observed per-case isolation report stands in for the run: the
        // backend and mechanism are constant across an agent's run, and a
        // per-case copy lives in each trial's artifacts.
        let mut run_isolation: Option<IsolationReport> = None;

        for case in &suite.cases {
            if !use_json {
                eprintln!("  Running case '{}'...", case.id);
            }

            let max_parallel = eval_config
                .parallel
                .unwrap_or_else(|| num_cpus::get().max(1) as u32)
                .max(1) as usize;
            let semaphore = Arc::new(Semaphore::new(max_parallel));
            let mut join_set: JoinSet<
                CliResult<(
                    u32,
                    fastskill_evals::runner::CaseRunOutput,
                    fastskill_evals::artifacts::CaseResult,
                    String,
                )>,
            > = JoinSet::new();

            for trial_id in 1..=trials_per_case {
                let permit = Arc::clone(&semaphore);
                let runner = Arc::clone(&runner);
                let case_clone = case.clone();
                let opts_clone = run_opts.clone();
                let checks_vec = checks.clone();

                join_set.spawn(async move {
                    let Ok(_permit) = permit.acquire().await else {
                        return Err(CliError::Config(
                            "EVAL_PARALLEL_EXHAUSTION: semaphore closed".to_string(),
                        ));
                    };
                    let (out, res, trace) =
                        runner.run_case(&case_clone, &opts_clone, &checks_vec).await;
                    Ok((trial_id, out, res, trace))
                });
            }

            let mut trials: Vec<TrialResult> = Vec::with_capacity(trials_per_case as usize);
            let mut pass_count: u32 = 0;
            let mut command_count_sum: usize = 0;
            let mut input_tokens_sum: u64 = 0;
            let mut output_tokens_sum: u64 = 0;
            let mut saw_any_command_count = false;
            let mut saw_any_input_tokens = false;
            let mut saw_any_output_tokens = false;

            while let Some(joined) = join_set.join_next().await {
                let (trial_id, out, case_result, trace_jsonl) = joined.map_err(|e| {
                    CliError::Config(format!(
                        "EVAL_PARALLEL_EXHAUSTION: trial task failed: {}",
                        e
                    ))
                })??;

                if run_isolation.is_none() {
                    run_isolation = out.isolation.clone();
                }

                let trial = TrialResult {
                    trial_id,
                    status: case_result.status.clone(),
                    command_count: case_result.command_count,
                    input_tokens: case_result.input_tokens,
                    output_tokens: case_result.output_tokens,
                    check_results: case_result.check_results.clone(),
                    error_message: case_result.error_message.clone(),
                };

                if trial.status == CaseStatus::Passed {
                    pass_count += 1;
                }
                if let Some(cc) = trial.command_count {
                    saw_any_command_count = true;
                    command_count_sum = command_count_sum.saturating_add(cc);
                }
                if let Some(it) = trial.input_tokens {
                    saw_any_input_tokens = true;
                    input_tokens_sum = input_tokens_sum.saturating_add(it);
                }
                if let Some(ot) = trial.output_tokens {
                    saw_any_output_tokens = true;
                    output_tokens_sum = output_tokens_sum.saturating_add(ot);
                }

                if let Err(e) = write_trial_artifacts(
                    &run_dir,
                    &case.id,
                    trial_id,
                    &out.stdout,
                    &out.stderr,
                    &trace_jsonl,
                    &trial,
                ) {
                    if !use_json {
                        eprintln!(
                            "  warning: failed to write artifacts for case '{}' trial {}: {}",
                            case.id, trial_id, e
                        );
                    }
                }

                trials.push(trial);
            }

            trials.sort_by_key(|t| t.trial_id);
            let total_trials = trials_per_case;
            let pass_rate = pass_count as f64 / total_trials as f64;
            let aggregated_status = if pass_rate >= pass_threshold {
                CaseStatus::Passed
            } else {
                CaseStatus::Failed
            };

            let aggregated = CaseTrialsResult {
                id: case.id.clone(),
                trials: trials.clone(),
                aggregated_status: aggregated_status.clone(),
                pass_count,
                total_trials,
                pass_rate,
            };

            if let Err(e) = write_case_trials_summary(&run_dir, &case.id, &aggregated) {
                if !use_json {
                    eprintln!(
                        "  warning: failed to write aggregated summary for case '{}': {}",
                        case.id, e
                    );
                }
            }

            case_summaries.push(CaseSummary {
                id: case.id.clone(),
                status: aggregated_status,
                command_count: if saw_any_command_count {
                    Some(command_count_sum)
                } else {
                    None
                },
                input_tokens: if saw_any_input_tokens {
                    Some(input_tokens_sum)
                } else {
                    None
                },
                output_tokens: if saw_any_output_tokens {
                    Some(output_tokens_sum)
                } else {
                    None
                },
                pass_count: Some(pass_count),
                total_trials: Some(total_trials),
                pass_rate: Some(pass_rate),
                trials,
            });
        }

        let passed = case_summaries
            .iter()
            .filter(|r| r.status == CaseStatus::Passed)
            .count();
        let failed = case_summaries.len() - passed;
        let suite_pass_rate = if case_summaries.is_empty() {
            0.0
        } else {
            passed as f64 / case_summaries.len() as f64
        };
        let suite_pass = if args.ci {
            suite_pass_rate >= pass_threshold
        } else {
            failed == 0
        };

        let summary = SummaryResult {
            suite_pass,
            suite_pass_rate: Some(suite_pass_rate),
            agent: agent_key.clone(),
            model: args.model.clone(),
            total_cases: case_summaries.len(),
            passed,
            failed,
            trials_per_case: Some(trials_per_case),
            parallel: eval_config.parallel,
            pass_threshold: Some(pass_threshold),
            run_dir: run_dir.clone(),
            checks_path: eval_config.checks_path.clone(),
            skill_project_root: project_root.clone(),
            isolation: run_isolation,
            cases: case_summaries,
        };

        if let Err(e) = write_summary(&run_dir, &summary) {
            if !use_json {
                eprintln!("warning: failed to write summary.json: {}", e);
            }
        }

        if !suite_pass {
            any_agent_failed = true;
        }

        all_summaries.push(summary);
    }

    // Output results.
    if use_json {
        if all_summaries.len() == 1 {
            crate::outln!(
                "{}",
                serde_json::to_string_pretty(&all_summaries[0]).unwrap_or_default()
            );
        } else {
            crate::outln!(
                "{}",
                serde_json::to_string_pretty(&all_summaries).unwrap_or_default()
            );
        }
    } else {
        for summary in &all_summaries {
            let suite_pass_rate = summary.suite_pass_rate.unwrap_or(0.0);
            crate::outln!(
                "\nEval run complete for agent '{}': {}/{} passed",
                summary.agent,
                summary.passed,
                summary.total_cases
            );
            crate::outln!("  run_dir: {}", summary.run_dir.display());
            crate::outln!("  {}", render_isolation_line(summary.isolation.as_ref()));
            if let Some(iso) = &summary.isolation {
                if !iso.ambient_skills.is_empty() {
                    crate::outln!(
                        "  ambient skills visible to agent: {}",
                        iso.ambient_skills.join(", ")
                    );
                }
            }
            if summary.suite_pass {
                if args.ci {
                    crate::outln!(
                        "  result: PASSED (suite pass rate {:.0}% ≥ {:.0}% threshold)",
                        suite_pass_rate * 100.0,
                        pass_threshold * 100.0
                    );
                } else {
                    crate::outln!("  result: PASSED");
                }
            } else if args.ci {
                crate::outln!(
                    "  result: FAILED (suite pass rate {:.0}% < {:.0}% threshold)",
                    suite_pass_rate * 100.0,
                    pass_threshold * 100.0
                );
            } else {
                crate::outln!("  result: FAILED ({} case(s) failed)", summary.failed);
            }
        }
    }

    let should_fail = any_agent_failed;

    if should_fail && !args.no_fail {
        let total_passed: usize = all_summaries.iter().map(|s| s.passed).sum();
        let total_cases: usize = all_summaries.iter().map(|s| s.total_cases).sum();
        return Err(CliError::Config(format!(
            "Eval suite failed: {}/{} cases passed across {} agent(s) (threshold={})",
            total_passed,
            total_cases,
            all_summaries.len(),
            pass_threshold
        )));
    }

    Ok(())
}

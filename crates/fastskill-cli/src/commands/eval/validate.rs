//! Eval validate subcommand - configuration and file validation

use crate::commands::common::{runtime_selection_error_to_cli, validate_eval_format_args};
use crate::error::{CliError, CliResult};
use crate::runtime_selector::RuntimeSelectionInput;
use aikit_sdk::is_agent_available;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::OutputFormat;
use fastskill_evals::checks::{load_checks, load_checks_file, CheckDefinition};
use fastskill_evals::judge::{validate_judges, IssueLevel};
use fastskill_evals::resolve_eval_config;
use fastskill_evals::suite::load_suite;
use std::collections::HashMap;
use std::env;

use super::observability::{partition_scoreable, validate_suite_checks};

/// Arguments for `fastskill eval validate`
#[derive(Debug)]
pub struct ValidateArgs {
    /// Target runtime(s) for this operation; repeatable (mutually exclusive with --all)
    pub agent: Vec<String>,

    /// Target all runtimes discovered by aikit (mutually exclusive with --agent)
    pub all: bool,

    /// The model `eval run` will target, so a judge declaring the same one can
    /// be reported as self-preference (R14). Not resolved from anywhere else:
    /// asking a runtime what it would use is a network call, and validate is
    /// file-only.
    pub model: Option<String>,

    /// Output format: table, json (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,
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

impl IntoCommandSpec for ValidateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Validate eval configuration and files",
            syntax: Some("eval validate [OPTIONS]"),
            category: Some("quality"),
            examples: vec!["fastskill eval validate --all"],
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
                    name: "model",
                    kind: ArgKind::Option,
                    long: Some("model"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help:
                        "The model eval run will target; warns when a judge declares the same one",
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
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ValidateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ValidateArgs {
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
            model: map.get("model").and_then(|v| {
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
        }
    }
}

impl From<&ValidateArgs> for RuntimeSelectionInput {
    fn from(args: &ValidateArgs) -> Self {
        RuntimeSelectionInput {
            agents: args.agent.clone(),
            all: args.all,
        }
    }
}

/// Execute the `eval validate` command
pub async fn execute_validate(args: ValidateArgs) -> CliResult<()> {
    let format = validate_eval_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

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

    // Parse and validate prompts CSV
    let suite =
        load_suite(&eval_config.prompts_path).map_err(|e| CliError::Config(e.to_string()))?;
    let case_count = suite.cases.len();

    // Parse and validate checks TOML if present and exists
    let checks: Vec<CheckDefinition> = match &eval_config.checks_path {
        Some(checks_path) if checks_path.exists() => {
            load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?
        }
        _ => Vec::new(),
    };
    let check_count = checks.len();

    // A suite with no checks file is still not check-free: every case's
    // `should_trigger` column generates a required skill-invocation check, and
    // whether that one is observable depends on the backend and on isolation.
    // Isolation is the default, and it stages the `SKILL.md` beside the project
    // file — so a project without one cannot stage anything, on any mode.
    let skill_will_be_staged = project_root.join("SKILL.md").is_file();

    // Resolve runtime selection (optional for validate).
    let input = RuntimeSelectionInput::from(&args);
    let selection = crate::runtime_selector::resolve_runtime_selection(&input)
        .map_err(runtime_selection_error_to_cli)?;

    // Probe agent availability before printing anything: the JSON document
    // must carry the per-agent result (the table output already does), and it
    // must never claim `valid: true` for a selection that then fails with
    // EVAL_AGENT_UNAVAILABLE.
    // A contradiction between a case's `should_trigger` and an explicit
    // `skill_invoked` on it is a property of the suite, not of any backend, so
    // it fails validation outright and is asked even with no runtime selected.
    validate_suite_checks(&suite.cases, &checks)?;

    // R14: judge declarations are checked from file content alone, so this
    // gives the same answer on a laptop and in CI. Endpoint reachability and
    // key presence are R3's concern and are asked when judging starts.
    let mut judge_names: Vec<String> = Vec::new();
    let mut judge_warnings: Vec<String> = Vec::new();
    if let Some(checks_path) = eval_config.checks_path.as_ref().filter(|p| p.exists()) {
        let file = load_checks_file(checks_path).map_err(|e| CliError::Config(e.to_string()))?;
        judge_names = file.judges.iter().map(|j| j.name.clone()).collect();
        let checks_dir = checks_path.parent().unwrap_or(&project_root);
        let issues = validate_judges(&file, checks_dir, Some(&suite), None, args.model.as_deref());
        let errors: Vec<String> = issues
            .iter()
            .filter(|i| i.level == IssueLevel::Error)
            .map(ToString::to_string)
            .collect();
        if !errors.is_empty() {
            return Err(CliError::Config(format!(
                "EVAL_JUDGE_INVALID: {} declared in '{}':\n{}",
                if errors.len() == 1 {
                    "1 problem".to_string()
                } else {
                    format!("{} problems", errors.len())
                },
                checks_path.display(),
                errors.join("\n")
            )));
        }
        judge_warnings = issues
            .iter()
            .filter(|i| i.level == IssueLevel::Warning)
            .map(ToString::to_string)
            .collect();
    }

    let mut agents: Vec<(String, bool)> = Vec::new();
    let mut unscoreable: Vec<(String, String)> = Vec::new();
    if let Some(sel) = &selection {
        for agent_key in &sel.runtimes {
            let available = is_agent_available(agent_key);
            if !available && eval_config.fail_on_missing_agent {
                return Err(CliError::Config(format!(
                    "EVAL_AGENT_UNAVAILABLE: Agent '{}' is not available. Install it or use --agent with an available agent.",
                    agent_key
                )));
            }
            if !available {
                eprintln!(
                    "warning: agent '{}' is not available (fail_on_missing_agent=false, continuing)",
                    agent_key
                );
            }
            agents.push((agent_key.clone(), available));
        }

        // R10: report which of these backends could actually score the suite,
        // before `eval run` spends a token finding out. `validate` answers for
        // the default isolation mode; `run` re-asks with the mode selected.
        // A selection with nothing scoreable is invalid; a mixed one is
        // reported per agent, matching how `run` drops and continues.
        let split =
            partition_scoreable(&sel.runtimes, &suite.cases, &checks, skill_will_be_staged)?;
        if split.scoreable.is_empty() && !sel.runtimes.is_empty() {
            return Err(split.into_error());
        }
        unscoreable = split.into_reasons();
    }

    for warning in &judge_warnings {
        eprintln!("warning: {}", warning);
    }

    if use_json {
        let agents_json: serde_json::Map<String, serde_json::Value> = agents
            .iter()
            .map(|(key, available)| (key.clone(), serde_json::Value::Bool(*available)))
            .collect();
        // Sibling of `agents` rather than a richer value inside it: readers
        // pinned to `agents.<key> == true|false` keep working, and a backend
        // that can score simply has no entry here.
        let unscoreable_json: serde_json::Map<String, serde_json::Value> = unscoreable
            .iter()
            .map(|(key, reason)| (key.clone(), serde_json::Value::String(reason.clone())))
            .collect();
        let output = serde_json::json!({
            "valid": true,
            "prompts_path": eval_config.prompts_path,
            "checks_path": eval_config.checks_path,
            "timeout_seconds": eval_config.timeout_seconds,
            "trials_per_case": eval_config.trials_per_case,
            "parallel": eval_config.parallel,
            "pass_threshold": eval_config.pass_threshold,
            "fail_on_missing_agent": eval_config.fail_on_missing_agent,
            "project_root": eval_config.project_root,
            "case_count": case_count,
            "check_count": check_count,
            "judges": judge_names,
            "judge_warnings": judge_warnings,
            "agents": agents_json,
            "unscoreable": unscoreable_json,
        });
        crate::outln!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        crate::outln!("eval configuration: valid");
        crate::outln!("  prompts: {}", eval_config.prompts_path.display());
        crate::outln!("  cases: {}", case_count);
        if let Some(ref checks) = eval_config.checks_path {
            crate::outln!("  checks: {}", checks.display());
            crate::outln!("  check count: {}", check_count);
        }
        if !judge_names.is_empty() {
            crate::outln!("  judges: {}", judge_names.join(", "));
        }
        crate::outln!("  timeout: {}s", eval_config.timeout_seconds);
        crate::outln!("  trials_per_case: {}", eval_config.trials_per_case);
        crate::outln!("  parallel: {}", eval_config.parallel.unwrap_or(0));
        crate::outln!("  pass_threshold: {}", eval_config.pass_threshold);
        crate::outln!(
            "  fail_on_missing_agent: {}",
            eval_config.fail_on_missing_agent
        );
        for (agent_key, available) in &agents {
            let scoreable = !unscoreable.iter().any(|(key, _)| key == agent_key);
            crate::outln!(
                "  agent '{}': {}{}",
                agent_key,
                if *available {
                    "available"
                } else {
                    "unavailable"
                },
                if scoreable {
                    ""
                } else {
                    ", cannot score this suite"
                }
            );
        }
        for (agent_key, reason) in &unscoreable {
            crate::outln!("  agent '{}' has no score:\n{}", agent_key, reason);
        }
    }

    Ok(())
}

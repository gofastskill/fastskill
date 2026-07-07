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
use fastskill_evals::checks::load_checks;
use fastskill_evals::resolve_eval_config;
use fastskill_evals::suite::load_suite;
use std::collections::HashMap;
use std::env;

/// Arguments for `fastskill eval validate`
#[derive(Debug)]
pub struct ValidateArgs {
    /// Target runtime(s) for this operation; repeatable (mutually exclusive with --all)
    pub agent: Vec<String>,

    /// Target all runtimes discovered by aikit (mutually exclusive with --agent)
    pub all: bool,

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
    let check_count = if let Some(ref checks_path) = eval_config.checks_path {
        if checks_path.exists() {
            let checks = load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?;
            checks.len()
        } else {
            0
        }
    } else {
        0
    };

    // Resolve runtime selection (optional for validate).
    let input = RuntimeSelectionInput::from(&args);
    let selection = crate::runtime_selector::resolve_runtime_selection(&input)
        .map_err(runtime_selection_error_to_cli)?;

    if use_json {
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
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        println!("eval configuration: valid");
        println!("  prompts: {}", eval_config.prompts_path.display());
        println!("  cases: {}", case_count);
        if let Some(ref checks) = eval_config.checks_path {
            println!("  checks: {}", checks.display());
            println!("  check count: {}", check_count);
        }
        println!("  timeout: {}s", eval_config.timeout_seconds);
        println!("  trials_per_case: {}", eval_config.trials_per_case);
        println!("  parallel: {}", eval_config.parallel.unwrap_or(0));
        println!("  pass_threshold: {}", eval_config.pass_threshold);
        println!(
            "  fail_on_missing_agent: {}",
            eval_config.fail_on_missing_agent
        );
    }

    // Check agent availability for each resolved runtime.
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
            if !use_json {
                println!(
                    "  agent '{}': {}",
                    agent_key,
                    if available {
                        "available"
                    } else {
                        "unavailable"
                    }
                );
            }
        }
    }

    Ok(())
}

//! Eval validate subcommand - configuration and file validation

use crate::commands::common::{runtime_selection_error_to_cli, validate_format_args};
use crate::error::{CliError, CliResult};
use aikit_sdk::is_agent_available;
use clap::Args;
use fastskill_core::core::agent_runtime_selector::RuntimeSelectionInput;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::eval::checks::load_checks;
use fastskill_core::eval::config::resolve_eval_config;
use fastskill_core::eval::suite::load_suite;
use fastskill_core::OutputFormat;
use std::env;

/// Arguments for `fastskill eval validate`
#[derive(Debug, Args)]
#[command(
    about = "Validate eval configuration and files",
    after_help = "Examples:\n  fastskill eval validate\n  fastskill eval validate --agent codex\n  fastskill eval validate --agent codex --agent claude\n  fastskill eval validate --all"
)]
pub struct ValidateArgs {
    /// Target runtime(s) for this operation; repeatable (mutually exclusive with --all)
    #[arg(long, short = 'a', action = clap::ArgAction::Append)]
    pub agent: Vec<String>,

    /// Target all runtimes discovered by aikit (mutually exclusive with --agent)
    #[arg(long)]
    pub all: bool,

    /// Output format: table, json, grid, xml (default: table)
    #[arg(long, value_enum, help = "Output format: table, json, grid, xml")]
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    #[arg(long, help = "Shorthand for --format json")]
    pub json: bool,
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
    let format = validate_format_args(&args.format, args.json)?;
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
    let selection = fastskill_core::core::agent_runtime_selector::resolve_runtime_selection(&input)
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

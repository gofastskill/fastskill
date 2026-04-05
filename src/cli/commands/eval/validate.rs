//! Eval validate subcommand - configuration and file validation

use crate::cli::error::{CliError, CliResult};
use aikit_sdk::{is_agent_available, is_runnable, runnable_agents};
use clap::Args;
use fastskill::core::project::resolve_project_file;
use fastskill::eval::config::resolve_eval_config;
use std::env;

/// Arguments for `fastskill eval validate`
#[derive(Debug, Args)]
#[command(
    about = "Validate eval configuration and files",
    after_help = "Examples:\n  fastskill eval validate\n  fastskill eval validate --agent codex"
)]
pub struct ValidateArgs {
    /// Check agent availability for the specified agent key
    #[arg(long, value_parser = validate_agent_key_parser)]
    pub agent: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

fn validate_agent_key_parser(s: &str) -> Result<String, String> {
    if is_runnable(s) {
        Ok(s.to_string())
    } else {
        Err(format!(
            "'{}' is not a supported agent. Supported: {}",
            s,
            runnable_agents().join(", ")
        ))
    }
}

/// Execute the `eval validate` command
pub async fn execute_validate(args: ValidateArgs) -> CliResult<()> {
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

    // Check agent availability if --agent was specified
    if let Some(ref agent_key) = args.agent {
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
    }

    if args.json {
        let output = serde_json::json!({
            "valid": true,
            "prompts_path": eval_config.prompts_path,
            "checks_path": eval_config.checks_path,
            "timeout_seconds": eval_config.timeout_seconds,
            "fail_on_missing_agent": eval_config.fail_on_missing_agent,
            "project_root": eval_config.project_root,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_default()
        );
    } else {
        println!("eval configuration: valid");
        println!("  prompts: {}", eval_config.prompts_path.display());
        if let Some(ref checks) = eval_config.checks_path {
            println!("  checks: {}", checks.display());
        }
        println!("  timeout: {}s", eval_config.timeout_seconds);
        println!(
            "  fail_on_missing_agent: {}",
            eval_config.fail_on_missing_agent
        );
        if let Some(ref agent_key) = args.agent {
            let available = is_agent_available(agent_key);
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

    Ok(())
}

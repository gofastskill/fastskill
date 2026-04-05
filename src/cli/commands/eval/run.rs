//! Eval run subcommand - case execution orchestration

use crate::cli::error::{CliError, CliResult};
use aikit_sdk::{is_agent_available, is_runnable, runnable_agents};
use chrono::Utc;
use clap::Args;
use fastskill::core::project::resolve_project_file;
use fastskill::eval::artifacts::{
    allocate_run_dir, write_case_artifacts, write_summary, CaseSummary, SummaryResult,
};
use fastskill::eval::checks::load_checks;
use fastskill::eval::config::resolve_eval_config;
use fastskill::eval::runner::{run_eval_case, CaseRunOptions};
use fastskill::eval::suite::load_suite;
use fastskill::eval::trace::{stdout_to_trace, trace_to_jsonl};
use std::env;
use std::path::PathBuf;

/// Arguments for `fastskill eval run`
#[derive(Debug, Args)]
#[command(
    about = "Run eval cases against an agent",
    after_help = "Examples:\n  fastskill eval run --agent codex --output-dir /tmp/evals\n  fastskill eval run --agent claude --output-dir ./evals --case my-case\n  fastskill eval run --agent codex --output-dir ./evals --tag basic"
)]
pub struct RunArgs {
    /// Agent to use for execution (required)
    #[arg(long, required = true, value_parser = validate_agent_key_for_run)]
    pub agent: String,

    /// Output directory for artifacts (required)
    #[arg(long, required = true)]
    pub output_dir: PathBuf,

    /// Optional model override forwarded to the agent
    #[arg(long)]
    pub model: Option<String>,

    /// Filter: run only the case with this ID
    #[arg(long)]
    pub case: Option<String>,

    /// Filter: run only cases with this tag
    #[arg(long)]
    pub tag: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    #[arg(long)]
    pub no_fail: bool,
}

fn validate_agent_key_for_run(s: &str) -> Result<String, String> {
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

/// Execute the `eval run` command
pub async fn execute_run(args: RunArgs) -> CliResult<()> {
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

    // Check agent availability
    if eval_config.fail_on_missing_agent && !is_agent_available(&args.agent) {
        return Err(CliError::Config(format!(
            "EVAL_AGENT_UNAVAILABLE: Agent '{}' is not available. Install it first.",
            args.agent
        )));
    }

    // Load suite
    let mut suite =
        load_suite(&eval_config.prompts_path).map_err(|e| CliError::Config(e.to_string()))?;

    // Apply filters
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

    // Load checks if configured
    let checks = if let Some(ref checks_path) = eval_config.checks_path {
        load_checks(checks_path).map_err(|e| CliError::Config(e.to_string()))?
    } else {
        vec![]
    };

    // Allocate run directory
    let run_id = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    std::fs::create_dir_all(&args.output_dir).map_err(|e| {
        CliError::Config(format!(
            "Failed to create output directory '{}': {}",
            args.output_dir.display(),
            e
        ))
    })?;
    let run_dir =
        allocate_run_dir(&args.output_dir, &run_id).map_err(|e| CliError::Config(e.to_string()))?;

    let run_opts = CaseRunOptions {
        agent_key: args.agent.clone(),
        model: args.model.clone(),
        project_root: project_root.clone(),
        timeout_seconds: eval_config.timeout_seconds,
    };

    eprintln!(
        "Running {} eval case(s) with agent '{}'...",
        suite.cases.len(),
        args.agent
    );

    let mut case_results = Vec::new();
    let mut case_summaries = Vec::new();

    for case in &suite.cases {
        eprintln!("  Running case '{}'...", case.id);

        let (run_output, case_result) = run_eval_case(case, &run_opts, &checks).await;

        // Build trace
        let trace_events = stdout_to_trace(&run_output.stdout);
        let trace_jsonl = trace_to_jsonl(&trace_events);

        // Write artifacts
        if let Err(e) = write_case_artifacts(
            &run_dir,
            &case.id,
            &run_output.stdout,
            &run_output.stderr,
            &trace_jsonl,
            &case_result,
        ) {
            eprintln!(
                "  warning: failed to write artifacts for case '{}': {}",
                case.id, e
            );
        }

        let summary_entry = CaseSummary {
            id: case_result.id.clone(),
            status: case_result.status.clone(),
            command_count: case_result.command_count,
            input_tokens: case_result.input_tokens,
            output_tokens: case_result.output_tokens,
        };

        case_summaries.push(summary_entry);
        case_results.push(case_result);
    }

    let passed = case_results
        .iter()
        .filter(|r| r.status == fastskill::eval::artifacts::CaseStatus::Passed)
        .count();
    let failed = case_results.len() - passed;
    let suite_pass = failed == 0;

    let summary = SummaryResult {
        suite_pass,
        agent: args.agent.clone(),
        model: args.model.clone(),
        total_cases: case_results.len(),
        passed,
        failed,
        run_dir: run_dir.clone(),
        checks_path: eval_config.checks_path.map(|p| {
            if p.is_absolute() {
                p
            } else {
                project_root.join(p)
            }
        }),
        skill_project_root: project_root,
        cases: case_summaries,
    };

    // Write summary
    if let Err(e) = write_summary(&run_dir, &summary) {
        eprintln!("warning: failed to write summary.json: {}", e);
    }

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        println!(
            "\nEval run complete: {}/{} passed",
            passed,
            case_results.len()
        );
        println!("  run_dir: {}", run_dir.display());
        if suite_pass {
            println!("  result: PASSED");
        } else {
            println!("  result: FAILED ({} case(s) failed)", failed);
        }
    }

    if !suite_pass && !args.no_fail {
        return Err(CliError::Config(format!(
            "Eval suite failed: {}/{} cases passed",
            passed,
            case_results.len()
        )));
    }

    Ok(())
}

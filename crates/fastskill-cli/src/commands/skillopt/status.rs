//! `fastskill optimize status` subcommand

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Arguments for `fastskill optimize status`
#[derive(Debug)]
pub struct StatusArgs {
    /// Path to the run directory
    pub run_dir: PathBuf,

    /// Poll and re-render every ~2 seconds
    pub watch: bool,
}

impl IntoCommandSpec for StatusArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Show the status of a training run",
            syntax: Some("optimize status <run-dir> [--watch]"),
            args: vec![
                ArgSpec {
                    name: "run-dir",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Path to the run directory",
                    ..Default::default()
                },
                ArgSpec {
                    name: "watch",
                    kind: ArgKind::Flag,
                    long: Some("watch"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Poll and re-render every ~2 seconds",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for StatusArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            run_dir: match map.get("run-dir") {
                Some(ArgValue::Str(s)) => PathBuf::from(s),
                _ => panic!("framework bug: required 'run-dir' missing from validated map"),
            },
            watch: matches!(map.get("watch"), Some(ArgValue::Bool(true))),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeStateView {
    best_score: f64,
    epoch: u32,
    global_step: u32,
}

#[derive(Debug, Deserialize)]
struct StepRecordView {
    global_step: u32,
    accepted: bool,
    score_current: f64,
    score_candidate: f64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

pub async fn execute_status(args: StatusArgs) -> CliResult<()> {
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    loop {
        render_status(&args.run_dir)?;
        if !args.watch {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
    }

    Ok(())
}

fn render_status(run_dir: &Path) -> CliResult<()> {
    let state_path = run_dir.join("runtime_state.json");
    let history_path = run_dir.join("history.json");

    if !state_path.exists() || !history_path.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_CORRUPT: missing runtime_state.json or history.json in: {}",
            run_dir.display()
        )));
    }

    let state_bytes = std::fs::read(&state_path).map_err(CliError::Io)?;
    let state: RuntimeStateView = serde_json::from_slice(&state_bytes).map_err(|e| {
        CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_CORRUPT: malformed runtime_state.json: {e}"
        ))
    })?;

    let history_bytes = std::fs::read(&history_path).map_err(CliError::Io)?;
    let history: Vec<StepRecordView> = serde_json::from_slice(&history_bytes).unwrap_or_default();

    println!(
        "Run: {}  |  epoch: {}  global_step: {}  best_score: {:.4}",
        run_dir.display(),
        state.epoch,
        state.global_step,
        state.best_score
    );
    println!();
    println!(
        "{:<6}  {:<10}  {:<10}  {:<10}  {:<8}  {:<10}",
        "step", "gate", "score(S)", "score(S')", "delta", "tokens"
    );
    println!("{}", "-".repeat(62));

    for record in &history {
        let gate_label = if record.accepted {
            "accepted"
        } else {
            "rejected"
        };
        let delta = record.score_candidate - record.score_current;
        let tokens = record
            .input_tokens
            .unwrap_or(0)
            .saturating_add(record.output_tokens.unwrap_or(0));
        println!(
            "{:<6}  {:<10}  {:<10.4}  {:<10.4}  {:<+8.4}  {:<10}",
            record.global_step,
            gate_label,
            record.score_current,
            record.score_candidate,
            delta,
            tokens
        );
    }

    Ok(())
}

//! `fastskill optimization status` subcommand

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Arguments for `fastskill optimization status`
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
            help_order: Some(30),
            syntax: Some("optimization status <run-dir> [--watch]"),
            examples: vec![
                "fastskill optimization status ./optimize-runs/run-1",
                "fastskill optimization status ./optimize-runs/run-1 --watch",
            ],
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

    crate::outln!(
        "Run: {}  |  epoch: {}  global_step: {}  best_score: {:.4}",
        run_dir.display(),
        state.epoch,
        state.global_step,
        state.best_score
    );
    crate::outln!();
    crate::outln!(
        "{:<6}  {:<10}  {:<10}  {:<10}  {:<8}  {:<10}",
        "step",
        "gate",
        "score(S)",
        "score(S')",
        "delta",
        "tokens"
    );
    crate::outln!("{}", "-".repeat(62));

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
        crate::outln!(
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_state(run_dir: &Path, state: &str, history: &str) {
        std::fs::write(run_dir.join("runtime_state.json"), state).unwrap();
        std::fs::write(run_dir.join("history.json"), history).unwrap();
    }

    #[test]
    fn command_spec_and_argument_map_preserve_watch_flag() {
        let spec = StatusArgs::command_spec();
        assert_eq!(spec.help_order, Some(30));
        assert_eq!(spec.args.len(), 2);

        let map = HashMap::from([
            ("run-dir".to_string(), ArgValue::Str("run-one".to_string())),
            ("watch".to_string(), ArgValue::Bool(true)),
        ]);
        let args = StatusArgs::from_arg_value_map(&map);
        assert_eq!(args.run_dir, PathBuf::from("run-one"));
        assert!(args.watch);

        let map = HashMap::from([("run-dir".to_string(), ArgValue::Str("run-two".to_string()))]);
        assert!(!StatusArgs::from_arg_value_map(&map).watch);
    }

    #[tokio::test]
    async fn execute_reports_missing_run_directory() {
        let temp = TempDir::new().unwrap();
        let err = execute_status(StatusArgs {
            run_dir: temp.path().join("missing"),
            watch: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_RUN_DIR_MISSING"));
    }

    #[tokio::test]
    async fn execute_reports_missing_and_malformed_runtime_artifacts() {
        let temp = TempDir::new().unwrap();
        let err = execute_status(StatusArgs {
            run_dir: temp.path().to_path_buf(),
            watch: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_RUN_DIR_CORRUPT"));

        write_state(temp.path(), "not-json", "[]");
        let err = execute_status(StatusArgs {
            run_dir: temp.path().to_path_buf(),
            watch: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("malformed runtime_state.json"));
    }

    #[tokio::test]
    async fn renders_accepted_and_rejected_steps_with_saturating_token_totals() {
        let temp = TempDir::new().unwrap();
        write_state(
            temp.path(),
            r#"{"best_score":0.75,"epoch":2,"global_step":9}"#,
            r#"[
                {"global_step":8,"accepted":true,"score_current":0.5,"score_candidate":0.75,
                 "input_tokens":12,"output_tokens":8},
                {"global_step":9,"accepted":false,"score_current":0.75,"score_candidate":0.7,
                 "input_tokens":null,"output_tokens":null}
            ]"#,
        );

        let (result, output) = crate::output::capture(execute_status(StatusArgs {
            run_dir: temp.path().to_path_buf(),
            watch: false,
        }))
        .await;
        result.unwrap();
        assert!(output.contains("epoch: 2  global_step: 9  best_score: 0.7500"));
        assert!(output.contains("accepted"));
        assert!(output.contains("rejected"));
        assert!(output.contains("20"));
    }

    #[tokio::test]
    async fn malformed_history_is_rendered_as_an_empty_table() {
        let temp = TempDir::new().unwrap();
        write_state(
            temp.path(),
            r#"{"best_score":0.0,"epoch":0,"global_step":0}"#,
            "not-json",
        );

        let (result, output) = crate::output::capture(execute_status(StatusArgs {
            run_dir: temp.path().to_path_buf(),
            watch: false,
        }))
        .await;
        result.unwrap();
        assert!(output.contains("score(S')"));
        assert!(!output.contains("accepted"));
        assert!(!output.contains("rejected"));
    }
}

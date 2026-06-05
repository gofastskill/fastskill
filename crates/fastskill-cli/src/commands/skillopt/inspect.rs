//! `fastskill optimize inspect` subcommand

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use std::path::PathBuf;

/// Arguments for `fastskill optimize inspect`
#[derive(Debug)]
pub struct InspectArgs {
    /// Path to the run directory
    pub run_dir: PathBuf,

    /// Step number to inspect
    pub step: u32,

    /// What to show
    pub show: ShowMode,
}

#[derive(Debug, Clone)]
pub enum ShowMode {
    Patches,
    Diffs,
    Gate,
    Skips,
    All,
}

impl IntoCommandSpec for InspectArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Inspect per-step artifacts from a training run",
            syntax: Some("optimize inspect <run-dir> --step <n> [--show <mode>]"),
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
                    name: "step",
                    kind: ArgKind::Option,
                    long: Some("step"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Required,
                    help: "Step number to inspect",
                    ..Default::default()
                },
                ArgSpec {
                    name: "show",
                    kind: ArgKind::Option,
                    long: Some("show"),
                    value_type: ArgValueType::Enum(vec![
                        "patches", "diffs", "gate", "skips", "all",
                    ]),
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Enum("all".to_string())),
                    help: "What to show: patches, diffs, gate, skips, or all",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for InspectArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            run_dir: match map.get("run-dir") {
                Some(ArgValue::Str(s)) => PathBuf::from(s),
                _ => panic!("framework bug: required 'run-dir' missing from validated map"),
            },
            step: match map.get("step") {
                Some(ArgValue::Int(i)) => *i as u32,
                _ => panic!("framework bug: required 'step' missing from validated map"),
            },
            show: match map.get("show") {
                Some(ArgValue::Enum(s)) | Some(ArgValue::Str(s)) => match s.as_str() {
                    "patches" => ShowMode::Patches,
                    "diffs" => ShowMode::Diffs,
                    "gate" => ShowMode::Gate,
                    "skips" => ShowMode::Skips,
                    _ => ShowMode::All,
                },
                _ => ShowMode::All,
            },
        }
    }
}

pub async fn execute_inspect(args: InspectArgs) -> CliResult<()> {
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    let step_dir = args.run_dir.join(format!("step-{}", args.step));
    if !step_dir.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_STEP_NOT_FOUND: no artifacts for step {} in: {}",
            args.step,
            args.run_dir.display()
        )));
    }

    match args.show {
        ShowMode::Patches => show_patches(&step_dir)?,
        ShowMode::Diffs => show_diffs(&step_dir)?,
        ShowMode::Gate => show_gate(&step_dir)?,
        ShowMode::Skips => show_skips(&step_dir)?,
        ShowMode::All => {
            println!("=== patches ===");
            show_patches(&step_dir)?;
            println!("\n=== diffs ===");
            show_diffs(&step_dir)?;
            println!("\n=== gate ===");
            show_gate(&step_dir)?;
            println!("\n=== skips ===");
            show_skips(&step_dir)?;
        }
    }

    Ok(())
}

fn pretty_print_json(path: &std::path::Path, label: &str) -> CliResult<()> {
    if !path.exists() {
        println!("(no {} artifact)", label);
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(CliError::Io)?;
    let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
    Ok(())
}

fn show_patches(step_dir: &std::path::Path) -> CliResult<()> {
    pretty_print_json(&step_dir.join("patch.json"), "patch.json")
}

fn show_gate(step_dir: &std::path::Path) -> CliResult<()> {
    pretty_print_json(&step_dir.join("gate_scores.json"), "gate_scores.json")
}

fn show_skips(step_dir: &std::path::Path) -> CliResult<()> {
    pretty_print_json(&step_dir.join("skips.json"), "skips.json")
}

fn show_diffs(step_dir: &std::path::Path) -> CliResult<()> {
    let before_path = step_dir.join("skill_before.md");
    let after_path = step_dir.join("skill_after.md");

    let before = if before_path.exists() {
        std::fs::read_to_string(&before_path).map_err(CliError::Io)?
    } else {
        String::new()
    };

    let after = if after_path.exists() {
        std::fs::read_to_string(&after_path).map_err(CliError::Io)?
    } else {
        String::new()
    };

    println!("--- skill_before.md");
    println!("+++ skill_after.md");
    render_unified_diff(&before, &after);

    Ok(())
}

/// Emit a simple unified diff (all before lines as `-`, all after lines as `+`).
fn render_unified_diff(before: &str, after: &str) {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    // A minimal approach: show removed lines then added lines within a single hunk
    let max = std::cmp::max(before_lines.len(), after_lines.len());
    if max == 0 {
        return;
    }

    println!(
        "@@ -{},{} +{},{} @@",
        1,
        before_lines.len(),
        1,
        after_lines.len()
    );

    for line in &before_lines {
        println!("-{}", line);
    }
    for line in &after_lines {
        println!("+{}", line);
    }
}

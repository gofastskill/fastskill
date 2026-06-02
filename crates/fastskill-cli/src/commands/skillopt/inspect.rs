//! `fastskill skillopt inspect` subcommand

use crate::error::{CliError, CliResult};
use clap::Args;
use std::path::PathBuf;

/// Arguments for `fastskill skillopt inspect`
#[derive(Debug, Args)]
#[command(
    about = "Inspect per-step artifacts from a training run",
    after_help = "Examples:\n  fastskill skillopt inspect .skillopt/runs/2026-06-02T14-00-00Z --step 3\n  fastskill skillopt inspect .skillopt/runs/2026-06-02T14-00-00Z --step 3 --show diffs"
)]
pub struct InspectArgs {
    /// Path to the run directory
    pub run_dir: PathBuf,

    /// Step number to inspect
    #[arg(long, required = true)]
    pub step: u32,

    /// What to show
    #[arg(long, value_enum, default_value = "all")]
    pub show: ShowMode,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ShowMode {
    Patches,
    Diffs,
    Gate,
    Skips,
    All,
}

pub async fn execute_inspect(args: InspectArgs) -> CliResult<()> {
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    let step_dir = args.run_dir.join(format!("step-{}", args.step));
    if !step_dir.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_STEP_NOT_FOUND: no artifacts for step {} in: {}",
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

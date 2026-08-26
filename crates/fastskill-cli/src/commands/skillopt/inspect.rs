//! `fastskill optimize inspect` subcommand

use super::config::{resolve_step_versions, StepVersions};
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
            examples: vec![
                "fastskill optimize inspect ./optimize-runs/run-1 --step 3",
                "fastskill optimize inspect ./optimize-runs/run-1 --step 3 --show diffs",
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
                    name: "step",
                    kind: ArgKind::Option,
                    long: Some("step"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Required,
                    help: "Step number to inspect (0-based, matching `optimize status`)",
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

/// `--step N` is 0-based, matching both `aikit-skillopt`'s `global_step` counter
/// (the writer: `ensure_step_dir(run_dir, global_step)` writes `steps/step_{global_step:04}`)
/// and what `optimize status` prints in its `step` column (`status.rs` renders
/// `StepRecordView.global_step` verbatim, starting at 0 for the first step). Using
/// the same number the user just read off `status` means there is no off-by-one
/// translation for them to get wrong.
fn step_dir_for(run_dir: &std::path::Path, step: u32) -> PathBuf {
    run_dir.join("steps").join(format!("step_{step:04}"))
}

pub async fn execute_inspect(args: InspectArgs) -> CliResult<()> {
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    let step_dir = step_dir_for(&args.run_dir, args.step);
    if !step_dir.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_STEP_NOT_FOUND: no artifacts for step {} in: {}",
            args.step,
            args.run_dir.display()
        )));
    }

    match args.show {
        ShowMode::Patches => show_patches(&step_dir)?,
        ShowMode::Diffs => show_diffs(&args.run_dir, args.step)?,
        ShowMode::Gate => show_gate(&step_dir)?,
        ShowMode::Skips => show_skips(&step_dir)?,
        ShowMode::All => {
            crate::outln!("=== patches ===");
            show_patches(&step_dir)?;
            crate::outln!("\n=== diffs ===");
            show_diffs(&args.run_dir, args.step)?;
            crate::outln!("\n=== gate ===");
            show_gate(&step_dir)?;
            crate::outln!("\n=== skips ===");
            show_skips(&step_dir)?;
            crate::outln!("\n=== rollouts ===");
            show_rollouts(&step_dir)?;
        }
    }

    Ok(())
}

fn read_json(path: &std::path::Path) -> CliResult<Option<serde_json::Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(CliError::Io)?;
    Ok(Some(
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    ))
}

fn pretty_print_json(path: &std::path::Path, label: &str) -> CliResult<()> {
    match read_json(path)? {
        None => crate::outln!("(no {} artifact)", label),
        Some(val) => crate::outln!("{}", serde_json::to_string_pretty(&val).unwrap_or_default()),
    }
    Ok(())
}

/// `aikit-skillopt` (pinned `a34e1e47`, `aikit-textgrad/src/training/step.rs`) writes
/// the patch pool it selected for this step to `patch.json` under the step directory.
fn show_patches(step_dir: &std::path::Path) -> CliResult<()> {
    pretty_print_json(&step_dir.join("patch.json"), "patch.json")
}

/// The writer's step directory holds `gate.json` (accept/reject decision and
/// scores), not the `gate_scores.json` this reader used to look for.
fn show_gate(step_dir: &std::path::Path) -> CliResult<()> {
    pretty_print_json(&step_dir.join("gate.json"), "gate.json")
}

/// There is no `skips.json` artifact anywhere in the writer. The closest thing —
/// which fields of the proposed patch pool got dropped for exceeding the step's
/// edit budget — lives in `update.json`'s `skipped_count`/`chosen`/`budget` fields.
/// Render that data plainly, but say where it came from so the output cannot be
/// mistaken for a dedicated skips report that doesn't exist.
fn show_skips(step_dir: &std::path::Path) -> CliResult<()> {
    let path = step_dir.join("update.json");
    match read_json(&path)? {
        None => crate::outln!("(no update.json artifact — skip information unavailable)"),
        Some(val) => {
            crate::outln!(
                "Skip info (there is no dedicated skips.json artifact; derived from update.json):"
            );
            let budget = val
                .get("budget")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let chosen = val
                .get("chosen")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let skipped_count = val
                .get("skipped_count")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            crate::outln!("  budget:        {}", budget);
            crate::outln!("  chosen:        {}", chosen);
            crate::outln!("  skipped_count: {}", skipped_count);
        }
    }
    Ok(())
}

/// `rollouts.json` (per-case scores for this step's trajectories) is real writer
/// output that no `--show` mode previously surfaced at all.
fn show_rollouts(step_dir: &std::path::Path) -> CliResult<()> {
    pretty_print_json(&step_dir.join("rollouts.json"), "rollouts.json")
}

/// Versioned skill documents live at `<run>/skills/skill_v{NNNN}.md`
/// (`aikit-textgrad`'s `save_accepted_artifact`), not `skill_before.md`/
/// `skill_after.md` in the step directory — those never existed.
///
/// Version numbers advance only when a step is *accepted* (`save_accepted_artifact` runs
/// solely in that branch), so a step index is **not** a version index once any earlier
/// step has been rejected. We therefore resolve the bracketing versions from
/// `history.json` via [`resolve_step_versions`], counting accepted steps before this one,
/// rather than assuming `step -> v{step}/v{step+1}`.
///
/// That assumption is not merely imprecise, it is wrong in a silent way: with step 0
/// rejected and steps 1 and 2 accepted, step 1's real diff is `v0000 -> v0001`, but the
/// naive mapping resolves `v0001 -> v0002` — step 2's diff, rendered under step 1's name.
///
/// When history cannot tell us (step absent, or `history.json` missing/unreadable) we say
/// so rather than falling back to a guess.
fn show_diffs(run_dir: &std::path::Path, step: u32) -> CliResult<()> {
    let skills_dir = run_dir.join("skills");

    let (before, after) = match resolve_step_versions(run_dir, step) {
        StepVersions::Accepted { before, after } => (before, after),
        StepVersions::Rejected => {
            crate::outln!(
                "(no diff: step {step} was rejected, so no new skill version was written — \
                 run `optimize inspect --step {step} --show gate` for the scores)"
            );
            return Ok(());
        }
        StepVersions::Unknown => {
            crate::outln!(
                "(no diff: step {} is not recorded in {}/history.json, so its skill \
                 versions cannot be resolved)",
                step,
                run_dir.display()
            );
            return Ok(());
        }
    };

    let before_name = format!("skill_v{before:04}.md");
    let after_name = format!("skill_v{after:04}.md");
    let before_path = skills_dir.join(&before_name);
    let after_path = skills_dir.join(&after_name);

    if !before_path.exists() || !after_path.exists() {
        let missing = if before_path.exists() {
            &after_name
        } else {
            &before_name
        };
        crate::outln!(
            "(no diff available: step {} resolves to {} -> {}, but {} is missing under {})",
            step,
            before_name,
            after_name,
            missing,
            skills_dir.display()
        );
        return Ok(());
    }

    let before = std::fs::read_to_string(&before_path).map_err(CliError::Io)?;
    let after = std::fs::read_to_string(&after_path).map_err(CliError::Io)?;

    crate::outln!("--- {}", before_name);
    crate::outln!("+++ {}", after_name);
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

    crate::outln!(
        "@@ -{},{} +{},{} @@",
        1,
        before_lines.len(),
        1,
        after_lines.len()
    );

    for line in &before_lines {
        crate::outln!("-{}", line);
    }
    for line in &after_lines {
        crate::outln!("+{}", line);
    }
}

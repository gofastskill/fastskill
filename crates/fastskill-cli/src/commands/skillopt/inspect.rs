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
/// Mapping decision: version numbers only advance when a step is *accepted*
/// (`save_accepted_artifact` is only called in that branch), so they are not
/// guaranteed to equal the step index once any earlier step in the run was
/// rejected. Reconstructing the true accepted-version-count for an arbitrary
/// step would require scanning every prior step's `gate.json`. We instead take
/// the direct, obvious mapping — step N's before/after documents are
/// `skill_v{N:04}.md` and `skill_v{N+1:04}.md` — which is exact for step 0 (the
/// initial artifact is always written as `skill_v0000.md` before any step runs)
/// and for any run where every step up to N was accepted. When a version file is
/// missing (e.g. the step was rejected, so no new version was ever written), we
/// print an explanatory message naming the missing file instead of erroring or
/// fabricating a diff against stale or absent content.
fn show_diffs(run_dir: &std::path::Path, step: u32) -> CliResult<()> {
    let skills_dir = run_dir.join("skills");
    let before_name = format!("skill_v{step:04}.md");
    let after_name = format!("skill_v{:04}.md", step + 1);
    let before_path = skills_dir.join(&before_name);
    let after_path = skills_dir.join(&after_name);

    if !before_path.exists() {
        crate::outln!(
            "(no diff available: {} not found under {})",
            before_name,
            skills_dir.display()
        );
        return Ok(());
    }
    if !after_path.exists() {
        crate::outln!(
            "(no diff available: {} does not exist — step {} likely was not accepted, so no new version was written; run `optimize inspect --step {} --show gate` to check)",
            after_name,
            step,
            step
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

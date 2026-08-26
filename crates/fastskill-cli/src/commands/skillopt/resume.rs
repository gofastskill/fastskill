//! `fastskill optimize resume` subcommand

use super::config::{
    build_run_config, completion_output, count_history_steps, load_suite_with_splits,
    validate_config, SkillOptToml,
};
use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use std::path::PathBuf;

/// Arguments for `fastskill optimize resume`
#[derive(Debug)]
pub struct ResumeArgs {
    /// Path to the run directory to resume
    pub run_dir: PathBuf,
}

impl IntoCommandSpec for ResumeArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Resume an interrupted optimization run",
            syntax: Some("optimize resume <run-dir>"),
            examples: vec!["fastskill optimize resume ./optimize-runs/run-1"],
            args: vec![ArgSpec {
                name: "run-dir",
                kind: ArgKind::Positional,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                help: "Path to the run directory to resume",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for ResumeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            run_dir: match map.get("run-dir") {
                Some(ArgValue::Str(s)) => PathBuf::from(s),
                _ => panic!("framework bug: required 'run-dir' missing from validated map"),
            },
        }
    }
}

pub async fn execute_resume(args: ResumeArgs) -> CliResult<()> {
    // 1. Verify run dir exists
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    // 2. Read stored config from the run directory; try optimize.toml first, then skillopt.toml
    let stored_config_path = {
        let new_path = args.run_dir.join("optimize.toml");
        if new_path.exists() {
            new_path
        } else {
            args.run_dir.join("skillopt.toml") // backward compat for pre-rename run dirs
        }
    };
    if !stored_config_path.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_CORRUPT: missing optimize.toml in run directory: {}",
            args.run_dir.display()
        )));
    }

    let config_str = std::fs::read_to_string(&stored_config_path).map_err(|e| {
        CliError::Config(format!("OPTIMIZE_RUN_DIR_CORRUPT: cannot read config: {e}"))
    })?;

    let cfg: SkillOptToml = toml::from_str(&config_str).map_err(|e| {
        CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_CORRUPT: invalid config toml: {e}"
        ))
    })?;

    // 3. Validate using the run dir as base (paths in stored config are relative to it)
    let base_dir = args.run_dir.clone();
    validate_config(&cfg, &base_dir)?;

    // 4. Parse suite and checks
    let suite_path = base_dir.join(&cfg.suite);
    let splits = load_suite_with_splits(&suite_path).map_err(CliError::Config)?;
    if splits.selection_count == 0 {
        return Err(CliError::Config(
            "OPTIMIZE_NO_SELECTION_CASES: suite has zero cases tagged 'selection'".to_string(),
        ));
    }
    if splits.train_count == 0 {
        return Err(CliError::Config(
            "OPTIMIZE_NO_TRAIN_CASES: suite has zero cases tagged 'train'. The training \
             loop only steps over 'train' cases (an absent or empty split column also \
             counts as 'train') — add rows with split = \"train\", or leave the split \
             column empty, so there is something for the optimizer to train on."
                .to_string(),
        ));
    }
    let suite = splits.cases;

    let checks = if let Some(ref checks_path) = cfg.checks {
        let checks_path = base_dir.join(checks_path);
        fastskill_evals::load_checks(&checks_path)
            .map_err(|e| CliError::Config(format!("OPTIMIZE_CHECKS_PARSE_ERROR: {e}")))?
    } else {
        vec![]
    };

    // 5. Resolve optimizer_agent (warn if defaulting)
    let optimizer_agent = match cfg.optimizer_agent.clone() {
        Some(a) => a,
        None => {
            eprintln!(
                "OPTIMIZE_OPTIMIZER_DEFAULT_WARN: optimizer_agent not set, defaulting to target_agent '{}'",
                cfg.target_agent
            );
            cfg.target_agent.clone()
        }
    };

    // 6. Read skill document
    let skill_path = base_dir.join(&cfg.skill);
    let initial_skill_md = std::fs::read_to_string(&skill_path)
        .map_err(|e| CliError::Config(format!("OPTIMIZE_SKILL_NOT_FOUND: {e}")))?;

    // 7. Build RunConfig
    let run_config = build_run_config(&cfg, &optimizer_agent)
        .map_err(|e| CliError::Config(format!("OPTIMIZE_TRAINING_FAILED: invalid config: {e}")))?;

    // 8. Resume training
    // aikit-skillopt now takes the eval runner explicitly (post goaikit/aikit#148).
    let runner = fastskill_evals::runner::AikitEvalRunner;
    let outcome = aikit_skillopt::resume_skill(
        args.run_dir.clone(),
        initial_skill_md,
        cfg.skill_name.clone(),
        suite,
        checks,
        run_config,
        &runner,
    )
    .await
    .map_err(|e| CliError::Config(format!("OPTIMIZE_TRAINING_FAILED: {e}")))?;

    // Zero-step defensive check, same as `optimize run` (spec 013 finding #3).
    let step_count = count_history_steps(&args.run_dir);
    let (stdout_line, warning) = completion_output(step_count, &outcome.best_artifact_path);
    if let Some(warning) = warning {
        eprintln!("{warning}");
    }
    crate::outln!("{stdout_line}");
    Ok(())
}

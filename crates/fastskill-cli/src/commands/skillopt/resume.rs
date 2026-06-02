//! `fastskill skillopt resume` subcommand

use super::config::{build_run_config, load_suite_with_splits, validate_config, SkillOptToml};
use crate::error::{CliError, CliResult};
use clap::Args;
use std::path::PathBuf;

/// Arguments for `fastskill skillopt resume`
#[derive(Debug, Args)]
#[command(
    about = "Resume an interrupted optimization run",
    after_help = "Examples:\n  fastskill skillopt resume .skillopt/runs/2026-06-02T14-00-00Z"
)]
pub struct ResumeArgs {
    /// Path to the run directory to resume
    pub run_dir: PathBuf,
}

pub async fn execute_resume(args: ResumeArgs) -> CliResult<()> {
    // 1. Verify run dir exists
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    // 2. Read stored skillopt.toml from the run directory
    let stored_config_path = args.run_dir.join("skillopt.toml");
    if !stored_config_path.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_RUN_DIR_CORRUPT: missing skillopt.toml in run directory: {}",
            args.run_dir.display()
        )));
    }

    let config_str = std::fs::read_to_string(&stored_config_path).map_err(|e| {
        CliError::Config(format!(
            "SKILLOPT_RUN_DIR_CORRUPT: cannot read skillopt.toml: {e}"
        ))
    })?;

    let cfg: SkillOptToml = toml::from_str(&config_str).map_err(|e| {
        CliError::Config(format!(
            "SKILLOPT_RUN_DIR_CORRUPT: invalid skillopt.toml: {e}"
        ))
    })?;

    // 3. Validate using the run dir as base (paths in stored config are relative to it)
    let base_dir = args.run_dir.clone();
    validate_config(&cfg, &base_dir)?;

    // 4. Parse suite and checks
    let suite_path = base_dir.join(&cfg.suite);
    let (suite, _) = load_suite_with_splits(&suite_path).map_err(CliError::Config)?;

    let checks = if let Some(ref checks_path) = cfg.checks {
        let checks_path = base_dir.join(checks_path);
        fastskill_evals::load_checks(&checks_path)
            .map_err(|e| CliError::Config(format!("SKILLOPT_CHECKS_PARSE_ERROR: {e}")))?
    } else {
        vec![]
    };

    // 5. Resolve optimizer_agent (warn if defaulting)
    let optimizer_agent = match cfg.optimizer_agent.clone() {
        Some(a) => a,
        None => {
            eprintln!(
                "SKILLOPT_OPTIMIZER_DEFAULT_WARN: optimizer_agent not set, defaulting to target_agent '{}'",
                cfg.target_agent
            );
            cfg.target_agent.clone()
        }
    };

    // 6. Read skill document
    let skill_path = base_dir.join(&cfg.skill);
    let initial_skill_md = std::fs::read_to_string(&skill_path)
        .map_err(|e| CliError::Config(format!("SKILLOPT_SKILL_NOT_FOUND: {e}")))?;

    // 7. Build RunConfig
    let run_config = build_run_config(&cfg, &optimizer_agent)
        .map_err(|e| CliError::Config(format!("SKILLOPT_TRAINING_FAILED: invalid config: {e}")))?;

    // 8. Resume training
    let outcome = aikit_skillopt::resume_skill(
        args.run_dir.clone(),
        initial_skill_md,
        cfg.skill_name.clone(),
        suite,
        checks,
        run_config,
    )
    .await
    .map_err(|e| CliError::Config(format!("SKILLOPT_TRAINING_FAILED: {e}")))?;

    println!("{}", outcome.best_artifact_path.display());
    Ok(())
}

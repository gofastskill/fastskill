//! `fastskill skillopt run` subcommand

use super::config::{build_run_config, load_suite_with_splits, validate_config, SkillOptToml};
use crate::error::{CliError, CliResult};
use clap::Args;
use std::path::PathBuf;

/// Arguments for `fastskill skillopt run`
#[derive(Debug, Args)]
#[command(
    about = "Run skill optimization from a config file",
    after_help = "Examples:\n  fastskill skillopt run --config skillopt.toml\n  fastskill skillopt run --config skillopt.toml --out-dir /tmp/runs"
)]
pub struct RunArgs {
    /// Path to skillopt.toml config file
    #[arg(long, required = true)]
    pub config: PathBuf,

    /// Override the out_dir from the config file
    #[arg(long)]
    pub out_dir: Option<PathBuf>,

    /// Resume from this run directory instead of starting fresh
    #[arg(long)]
    pub resume: Option<PathBuf>,
}

pub async fn execute_run(args: RunArgs) -> CliResult<()> {
    if let Some(run_dir) = args.resume {
        return super::resume::execute_resume(super::resume::ResumeArgs { run_dir }).await;
    }

    // 1. Read config file
    if !args.config.exists() {
        return Err(CliError::Config(format!(
            "SKILLOPT_CONFIG_MISSING: config file not found: {}",
            args.config.display()
        )));
    }

    let config_str = std::fs::read_to_string(&args.config).map_err(|e| {
        CliError::Config(format!("SKILLOPT_CONFIG_MISSING: cannot read config: {e}"))
    })?;

    let mut cfg: SkillOptToml = toml::from_str(&config_str)
        .map_err(|e| CliError::Config(format!("SKILLOPT_INVALID_TOML: {e}")))?;

    if let Some(out_dir) = args.out_dir {
        cfg.out_dir = out_dir.to_string_lossy().to_string();
    }

    let config_dir = args
        .config
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    // 2. Validate (structural then file existence)
    validate_config(&cfg, &config_dir)?;

    // 3. Parse suite CSV with split resolution
    let suite_path = config_dir.join(&cfg.suite);
    let (suite, selection_count) = load_suite_with_splits(&suite_path).map_err(CliError::Config)?;

    if selection_count == 0 {
        return Err(CliError::Config(
            "SKILLOPT_NO_SELECTION_CASES: suite has zero cases tagged 'selection'".to_string(),
        ));
    }

    // 4. Load checks
    let checks = if let Some(ref checks_path) = cfg.checks {
        let checks_path = config_dir.join(checks_path);
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
    let skill_path = config_dir.join(&cfg.skill);
    let initial_skill_md = std::fs::read_to_string(&skill_path).map_err(|e| {
        CliError::Config(format!("SKILLOPT_SKILL_NOT_FOUND: cannot read skill: {e}"))
    })?;

    // 7. Allocate timestamped run directory
    let out_base = config_dir.join(&cfg.out_dir);
    std::fs::create_dir_all(&out_base).map_err(|e| {
        CliError::Config(format!(
            "SKILLOPT_OUT_DIR_UNWRITABLE: cannot create out_dir: {e}"
        ))
    })?;

    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let run_dir = out_base.join(&timestamp);
    std::fs::create_dir_all(&run_dir).map_err(|e| {
        CliError::Config(format!(
            "SKILLOPT_OUT_DIR_UNWRITABLE: cannot create run dir: {e}"
        ))
    })?;

    // 8. Copy config for provenance (before calling train_skill)
    std::fs::write(run_dir.join("skillopt.toml"), &config_str).map_err(CliError::Io)?;

    // 9. Build RunConfig via serde_json (avoids direct GateMetric/SlowUpdateMode imports)
    let run_config = build_run_config(&cfg, &optimizer_agent)
        .map_err(|e| CliError::Config(format!("SKILLOPT_TRAINING_FAILED: invalid config: {e}")))?;

    // 10. Build inputs and invoke training loop
    let inputs = aikit_skillopt::SkillOptInputs {
        initial_skill_md,
        skill_name: cfg.skill_name.clone(),
        suite,
        checks,
        config: run_config,
        run_dir: run_dir.clone(),
    };

    let outcome = aikit_skillopt::train_skill(inputs)
        .await
        .map_err(|e| CliError::Config(format!("SKILLOPT_TRAINING_FAILED: {e}")))?;

    println!("{}", outcome.best_artifact_path.display());
    Ok(())
}

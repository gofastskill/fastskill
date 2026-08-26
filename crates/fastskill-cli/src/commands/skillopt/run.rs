//! `fastskill optimize run` subcommand

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

/// Arguments for `fastskill optimize run`
#[derive(Debug)]
pub struct RunArgs {
    /// Path to optimize config file
    pub config: PathBuf,

    /// Override the out_dir from the config file
    pub out_dir: Option<PathBuf>,

    /// Resume from this run directory instead of starting fresh
    pub resume: Option<PathBuf>,
}

impl IntoCommandSpec for RunArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Run skill optimization from a config file",
            syntax: Some("optimize run --config <path> [--out-dir <dir>] [--resume <run-dir>]"),
            examples: vec!["fastskill optimize run --config ./optimize.toml"],
            args: vec![
                ArgSpec {
                    name: "config",
                    kind: ArgKind::Option,
                    long: Some("config"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Path to optimize config file",
                    ..Default::default()
                },
                ArgSpec {
                    name: "out-dir",
                    kind: ArgKind::Option,
                    long: Some("out-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Override the out_dir from the config file",
                    ..Default::default()
                },
                ArgSpec {
                    name: "resume",
                    kind: ArgKind::Option,
                    long: Some("resume"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Resume from this run directory instead of starting fresh",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for RunArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            config: match map.get("config") {
                Some(ArgValue::Str(s)) => PathBuf::from(s),
                _ => panic!("framework bug: required 'config' missing from validated map"),
            },
            out_dir: map.get("out-dir").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
            resume: map.get("resume").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
        }
    }
}

pub async fn execute_run(args: RunArgs) -> CliResult<()> {
    if let Some(run_dir) = args.resume {
        return super::resume::execute_resume(super::resume::ResumeArgs { run_dir }).await;
    }

    // 1. Read config file
    if !args.config.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_CONFIG_MISSING: config file not found: {}",
            args.config.display()
        )));
    }

    let config_str = std::fs::read_to_string(&args.config).map_err(|e| {
        CliError::Config(format!("OPTIMIZE_CONFIG_MISSING: cannot read config: {e}"))
    })?;

    let mut cfg: SkillOptToml = toml::from_str(&config_str)
        .map_err(|e| CliError::Config(format!("OPTIMIZE_INVALID_TOML: {e}")))?;

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

    // 4. Load checks
    let checks = if let Some(ref checks_path) = cfg.checks {
        let checks_path = config_dir.join(checks_path);
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
    let skill_path = config_dir.join(&cfg.skill);
    let initial_skill_md = std::fs::read_to_string(&skill_path).map_err(|e| {
        CliError::Config(format!("OPTIMIZE_SKILL_NOT_FOUND: cannot read skill: {e}"))
    })?;

    // 7. Allocate timestamped run directory
    let out_base = config_dir.join(&cfg.out_dir);
    std::fs::create_dir_all(&out_base).map_err(|e| {
        CliError::Config(format!(
            "OPTIMIZE_OUT_DIR_UNWRITABLE: cannot create out_dir: {e}"
        ))
    })?;

    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let run_dir = out_base.join(&timestamp);
    std::fs::create_dir_all(&run_dir).map_err(|e| {
        CliError::Config(format!(
            "OPTIMIZE_OUT_DIR_UNWRITABLE: cannot create run dir: {e}"
        ))
    })?;

    // 8. Copy config for provenance (before calling train_skill)
    std::fs::write(run_dir.join("optimize.toml"), &config_str).map_err(CliError::Io)?;

    // 9. Build RunConfig via serde_json (avoids direct GateMetric/SlowUpdateMode imports)
    let run_config = build_run_config(&cfg, &optimizer_agent)
        .map_err(|e| CliError::Config(format!("OPTIMIZE_TRAINING_FAILED: invalid config: {e}")))?;

    // 10. Build inputs and invoke training loop
    let inputs = aikit_skillopt::SkillOptInputs {
        initial_skill_md,
        skill_name: cfg.skill_name.clone(),
        suite,
        checks,
        config: run_config,
        run_dir: run_dir.clone(),
    };

    // aikit-skillopt now takes the eval runner explicitly (post goaikit/aikit#148).
    // fastskill drives real agents through the same runner its `eval` command uses.
    let runner = fastskill_evals::runner::AikitEvalRunner;
    let outcome = aikit_skillopt::train_skill(inputs, &runner)
        .await
        .map_err(|e| CliError::Config(format!("OPTIMIZE_TRAINING_FAILED: {e}")))?;

    // 11. Zero-step defensive check: even with the split validation above, a run
    // that recorded no training steps must not print the same success-shaped
    // one-line output as a real run (spec 013 finding #3).
    let step_count = count_history_steps(&run_dir);
    let (stdout_line, warning) = completion_output(step_count, &outcome.best_artifact_path);
    if let Some(warning) = warning {
        eprintln!("{warning}");
    }
    crate::outln!("{stdout_line}");
    Ok(())
}

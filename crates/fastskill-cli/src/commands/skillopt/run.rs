//! `fastskill optimization run` subcommand

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

/// Canonical run_dir-relative names for the archived copies of the run's
/// inputs. Fixed names (rather than the originals' basenames) cannot collide
/// with each other, with the provenance `optimize.toml`, or with anything the
/// training loop itself writes (`best_skill.md`, `history.json`,
/// `runtime_state.json`, `skills/`, `steps/`).
const ARCHIVED_SKILL: &str = "skill.md";
const ARCHIVED_SUITE: &str = "suite.csv";
const ARCHIVED_CHECKS: &str = "checks.toml";

/// Arguments for `fastskill optimization run`
#[derive(Debug)]
pub struct RunArgs {
    /// Path to optimize config file
    pub config: PathBuf,

    /// Override the out_dir from the config file
    pub out_dir: Option<PathBuf>,

    /// Resume from this run directory instead of starting fresh
    pub resume: Option<PathBuf>,

    /// Disable per-case scoring isolation (spec 016 D5): score in a shared
    /// materialized workspace against the ambient agent environment.
    /// Persisted into the run's provenance config so `optimize resume`
    /// keeps the same behaviour.
    pub no_isolation: bool,
}

impl IntoCommandSpec for RunArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Run skill optimization from a config file",
            help_order: Some(10),
            syntax: Some("optimization run --config <path> [--out-dir <dir>] [--resume <run-dir>]"),
            examples: vec!["fastskill optimization run --config ./optimize.toml"],
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
                ArgSpec {
                    name: "no-isolation",
                    kind: ArgKind::Flag,
                    long: Some("no-isolation"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Score in a shared workspace against the ambient agent environment \
                           (disables per-case scoring isolation)",
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
            no_isolation: matches!(map.get("no-isolation"), Some(ArgValue::Bool(true))),
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
    if args.no_isolation {
        cfg.isolate = Some(false);
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

    // 8. Archive the resolved inputs and config into the run dir (before
    // calling train_skill). `optimize resume` resolves the stored
    // optimize.toml against the run dir itself, so its skill/suite/checks
    // paths must point at copies that live there — without them every real
    // resume died at validate_config with SKILLOPT_SKILL_NOT_FOUND before the
    // loop. Archiving also pins the exact inputs this run trained on, immune
    // to later edits of the originals.
    let archive_input = |src: &std::path::Path, name: &str| -> CliResult<()> {
        std::fs::copy(src, run_dir.join(name))
            .map(|_| ())
            .map_err(|e| {
                CliError::Config(format!(
                    "OPTIMIZE_OUT_DIR_UNWRITABLE: cannot archive {} into run dir: {e}",
                    src.display()
                ))
            })
    };
    archive_input(&skill_path, ARCHIVED_SKILL)?;
    archive_input(&suite_path, ARCHIVED_SUITE)?;
    if let Some(ref checks_rel) = cfg.checks {
        archive_input(&config_dir.join(checks_rel), ARCHIVED_CHECKS)?;
    }

    // The provenance config's paths are rewritten to those run_dir-relative
    // copies so resume validates against exactly what was archived.
    let mut provenance: toml::Value = toml::from_str(&config_str)
        .map_err(|e| CliError::Config(format!("OPTIMIZE_INVALID_TOML: {e}")))?;
    if let Some(table) = provenance.as_table_mut() {
        table.insert("skill".to_string(), toml::Value::from(ARCHIVED_SKILL));
        table.insert("suite".to_string(), toml::Value::from(ARCHIVED_SUITE));
        if cfg.checks.is_some() {
            table.insert("checks".to_string(), toml::Value::from(ARCHIVED_CHECKS));
        }
        // Persist the effective isolation decision (config knob or
        // --no-isolation override) so resume replays the same behaviour
        // instead of silently re-defaulting.
        if let Some(isolate) = cfg.isolate {
            table.insert("isolate".to_string(), toml::Value::from(isolate));
        }
    }
    let provenance_str = toml::to_string_pretty(&provenance).map_err(|e| {
        CliError::Config(format!(
            "OPTIMIZE_INVALID_TOML: cannot serialize provenance config: {e}"
        ))
    })?;
    std::fs::write(run_dir.join("optimize.toml"), provenance_str).map_err(CliError::Io)?;

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
    let runner = fastskill_evals::runner::AikitEvalRunner::new();
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn config_toml(checks: Option<&str>, optimizer_agent: Option<&str>) -> String {
        let checks = checks
            .map(|path| format!("checks = \"{path}\"\n"))
            .unwrap_or_default();
        let optimizer = optimizer_agent
            .map(|agent| format!("optimizer_agent = \"{agent}\"\n"))
            .unwrap_or_default();
        format!(
            "skill = \"SKILL.md\"\n\
             skill_name = \"test-skill\"\n\
             suite = \"suite.csv\"\n\
             {checks}\
             out_dir = \"runs\"\n\
             target_agent = \"windsurf\"\n\
             {optimizer}\
             n_epochs = 1\n\
             batch_size = 1\n\
             accumulation = 1\n\
             aggregate_group_size = 1\n\
             lr_0 = 1\n\
             pass_threshold = 0.5\n\
             gate_metric = \"hard\"\n\
             gate_trials = 1\n\
             gate_epsilon = 0.0\n\
             slow_update_mode = \"gated\"\n\
             protected_soft_cap_chars = 500\n\
             timeout_seconds = 1\n"
        )
    }

    fn write_project(temp: &TempDir, suite: &str, config: &str) -> PathBuf {
        std::fs::write(temp.path().join("SKILL.md"), "# Skill\n").unwrap();
        std::fs::write(temp.path().join("suite.csv"), suite).unwrap();
        let config_path = temp.path().join("optimize.toml");
        std::fs::write(&config_path, config).unwrap();
        config_path
    }

    /// spec 016 D5: --no-isolation must be registered, default to false
    /// (isolated is the default), parse when present, and override the
    /// config knob to `Some(false)`.
    #[test]
    fn no_isolation_flag_registered_parsed_and_overrides_config() {
        let spec = RunArgs::command_spec();
        let flag = spec
            .args
            .iter()
            .find(|a| a.name == "no-isolation")
            .expect("--no-isolation registered");
        assert_eq!(flag.kind, ArgKind::Flag);

        let mut m = HashMap::new();
        m.insert(
            "config".to_string(),
            ArgValue::Str("optimize.toml".to_string()),
        );
        let args = RunArgs::from_arg_value_map(&m);
        assert!(!args.no_isolation, "isolation must be the default");

        m.insert("no-isolation".to_string(), ArgValue::Bool(true));
        let args = RunArgs::from_arg_value_map(&m);
        assert!(args.no_isolation);
    }

    #[test]
    fn argument_map_preserves_optional_paths() {
        let map = HashMap::from([
            (
                "config".to_string(),
                ArgValue::Str("optimize.toml".to_string()),
            ),
            (
                "out-dir".to_string(),
                ArgValue::Str("custom-runs".to_string()),
            ),
            ("resume".to_string(), ArgValue::Str("run-one".to_string())),
        ]);
        let args = RunArgs::from_arg_value_map(&map);
        assert_eq!(args.out_dir, Some(PathBuf::from("custom-runs")));
        assert_eq!(args.resume, Some(PathBuf::from("run-one")));
    }

    #[tokio::test]
    async fn reports_missing_and_malformed_config() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("missing.toml");
        let err = execute_run(RunArgs {
            config: missing,
            out_dir: None,
            resume: None,
            no_isolation: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_CONFIG_MISSING"));

        let config = temp.path().join("bad.toml");
        std::fs::write(&config, "not = [valid").unwrap();
        let err = execute_run(RunArgs {
            config,
            out_dir: None,
            resume: None,
            no_isolation: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_INVALID_TOML"));
    }

    #[tokio::test]
    async fn resume_option_delegates_before_reading_config() {
        let temp = TempDir::new().unwrap();
        let missing_run = temp.path().join("missing-run");
        let err = execute_run(RunArgs {
            config: temp.path().join("also-missing.toml"),
            out_dir: None,
            resume: Some(missing_run),
            no_isolation: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_RUN_DIR_MISSING"));
    }

    #[tokio::test]
    async fn enforces_selection_and_train_split_preconditions() {
        let temp = TempDir::new().unwrap();
        let config = write_project(
            &temp,
            "id,prompt,should_trigger,split\ntrain,hello,true,train\n",
            &config_toml(None, Some("windsurf")),
        );
        let err = execute_run(RunArgs {
            config,
            out_dir: Some(PathBuf::from("overridden-runs")),
            resume: None,
            no_isolation: true,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_NO_SELECTION_CASES"));

        let temp = TempDir::new().unwrap();
        let config = write_project(
            &temp,
            "id,prompt,should_trigger,split\nselect,hello,true,selection\n",
            &config_toml(None, Some("windsurf")),
        );
        let err = execute_run(RunArgs {
            config,
            out_dir: None,
            resume: None,
            no_isolation: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_NO_TRAIN_CASES"));
    }

    #[tokio::test]
    async fn reports_malformed_checks_after_valid_split_resolution() {
        let temp = TempDir::new().unwrap();
        let config = write_project(
            &temp,
            "id,prompt,should_trigger,split\ntrain,hello,true,train\nselect,world,true,selection\n",
            &config_toml(Some("checks.toml"), None),
        );
        std::fs::write(temp.path().join("checks.toml"), "[[check]\n").unwrap();

        let err = execute_run(RunArgs {
            config,
            out_dir: None,
            resume: None,
            no_isolation: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_CHECKS_PARSE_ERROR"));
    }

    #[tokio::test]
    async fn reports_unreadable_skill_and_unwritable_output_directory() {
        let suite =
            "id,prompt,should_trigger,split\ntrain,hello,true,train\nselect,world,true,selection\n";
        let temp = TempDir::new().unwrap();
        let config = write_project(&temp, suite, &config_toml(None, None));
        std::fs::remove_file(temp.path().join("SKILL.md")).unwrap();
        std::fs::create_dir(temp.path().join("SKILL.md")).unwrap();
        let err = execute_run(RunArgs {
            config,
            out_dir: None,
            resume: None,
            no_isolation: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_SKILL_NOT_FOUND"));

        let temp = TempDir::new().unwrap();
        let config = write_project(&temp, suite, &config_toml(None, Some("windsurf")));
        std::fs::write(temp.path().join("runs"), "blocks create_dir_all").unwrap();
        let err = execute_run(RunArgs {
            config,
            out_dir: None,
            resume: None,
            no_isolation: false,
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_OUT_DIR_UNWRITABLE"));
    }

    #[tokio::test]
    async fn successful_run_archives_inputs_and_writes_resumable_provenance() {
        let temp = TempDir::new().unwrap();
        let suite =
            "id,prompt,should_trigger,split\ntrain,hello,true,train\nselect,world,true,selection\n";
        let config = write_project(
            &temp,
            suite,
            &config_toml(Some("checks.toml"), Some("windsurf")),
        );
        std::fs::write(
            temp.path().join("checks.toml"),
            "[[check]]\nname = \"trigger_expectation\"\npattern = \"fastskill\"\nexpected = true\n",
        )
        .unwrap();

        let (result, output) = crate::output::capture(execute_run(RunArgs {
            config,
            out_dir: None,
            resume: None,
            no_isolation: false,
        }))
        .await;
        result.unwrap();
        assert!(output.contains("Best skill"));

        let run_dir = std::fs::read_dir(temp.path().join("runs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::read_to_string(run_dir.join(ARCHIVED_SKILL)).unwrap(),
            "# Skill\n"
        );
        assert_eq!(
            std::fs::read_to_string(run_dir.join(ARCHIVED_SUITE)).unwrap(),
            suite
        );
        assert!(run_dir.join(ARCHIVED_CHECKS).is_file());
        let provenance = std::fs::read_to_string(run_dir.join("optimize.toml")).unwrap();
        assert!(provenance.contains("skill = \"skill.md\""));
        assert!(provenance.contains("suite = \"suite.csv\""));
    }
}

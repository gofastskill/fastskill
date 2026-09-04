//! Eval run subcommand - case execution orchestration

use crate::runtime_selector::RuntimeSelectionInput;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::OutputFormat;

use std::collections::HashMap;
use std::path::PathBuf;

/// Arguments for `fastskill eval run`
#[derive(Debug)]
pub struct RunArgs {
    /// Target runtime(s) for this operation; repeatable (mutually exclusive with --all)
    pub agent: Vec<String>,

    /// Target all runtimes discovered by aikit (mutually exclusive with --agent)
    pub all: bool,

    /// Output directory for artifacts (required)
    pub output_dir: PathBuf,

    /// Optional model override forwarded to the agent
    pub model: Option<String>,

    /// Filter: run only the case with this ID
    pub case: Option<String>,

    /// Filter: run only cases with this tag
    pub tag: Option<String>,

    /// Output format: table, json (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,

    /// Do not fail with non-zero exit code on suite failure
    pub no_fail: bool,

    /// Override trials per case from config.
    ///
    /// Stored as the raw parsed integer (not clamped to `u32`) so the
    /// execute-time range check can echo exactly what the user typed — e.g. a
    /// negative `-3` is reported as `-3`, not a wrapped `u32::MAX`.
    pub trials: Option<i64>,

    /// Enable CI mode: exit non-zero if suite pass rate below threshold
    pub ci: bool,

    /// Override pass threshold (0.0-1.0)
    pub threshold: Option<f64>,

    /// Judge the run with the judges its checks file declares, right after the
    /// run's own scoring (spec eval-judge R13). The same function `eval judge`
    /// calls, so the two entry points cannot score a run differently.
    pub judge: bool,

    /// Override every judge's model, and record the override
    pub judge_model: Option<String>,

    /// Disable environment isolation: run cases in the project root against
    /// the ambient agent environment (legacy behaviour). Scores are then
    /// influenced by whatever skills are installed on this machine.
    pub no_isolation: bool,
}

fn parse_output_format(s: &str) -> Option<fastskill_core::OutputFormat> {
    match s {
        "table" => Some(fastskill_core::OutputFormat::Table),
        "json" => Some(fastskill_core::OutputFormat::Json),
        "grid" => Some(fastskill_core::OutputFormat::Grid),
        "xml" => Some(fastskill_core::OutputFormat::Xml),
        _ => None,
    }
}

impl IntoCommandSpec for RunArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Run eval cases against an agent",
            syntax: Some("eval run [OPTIONS]"),
            category: Some("quality"),
            examples: vec![
                "fastskill eval run --agent claude --output-dir ./eval-runs",
                "fastskill eval run --all --output-dir ./eval-runs --ci --threshold 0.9",
            ],
            args: vec![
                ArgSpec {
                    name: "agent",
                    kind: ArgKind::Option,
                    short: Some('a'),
                    long: Some("agent"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Target runtime(s); repeatable (mutually exclusive with --all)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "all",
                    kind: ArgKind::Flag,
                    long: Some("all"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Target all runtimes (mutually exclusive with --agent)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "output-dir",
                    kind: ArgKind::Option,
                    long: Some("output-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Output directory for artifacts (required)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "model",
                    kind: ArgKind::Option,
                    long: Some("model"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Optional model override forwarded to the agent",
                    ..Default::default()
                },
                ArgSpec {
                    name: "case",
                    kind: ArgKind::Option,
                    long: Some("case"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Filter: run only the case with this ID",
                    ..Default::default()
                },
                ArgSpec {
                    name: "tag",
                    kind: ArgKind::Option,
                    long: Some("tag"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Filter: run only cases with this tag",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Shorthand for --format json",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-fail",
                    kind: ArgKind::Flag,
                    long: Some("no-fail"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Do not fail with non-zero exit code on suite failure",
                    ..Default::default()
                },
                ArgSpec {
                    name: "trials",
                    kind: ArgKind::Option,
                    long: Some("trials"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Override trials per case from config (1-1000)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "ci",
                    kind: ArgKind::Flag,
                    long: Some("ci"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "CI mode: pass/fail on suite pass rate vs threshold",
                    ..Default::default()
                },
                ArgSpec {
                    name: "threshold",
                    kind: ArgKind::Option,
                    long: Some("threshold"),
                    value_type: ArgValueType::Float,
                    cardinality: Cardinality::Optional,
                    help: "Override pass threshold (0.0-1.0)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "judge",
                    kind: ArgKind::Flag,
                    long: Some("judge"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Judge the run after scoring it, with the judges checks.toml declares",
                    ..Default::default()
                },
                ArgSpec {
                    name: "judge-model",
                    kind: ArgKind::Option,
                    long: Some("judge-model"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Override every judge's model; recorded in each judgment",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-isolation",
                    kind: ArgKind::Flag,
                    long: Some("no-isolation"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Run in the project root against the ambient agent environment \
                           (disables per-case scratch-workspace isolation)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for RunArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        RunArgs {
            agent: match map.get("agent") {
                Some(ArgValue::List(items)) => items
                    .iter()
                    .filter_map(|i| {
                        if let ArgValue::Str(s) = i {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => vec![],
            },
            all: matches!(map.get("all"), Some(ArgValue::Bool(true))),
            output_dir: map
                .get("output-dir")
                .map(|v| {
                    if let ArgValue::Str(s) = v {
                        PathBuf::from(s)
                    } else {
                        panic!("fw bug")
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing output-dir")),
            model: map.get("model").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            case: map.get("case").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            tag: map.get("tag").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            no_fail: matches!(map.get("no-fail"), Some(ArgValue::Bool(true))),
            trials: map.get("trials").and_then(|v| {
                if let ArgValue::Int(i) = v {
                    // Carry the raw value through unchanged; the execute-time
                    // range check validates [1, 1000] and echoes the honest
                    // value the user typed on failure.
                    Some(*i)
                } else {
                    None
                }
            }),
            ci: matches!(map.get("ci"), Some(ArgValue::Bool(true))),
            threshold: map.get("threshold").and_then(|v| match v {
                ArgValue::Float(f) => Some(*f),
                ArgValue::Int(i) => Some(*i as f64),
                _ => None,
            }),
            no_isolation: matches!(map.get("no-isolation"), Some(ArgValue::Bool(true))),
            judge: matches!(map.get("judge"), Some(ArgValue::Bool(true))),
            judge_model: map.get("judge-model").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

impl From<&RunArgs> for RuntimeSelectionInput {
    fn from(args: &RunArgs) -> Self {
        RuntimeSelectionInput {
            agents: args.agent.clone(),
            all: args.all,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::items_after_test_module)]
mod tests {
    use super::*;
    use cli_framework::spec::arg_spec::{ArgKind, ArgValueType};

    fn base_map() -> HashMap<String, ArgValue> {
        let mut m = HashMap::new();
        // Value is incidental: it's parsed into a PathBuf field but no test in
        // this module performs filesystem I/O on it, so a platform-neutral
        // relative path is fine (avoids a Unix-only absolute path).
        m.insert(
            "output-dir".to_string(),
            ArgValue::Str("eval-out".to_string()),
        );
        m
    }

    /// The four flags must be registered in the command spec, otherwise the
    /// dispatcher never populates them and `from_arg_value_map` reads defaults.
    #[test]
    fn test_command_spec_registers_ci_flags() {
        let spec = RunArgs::command_spec();
        let by_name = |name: &str| spec.args.iter().find(|a| a.name == name).cloned();

        let no_fail = by_name("no-fail").expect("--no-fail registered");
        assert_eq!(no_fail.kind, ArgKind::Flag);

        let ci = by_name("ci").expect("--ci registered");
        assert_eq!(ci.kind, ArgKind::Flag);

        let trials = by_name("trials").expect("--trials registered");
        assert_eq!(trials.kind, ArgKind::Option);
        assert_eq!(trials.value_type, ArgValueType::Int);

        let threshold = by_name("threshold").expect("--threshold registered");
        assert_eq!(threshold.kind, ArgKind::Option);
        assert_eq!(threshold.value_type, ArgValueType::Float);
    }

    #[test]
    fn test_flags_default_when_absent() {
        let args = RunArgs::from_arg_value_map(&base_map());
        assert!(!args.no_fail);
        assert!(!args.ci);
        assert_eq!(args.trials, None);
        assert_eq!(args.threshold, None);
    }

    #[test]
    fn test_flags_parsed_from_map() {
        let mut m = base_map();
        m.insert("no-fail".to_string(), ArgValue::Bool(true));
        m.insert("ci".to_string(), ArgValue::Bool(true));
        m.insert("trials".to_string(), ArgValue::Int(7));
        m.insert("threshold".to_string(), ArgValue::Float(0.75));

        let args = RunArgs::from_arg_value_map(&m);
        assert!(args.no_fail);
        assert!(args.ci);
        assert_eq!(args.trials, Some(7));
        assert_eq!(args.threshold, Some(0.75));
    }

    #[test]
    fn test_threshold_accepts_integer_value() {
        let mut m = base_map();
        m.insert("threshold".to_string(), ArgValue::Int(1));
        let args = RunArgs::from_arg_value_map(&m);
        assert_eq!(args.threshold, Some(1.0));
    }

    #[test]
    fn test_negative_trials_preserved_for_honest_range_error() {
        let mut m = base_map();
        m.insert("trials".to_string(), ArgValue::Int(-3));
        let args = RunArgs::from_arg_value_map(&m);
        // The raw value is carried through unchanged so the execute-time
        // [1, 1000] range check can echo the honest value the user typed
        // (`-3`) rather than a wrapped `u32::MAX`.
        assert_eq!(args.trials, Some(-3));
    }

    /// spec eval-judge R13: the two options `eval run` carries for the judge
    /// tier must be registered, otherwise the dispatcher never populates them
    /// and `--judge` silently does nothing.
    #[test]
    fn test_command_spec_registers_judge_flags() {
        let spec = RunArgs::command_spec();
        let by_name = |name: &str| spec.args.iter().find(|a| a.name == name).cloned();

        let judge = by_name("judge").expect("--judge registered");
        assert_eq!(judge.kind, ArgKind::Flag);

        let judge_model = by_name("judge-model").expect("--judge-model registered");
        assert_eq!(judge_model.kind, ArgKind::Option);
        assert_eq!(judge_model.value_type, ArgValueType::String);

        let args = RunArgs::from_arg_value_map(&base_map());
        assert!(!args.judge, "judging must be opt-in");
        assert_eq!(args.judge_model, None);

        let mut m = base_map();
        m.insert("judge".to_string(), ArgValue::Bool(true));
        m.insert(
            "judge-model".to_string(),
            ArgValue::Str("judge-1".to_string()),
        );
        let args = RunArgs::from_arg_value_map(&m);
        assert!(args.judge);
        assert_eq!(args.judge_model.as_deref(), Some("judge-1"));
    }

    /// spec 016: --no-isolation must be registered as a flag, default to
    /// false (isolated is the default), and parse when present.
    #[test]
    fn test_no_isolation_flag_registered_and_parsed() {
        let spec = RunArgs::command_spec();
        let flag = spec
            .args
            .iter()
            .find(|a| a.name == "no-isolation")
            .expect("--no-isolation registered");
        assert_eq!(flag.kind, ArgKind::Flag);

        let args = RunArgs::from_arg_value_map(&base_map());
        assert!(!args.no_isolation, "isolation must be the default");

        let mut m = base_map();
        m.insert("no-isolation".to_string(), ArgValue::Bool(true));
        let args = RunArgs::from_arg_value_map(&m);
        assert!(args.no_isolation);
    }
}

mod execute;

pub use execute::execute_run;

// The injectable-runner entry point exists for the tests that drive a run
// end to end without an agent; nothing in the shipped binary calls it.
#[cfg(test)]
pub use execute::execute_run_with_runner;

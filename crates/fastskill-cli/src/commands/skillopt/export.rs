//! `fastskill optimize export` subcommand

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use std::path::PathBuf;

/// Arguments for `fastskill optimize export`
#[derive(Debug)]
pub struct ExportArgs {
    /// Path to the run directory
    pub run_dir: PathBuf,

    /// Destination path for the exported skill document
    pub out: PathBuf,
}

impl IntoCommandSpec for ExportArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Export the best skill document from a completed run",
            syntax: Some("optimize export <run-dir> --out <path>"),
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
                    name: "out",
                    kind: ArgKind::Option,
                    long: Some("out"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Destination path for the exported skill document",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for ExportArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            run_dir: match map.get("run-dir") {
                Some(ArgValue::Str(s)) => PathBuf::from(s),
                _ => panic!("framework bug: required 'run-dir' missing from validated map"),
            },
            out: match map.get("out") {
                Some(ArgValue::Str(s)) => PathBuf::from(s),
                _ => panic!("framework bug: required 'out' missing from validated map"),
            },
        }
    }
}

pub async fn execute_export(args: ExportArgs) -> CliResult<()> {
    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_RUN_DIR_MISSING: run directory not found: {}",
            args.run_dir.display()
        )));
    }

    let best_skill_path = args.run_dir.join("best_skill.md");
    if !best_skill_path.exists() {
        return Err(CliError::Config(format!(
            "OPTIMIZE_EXPORT_BEST_MISSING: best_skill.md not found in run directory: {}",
            args.run_dir.display()
        )));
    }

    if let Some(parent) = args.out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(CliError::Io)?;
        }
    }

    std::fs::copy(&best_skill_path, &args.out).map_err(CliError::Io)?;

    Ok(())
}

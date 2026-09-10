//! `fastskill optimization export` subcommand

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use std::path::PathBuf;

/// Arguments for `fastskill optimization export`
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
            help_order: Some(50),
            syntax: Some("optimization export <run-dir> --out <path>"),
            examples: vec!["fastskill optimization export ./optimize-runs/run-1 --out ./SKILL.md"],
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn command_spec_and_argument_map_describe_both_paths() {
        let spec = ExportArgs::command_spec();
        assert_eq!(spec.help_order, Some(50));
        assert_eq!(spec.args.len(), 2);

        let map = HashMap::from([
            ("run-dir".to_string(), ArgValue::Str("run-one".to_string())),
            (
                "out".to_string(),
                ArgValue::Str("exports/SKILL.md".to_string()),
            ),
        ]);
        let args = ExportArgs::from_arg_value_map(&map);
        assert_eq!(args.run_dir, PathBuf::from("run-one"));
        assert_eq!(args.out, PathBuf::from("exports/SKILL.md"));
    }

    #[tokio::test]
    async fn reports_missing_run_and_missing_best_artifact() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("missing");
        let err = execute_export(ExportArgs {
            run_dir: missing,
            out: temp.path().join("out.md"),
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_RUN_DIR_MISSING"));

        let err = execute_export(ExportArgs {
            run_dir: temp.path().to_path_buf(),
            out: temp.path().join("out.md"),
        })
        .await
        .unwrap_err();
        assert!(err.to_string().contains("OPTIMIZE_EXPORT_BEST_MISSING"));
    }

    #[tokio::test]
    async fn exports_to_a_nested_destination_and_overwrites_existing_content() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("best_skill.md"), "# improved\n").unwrap();
        let out = temp.path().join("release/nested/SKILL.md");

        execute_export(ExportArgs {
            run_dir: temp.path().to_path_buf(),
            out: out.clone(),
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "# improved\n");

        std::fs::write(temp.path().join("best_skill.md"), "# newer\n").unwrap();
        execute_export(ExportArgs {
            run_dir: temp.path().to_path_buf(),
            out: out.clone(),
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(out).unwrap(), "# newer\n");
    }

    #[tokio::test]
    async fn reports_an_io_error_when_destination_parent_is_a_file() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("best_skill.md"), "skill").unwrap();
        let parent_file = temp.path().join("blocked");
        std::fs::write(&parent_file, "not a directory").unwrap();

        let err = execute_export(ExportArgs {
            run_dir: temp.path().to_path_buf(),
            out: parent_file.join("SKILL.md"),
        })
        .await
        .unwrap_err();
        assert!(matches!(err, CliError::Io(_)));
    }
}

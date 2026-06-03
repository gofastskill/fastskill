//! Eval report subcommand - artifact summary and formatting

use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::OutputFormat;
use fastskill_evals::artifacts::read_summary;
use std::collections::HashMap;
use std::path::PathBuf;

/// Arguments for `fastskill eval report`
#[derive(Debug)]
pub struct ReportArgs {
    /// Path to the specific run directory
    pub run_dir: PathBuf,

    /// Output format: table, json, grid, xml (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,
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

impl IntoCommandSpec for ReportArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Show a report for a completed eval run",
            syntax: Some("eval report [OPTIONS]"),
            category: Some("quality"),
            args: vec![
                ArgSpec {
                    name: "run-dir",
                    kind: ArgKind::Option,
                    long: Some("run-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Path to the specific run directory",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json, grid, xml",
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
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReportArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ReportArgs {
            run_dir: map
                .get("run-dir")
                .map(|v| {
                    if let ArgValue::Str(s) = v {
                        PathBuf::from(s)
                    } else {
                        panic!("fw bug")
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing run-dir")),
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
        }
    }
}

/// Execute the `eval report` command
pub async fn execute_report(args: ReportArgs) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;
    let use_json = format == OutputFormat::Json;

    if !args.run_dir.exists() {
        return Err(CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Run directory does not exist: {}",
            args.run_dir.display()
        )));
    }

    let summary = read_summary(&args.run_dir).map_err(|e| {
        CliError::Config(format!(
            "EVAL_ARTIFACTS_CORRUPT: Failed to read summary.json: {}",
            e
        ))
    })?;

    if use_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        println!("Eval Report");
        println!("  run_dir: {}", args.run_dir.display());
        println!("  agent: {}", summary.agent);
        if let Some(ref model) = summary.model {
            println!("  model: {}", model);
        }
        println!(
            "  result: {}",
            if summary.suite_pass {
                "PASSED"
            } else {
                "FAILED"
            }
        );
        println!("  cases: {}/{} passed", summary.passed, summary.total_cases);

        if !summary.cases.is_empty() {
            println!("\nCase Results:");
            for case in &summary.cases {
                let token_info = match (case.input_tokens, case.output_tokens) {
                    (Some(i), Some(o)) => format!(" (in={} out={})", i, o),
                    (Some(i), None) => format!(" (in={})", i),
                    (None, Some(o)) => format!(" (out={})", o),
                    (None, None) => String::new(),
                };
                println!("  [{}] {}{}", case.status, case.id, token_info);
            }
        }
    }

    Ok(())
}

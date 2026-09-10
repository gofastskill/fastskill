//! Installed bundle inventory for `fastskill bundle list`.

use crate::commands::common::validate_format_args;
use crate::error::{manifest_required_message, CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::bundle::BundleService;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::{FastSkillService, OutputFormat};
use std::collections::HashMap;
use std::env;

#[derive(Debug, Clone)]
pub struct ListArgs {
    /// Output format: table, json, grid, xml
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,
}

impl IntoCommandSpec for ListArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List installed bundles",
            syntax: Some("bundle list [OPTIONS]"),
            category: Some("skills-projects"),
            help_order: Some(30),
            examples: vec!["fastskill bundle list", "fastskill bundle list --json"],
            args: vec![
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::Enum(vec!["table", "json", "grid", "xml"]),
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json, grid, xml (default: table)",
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

impl FromArgValueMap for ListArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            format: map.get("format").and_then(|value| match value {
                ArgValue::Str(value) => parse_format(value),
                _ => None,
            }),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

fn parse_format(value: &str) -> Option<OutputFormat> {
    match value {
        "table" => Some(OutputFormat::Table),
        "json" => Some(OutputFormat::Json),
        "grid" => Some(OutputFormat::Grid),
        "xml" => Some(OutputFormat::Xml),
        _ => None,
    }
}

pub async fn execute_list(
    service: &FastSkillService,
    args: ListArgs,
    global: bool,
) -> CliResult<()> {
    if global {
        return Err(CliError::Validation(
            "Bundle operations require a project Manifest and do not support --global".to_string(),
        ));
    }
    let format = validate_format_args(&args.format, args.json)?;
    let current = env::current_dir().map_err(|error| {
        CliError::Config(format!("Failed to determine current directory: {error}"))
    })?;
    let project = resolve_project_file(&current);
    if !project.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }
    let root = project.path.parent().ok_or_else(|| {
        CliError::Config("skill-project.toml has no project directory".to_string())
    })?;
    let bundles = BundleService::new(root, service.config().skill_storage_path.clone())
        .list()
        .map_err(CliError::Service)?;
    if matches!(format, OutputFormat::Json) {
        let rendered = serde_json::to_string_pretty(&bundles).map_err(|error| {
            CliError::Config(format!("Failed to format bundle list as JSON: {error}"))
        })?;
        crate::outln!("{rendered}");
    } else if bundles.is_empty() {
        crate::outln!("No bundles installed");
    } else {
        for bundle in bundles {
            crate::outln!(
                "{} {} [{}]",
                bundle.id,
                bundle.version,
                bundle.members.join(", ")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_map_parses_json_and_format() {
        let args = ListArgs::from_arg_value_map(&HashMap::from([
            ("format".to_string(), ArgValue::Str("grid".to_string())),
            ("json".to_string(), ArgValue::Bool(true)),
        ]));
        assert!(matches!(args.format, Some(OutputFormat::Grid)));
        assert!(args.json);
    }
}

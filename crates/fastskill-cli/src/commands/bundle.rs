//! Build publishable, self-contained skill bundles.

use crate::config::resolve_skills_storage_directory;
use crate::error::{manifest_required_message, CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::bundle::BundleService;
use fastskill_core::core::project::resolve_project_file;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

#[derive(Debug)]
pub struct BuildArgs {
    output: Option<PathBuf>,
}

impl IntoCommandSpec for BuildArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Build the bundle declared in skill-project.toml's [bundle] section",
            long_about: Some(
                "Build a self-contained ZIP from the skills selected under [bundle.members]. \
                 Each selected member must also appear in [dependencies].\n\n\
                 Add this to skill-project.toml before building:\n\n\
                 [bundle]\n\
                 format = \"fastskill-bundle-v1\"\n\
                 id = \"payments-team\"\n\
                 version = \"1.2.0\"\n\n\
                 [bundle.members.code-review]\n\
                 overridable = false\n\n\
                 [dependencies]\n\
                 code-review = \"1.0.0\"\n\n\
                 Set overridable = true only when recipients may replace that member with a \
                 declared personal override.",
            ),
            syntax: Some("bundle build [--output DIRECTORY]"),
            category: Some("packages"),
            examples: vec![
                "fastskill bundle build",
                "fastskill bundle build --output dist",
            ],
            args: vec![ArgSpec {
                name: "output",
                kind: ArgKind::Option,
                long: Some("output"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                help: "Directory for <bundle-id>-<version>.zip (default: project root)",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for BuildArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            output: map.get("output").and_then(|value| {
                if let ArgValue::Str(path) = value {
                    Some(PathBuf::from(path))
                } else {
                    None
                }
            }),
        }
    }
}

pub async fn execute_build(args: BuildArgs, skills_dir_override: Option<PathBuf>) -> CliResult<()> {
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
    let output = args.output.unwrap_or_else(|| root.to_path_buf());
    let skills_directory = match skills_dir_override {
        Some(path) => path,
        None => resolve_skills_storage_directory(false)?,
    };
    let result = BundleService::new(root, skills_directory)
        .build(&output)
        .map_err(CliError::Service)?;
    crate::outln!(
        "Built bundle {}@{}: {}",
        result.id,
        result.version,
        result.artifact.display()
    );
    Ok(())
}

#[derive(Debug)]
pub struct OverrideArgs {
    id: String,
    from: PathBuf,
}

impl IntoCommandSpec for OverrideArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Declare a permitted personal bundle-member override",
            syntax: Some("bundle override <SKILL_ID> --from DIRECTORY"),
            category: Some("packages"),
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Bundle member to override",
                    ..Default::default()
                },
                ArgSpec {
                    name: "from",
                    kind: ArgKind::Option,
                    long: Some("from"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Directory containing the personal SKILL.md",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for OverrideArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        let id = map
            .get("skill-id")
            .and_then(|value| match value {
                ArgValue::Str(value) => Some(value.clone()),
                _ => None,
            })
            .expect("required skill-id is supplied by command framework");
        let from = map
            .get("from")
            .and_then(|value| match value {
                ArgValue::Str(value) => Some(PathBuf::from(value)),
                _ => None,
            })
            .expect("required from is supplied by command framework");
        Self { id, from }
    }
}

pub async fn execute_override(
    args: OverrideArgs,
    skills_dir_override: Option<PathBuf>,
) -> CliResult<()> {
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
    let skills_directory = match skills_dir_override {
        Some(path) => path,
        None => resolve_skills_storage_directory(false)?,
    };
    BundleService::new(root, skills_directory)
        .override_member(&args.id, &args.from)
        .map_err(CliError::Service)?;
    crate::outln!("Declared personal override for {}", args.id);
    Ok(())
}

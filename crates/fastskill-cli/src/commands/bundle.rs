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

pub async fn execute_build(
    args: BuildArgs,
    skills_dir_override: Option<PathBuf>,
    global: bool,
) -> CliResult<()> {
    if global {
        return Err(CliError::Validation(
            "Bundle operations require a project Manifest and do not support --global".to_string(),
        ));
    }
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
    from: Option<PathBuf>,
    reset: bool,
    reindex: bool,
    no_reindex: bool,
    dry_run: bool,
    json: bool,
}

impl IntoCommandSpec for OverrideArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Declare a permitted personal bundle-member override",
            syntax: Some("bundle override <SKILL_ID> (--from DIRECTORY | --reset)"),
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
                    cardinality: Cardinality::Optional,
                    help: "Directory containing the personal SKILL.md",
                    ..Default::default()
                },
                ArgSpec {
                    name: "reset",
                    kind: ArgKind::Flag,
                    long: Some("reset"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Restore the packaged member and clear its personal override",
                    ..Default::default()
                },
                ArgSpec {
                    name: "reindex",
                    kind: ArgKind::Flag,
                    long: Some("reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Trigger reindex after changing the override",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-reindex",
                    kind: ArgKind::Flag,
                    long: Some("no-reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Skip reindex after changing the override",
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    kind: ArgKind::Flag,
                    long: Some("dry-run"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Validate and preview without changing managed state",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Emit one machine-readable lifecycle result",
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
        let from = map.get("from").and_then(|value| match value {
            ArgValue::Str(value) => Some(PathBuf::from(value)),
            _ => None,
        });
        let reset = matches!(map.get("reset"), Some(ArgValue::Bool(true)));
        let reindex = matches!(map.get("reindex"), Some(ArgValue::Bool(true)));
        let no_reindex = matches!(map.get("no-reindex"), Some(ArgValue::Bool(true)));
        let dry_run = matches!(map.get("dry-run"), Some(ArgValue::Bool(true)));
        let json = matches!(map.get("json"), Some(ArgValue::Bool(true)));
        Self {
            id,
            from,
            reset,
            reindex,
            no_reindex,
            dry_run,
            json,
        }
    }
}

pub async fn execute_override(
    args: OverrideArgs,
    service: &fastskill_core::FastSkillService,
    global: bool,
) -> CliResult<()> {
    if global {
        return Err(CliError::Validation(
            "Bundle operations require a project Manifest and do not support --global".to_string(),
        ));
    }
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    match (&args.from, args.reset) {
        (Some(_), true) | (None, false) => {
            return Err(CliError::Validation(
                "Choose either --from DIRECTORY or --reset".to_string(),
            ));
        }
        _ => {}
    }
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
    let skills_directory = service.config().skill_storage_path.clone();
    let bundles = BundleService::new(root, skills_directory);
    let preview = if args.reset {
        bundles
            .preview_reset_override(&args.id)
            .map_err(CliError::Service)?
    } else {
        bundles
            .preview_override(
                &args.id,
                args.from.as_deref().expect("validated --from argument"),
            )
            .map_err(CliError::Service)?
    };
    if args.dry_run {
        return emit_override_result(&preview, true, args.json);
    }
    if args.reset {
        let changed = bundles
            .reset_override(&args.id)
            .map_err(CliError::Service)?;
        if !args.json && changed {
            crate::outln!("Reset personal override for {}", args.id);
        } else if !args.json {
            crate::outln!("Personal override for {} is already reset", args.id);
        }
    } else if let Some(from) = &args.from {
        bundles
            .override_member(&args.id, from)
            .map_err(CliError::Service)?;
        if !args.json {
            crate::outln!("Declared personal override for {}", args.id);
        }
    }
    let auto_reindex = crate::config_file::load_auto_reindex_config();
    crate::utils::reindex_utils::maybe_auto_reindex(
        service,
        "bundle override",
        args.reindex,
        args.no_reindex,
        auto_reindex,
        false,
    )
    .await?;
    if args.json {
        emit_override_result(&preview, false, true)?;
    }
    Ok(())
}

fn emit_override_result(
    preview: &fastskill_core::core::bundle::BundleOverridePreview,
    dry_run: bool,
    json: bool,
) -> CliResult<()> {
    let outcome = if preview.changed {
        "changed"
    } else {
        "unchanged"
    };
    if json {
        let rendered = serde_json::to_string_pretty(&serde_json::json!({
            "scope": "project",
            "outcome": outcome,
            "dry_run": dry_run,
            "targets": [{
                "id": preview.id,
                "outcome": outcome,
                "current_revision": preview.current_revision,
                "target_revision": preview.target_revision,
                "changes": preview.changes,
                "retained": preview.retained,
            }],
            "diagnostics": Vec::<String>::new(),
        }))
        .map_err(|error| {
            CliError::Config(format!("Failed to serialize override result: {error}"))
        })?;
        crate::outln!("{rendered}");
    } else if preview.changed {
        crate::outln!(
            "Would change personal override for {}; no changes were applied",
            preview.id
        );
    } else {
        crate::outln!("Personal override for {} is already unchanged", preview.id);
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use fastskill_core::core::bundle::BundleOverridePreview;
    use fastskill_core::ServiceConfig;

    #[tokio::test]
    async fn build_reports_the_missing_project_manifest() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = tempfile::TempDir::new().unwrap();
        let original = std::env::current_dir().ok();
        let _guard = fastskill_core::test_utils::DirGuard(original);
        std::env::set_current_dir(root.path()).unwrap();

        assert!(matches!(
            execute_build(BuildArgs { output: None }, None, false).await,
            Err(CliError::Config(message)) if message.contains("skill-project.toml")
        ));

        assert!(matches!(
            execute_build(BuildArgs { output: None }, None, true).await,
            Err(CliError::Validation(message)) if message.contains("do not support --global")
        ));
    }

    #[test]
    fn argument_maps_ignore_values_with_the_wrong_type() {
        let build = BuildArgs::from_arg_value_map(&HashMap::from([(
            "output".to_string(),
            ArgValue::Bool(true),
        )]));
        assert!(build.output.is_none());

        let override_args = OverrideArgs::from_arg_value_map(&HashMap::from([
            ("skill-id".to_string(), ArgValue::Str("demo".to_string())),
            ("from".to_string(), ArgValue::Bool(true)),
        ]));
        assert_eq!(override_args.id, "demo");
        assert!(override_args.from.is_none());
    }

    #[tokio::test]
    async fn override_rejects_global_scope_before_project_discovery() {
        let root = tempfile::TempDir::new().unwrap();
        let service = fastskill_core::FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..ServiceConfig::default()
        })
        .await
        .unwrap();
        let override_args = OverrideArgs {
            id: "demo".to_string(),
            from: Some(root.path().join("demo")),
            reset: false,
            reindex: false,
            no_reindex: false,
            dry_run: false,
            json: false,
        };

        assert!(execute_override(override_args, &service, true)
            .await
            .is_err());

        let conflicting_reindex = OverrideArgs {
            id: "demo".to_string(),
            from: Some(root.path().join("demo")),
            reset: false,
            reindex: true,
            no_reindex: true,
            dry_run: false,
            json: false,
        };
        assert!(matches!(
            execute_override(conflicting_reindex, &service, false).await,
            Err(CliError::Validation(message)) if message.contains("--reindex and --no-reindex")
        ));
    }

    #[tokio::test]
    async fn override_result_has_stable_human_and_json_contracts() {
        let changed = BundleOverridePreview {
            id: "demo".to_string(),
            current_revision: Some("old".to_string()),
            target_revision: Some("new".to_string()),
            changed: true,
            changes: vec!["content".to_string()],
            retained: vec!["team".to_string()],
        };
        let ((), human) = crate::output::capture(async {
            emit_override_result(&changed, true, false).unwrap();
        })
        .await;
        assert!(human.contains("no changes were applied"));

        let ((), json) = crate::output::capture(async {
            emit_override_result(&changed, false, true).unwrap();
        })
        .await;
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["outcome"], "changed");
        assert_eq!(value["dry_run"], false);
        assert_eq!(value["targets"][0]["retained"][0], "team");

        let unchanged = BundleOverridePreview {
            changed: false,
            changes: Vec::new(),
            ..changed
        };
        let ((), human) = crate::output::capture(async {
            emit_override_result(&unchanged, true, false).unwrap();
        })
        .await;
        assert!(human.contains("already unchanged"));
    }
}

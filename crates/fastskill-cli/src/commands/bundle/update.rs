//! Explicit bundle replacement for `fastskill bundle update`.

use crate::commands::update::output::emit_bundle_update_result;
use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::messages;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::bundle::BundleService;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[derive(Debug, Clone)]
pub struct UpdateArgs {
    /// Installed bundle identity to update
    pub id: String,

    /// Replacement bundle ZIP artifact
    pub from: String,

    /// Check for changes without applying them
    pub check: bool,

    /// Validate and preview without changing managed state
    pub dry_run: bool,

    /// Emit one machine-readable lifecycle result
    pub json: bool,

    /// Trigger reindex after update
    pub reindex: bool,

    /// Skip reindex after update
    pub no_reindex: bool,

    /// Use a local artifact without provider or index access
    pub offline: bool,
}

impl IntoCommandSpec for UpdateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Replace an installed bundle release",
            syntax: Some("bundle update <BUNDLE_ID> --from <ARTIFACT> [OPTIONS]"),
            category: Some("skills-projects"),
            help_order: Some(40),
            examples: vec![
                "fastskill bundle update payments-team --from ./payments-team-2.0.0.zip",
                "fastskill bundle update payments-team --from ./payments-team-2.0.0.zip --dry-run",
            ],
            args: vec![
                ArgSpec {
                    name: "bundle-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Installed bundle identity to update",
                    ..Default::default()
                },
                ArgSpec {
                    name: "from",
                    kind: ArgKind::Option,
                    long: Some("from"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Replacement bundle ZIP artifact or HTTPS URL",
                    ..Default::default()
                },
                flag("check", "Check for changes without applying them"),
                flag(
                    "dry-run",
                    "Validate and preview without changing managed state",
                ),
                flag("json", "Emit one machine-readable lifecycle result"),
                flag("reindex", "Trigger reindex after update"),
                flag("no-reindex", "Skip reindex after update"),
                flag(
                    "offline",
                    "Use a local artifact without provider or index access",
                ),
            ],
            ..Default::default()
        }
    }
}

fn flag(name: &'static str, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Flag,
        long: Some(name),
        value_type: ArgValueType::Bool,
        cardinality: Cardinality::Optional,
        help,
        ..Default::default()
    }
}

impl FromArgValueMap for UpdateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            id: string_value(map, "bundle-id"),
            from: string_value(map, "from"),
            check: bool_value(map, "check"),
            dry_run: bool_value(map, "dry-run"),
            json: bool_value(map, "json"),
            reindex: bool_value(map, "reindex"),
            no_reindex: bool_value(map, "no-reindex"),
            offline: bool_value(map, "offline"),
        }
    }
}

fn string_value(map: &HashMap<String, ArgValue>, name: &str) -> String {
    match map.get(name) {
        Some(ArgValue::Str(value)) => value.clone(),
        _ => String::new(),
    }
}

fn bool_value(map: &HashMap<String, ArgValue>, name: &str) -> bool {
    matches!(map.get(name), Some(ArgValue::Bool(true)))
}

fn is_remote_artifact(value: &str) -> bool {
    value.starts_with("https://") || (cfg!(test) && value.starts_with("http://127.0.0.1"))
}

pub async fn execute_update(
    service: &FastSkillService,
    args: UpdateArgs,
    global: bool,
) -> CliResult<()> {
    validate_args(&args, global)?;
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

    let downloaded;
    let artifact = if is_remote_artifact(&args.from) {
        if args.offline {
            return Err(CliError::Validation(
                "--offline requires a local bundle artifact".to_string(),
            ));
        }
        let response = reqwest::get(&args.from)
            .await
            .map_err(|error| {
                CliError::InvalidSource(format!("Failed to download '{}': {error}", args.from))
            })?
            .error_for_status()
            .map_err(|error| {
                CliError::InvalidSource(format!("Failed to download '{}': {error}", args.from))
            })?;
        let bytes = response.bytes().await.map_err(|error| {
            CliError::InvalidSource(format!("Failed to read '{}': {error}", args.from))
        })?;
        downloaded = TempDir::new().map_err(CliError::Io)?;
        let path = downloaded.path().join("bundle.zip");
        fs::write(&path, bytes).map_err(CliError::Io)?;
        path
    } else if args.from.contains("://") {
        return Err(CliError::Validation(
            "Bundle URLs must use HTTPS. Download private artifacts with your authenticated tool, then pass the local ZIP."
                .to_string(),
        ));
    } else {
        PathBuf::from(&args.from)
    };

    let bundles = BundleService::new(root, service.config().skill_storage_path.clone());
    let preview = bundles
        .plan_update(&args.id, &artifact)
        .map_err(CliError::Service)?;
    if !args.json {
        for change in &preview.changes {
            crate::outln!("  {change}");
        }
    }
    if args.dry_run || args.check {
        if args.json {
            let indexing = crate::utils::reindex_utils::LifecycleIndexResult {
                outcome: "skipped",
                count: 0,
                diagnostic: Some("preview does not run derived indexing".to_string()),
            };
            emit_bundle_update_result(&preview, true, &indexing)?;
        } else if preview.changes.is_empty() {
            crate::outln!("Bundle {} is already unchanged", preview.id);
        }
        return Ok(());
    }

    let result = bundles
        .update(&args.id, &artifact)
        .map_err(CliError::Service)?;
    if !args.json {
        if result.unchanged {
            crate::outln!(
                "Bundle {}@{} is already unchanged",
                result.id,
                result.version
            );
        } else {
            crate::outln!("Updated bundle {}@{}", result.id, result.version);
            crate::outln!(
                "{}",
                messages::ok("Updated skill-project.toml and skills.lock")
            );
        }
    }
    let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
        service,
        "bundle update",
        args.reindex,
        args.no_reindex || args.offline,
        crate::config_file::load_auto_reindex_config(),
    )
    .await;
    if args.json {
        emit_bundle_update_result(&preview, false, &indexing)?;
    } else {
        crate::utils::reindex_utils::report_lifecycle_index_result(&indexing);
    }
    Ok(())
}

fn validate_args(args: &UpdateArgs, global: bool) -> CliResult<()> {
    if global {
        return Err(CliError::Validation(
            "Bundle operations require a project Manifest and do not support --global".to_string(),
        ));
    }
    if args.check && args.dry_run {
        return Err(CliError::Validation(
            "--check and --dry-run are mutually exclusive".to_string(),
        ));
    }
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    if args.offline && args.reindex {
        return Err(CliError::Validation(
            "--offline and --reindex cannot be used together".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use fastskill_core::ServiceConfig;

    fn args() -> UpdateArgs {
        UpdateArgs {
            id: "team".to_string(),
            from: "team.zip".to_string(),
            check: false,
            dry_run: false,
            json: false,
            reindex: false,
            no_reindex: true,
            offline: false,
        }
    }

    #[test]
    fn argument_map_requires_explicit_id_and_from_fields() {
        let args = UpdateArgs::from_arg_value_map(&HashMap::from([
            ("bundle-id".to_string(), ArgValue::Str("team".to_string())),
            ("from".to_string(), ArgValue::Str("team.zip".to_string())),
            ("check".to_string(), ArgValue::Bool(true)),
        ]));
        assert_eq!(args.id, "team");
        assert_eq!(args.from, "team.zip");
        assert!(args.check);

        let defaults = UpdateArgs::from_arg_value_map(&HashMap::new());
        assert!(defaults.id.is_empty());
        assert!(defaults.from.is_empty());
        assert!(!defaults.json);

        let spec = UpdateArgs::command_spec();
        assert_eq!(
            spec.syntax,
            Some("bundle update <BUNDLE_ID> --from <ARTIFACT> [OPTIONS]")
        );
        assert_eq!(spec.category, Some("skills-projects"));
        assert_eq!(spec.help_order, Some(40));
    }

    #[test]
    fn validation_rejects_global_and_conflicting_preview_flags() {
        let mut args = args();
        args.no_reindex = false;
        assert!(validate_args(&args, true).is_err());
        args.check = true;
        args.dry_run = true;
        assert!(validate_args(&args, false).is_err());

        args.check = false;
        args.dry_run = false;
        args.reindex = true;
        args.no_reindex = true;
        assert!(matches!(
            validate_args(&args, false),
            Err(CliError::Validation(message)) if message.contains("--reindex and --no-reindex")
        ));

        args.no_reindex = false;
        args.offline = true;
        assert!(matches!(
            validate_args(&args, false),
            Err(CliError::Validation(message)) if message.contains("--offline and --reindex")
        ));
    }

    #[tokio::test]
    async fn bundle_update_requires_a_project_and_rejects_unsupported_transports() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = tempfile::tempdir().unwrap();
        let original = env::current_dir().ok();
        let _guard = fastskill_core::test_utils::DirGuard(original);
        env::set_current_dir(root.path()).unwrap();
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();

        assert!(matches!(
            execute_update(&service, args(), false).await,
            Err(CliError::Config(message)) if message.contains("skill-project.toml")
        ));

        fs::write(root.path().join("skill-project.toml"), "[dependencies]\n").unwrap();
        let mut unsupported = args();
        unsupported.from = "ftp://example.com/team.zip".to_string();
        assert!(matches!(
            execute_update(&service, unsupported, false).await,
            Err(CliError::Validation(message)) if message.contains("must use HTTPS")
        ));

        let mut offline_remote = args();
        offline_remote.from = "https://example.com/team.zip".to_string();
        offline_remote.offline = true;
        assert!(matches!(
            execute_update(&service, offline_remote, false).await,
            Err(CliError::Validation(message)) if message.contains("local bundle artifact")
        ));
    }
}

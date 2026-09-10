//! Explicit bundle removal for `fastskill bundle remove`.

use crate::commands::remove::{confirmation::confirm_removal, output::emit_bundle_removal};
use crate::error::{manifest_required_message, CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::bundle::BundleService;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;

#[derive(Debug, Clone)]
pub struct RemoveArgs {
    /// Installed bundle identity to remove
    pub id: String,

    /// Remove without confirmation
    pub force: bool,

    /// Trigger reindex after removal
    pub reindex: bool,

    /// Skip reindex after removal
    pub no_reindex: bool,

    /// Validate and preview without changing managed state
    pub dry_run: bool,

    /// Emit one machine-readable lifecycle result
    pub json: bool,
}

impl IntoCommandSpec for RemoveArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Remove an installed bundle and its ownership",
            syntax: Some("bundle remove <BUNDLE_ID> [OPTIONS]"),
            category: Some("skills-projects"),
            help_order: Some(50),
            examples: vec![
                "fastskill bundle remove payments-team",
                "fastskill bundle remove payments-team --dry-run --json",
            ],
            args: vec![
                ArgSpec {
                    name: "bundle-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Installed bundle identity to remove",
                    ..Default::default()
                },
                flag("force", Some('f'), "Remove without confirmation"),
                flag("reindex", None, "Trigger reindex after removal"),
                flag("no-reindex", None, "Skip reindex after removal"),
                flag(
                    "dry-run",
                    None,
                    "Validate and preview without changing managed state",
                ),
                flag("json", None, "Emit one machine-readable lifecycle result"),
            ],
            ..Default::default()
        }
    }
}

fn flag(name: &'static str, short: Option<char>, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Flag,
        long: Some(name),
        short,
        value_type: ArgValueType::Bool,
        cardinality: Cardinality::Optional,
        help,
        ..Default::default()
    }
}

impl FromArgValueMap for RemoveArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            id: match map.get("bundle-id") {
                Some(ArgValue::Str(value)) => value.clone(),
                _ => String::new(),
            },
            force: bool_value(map, "force"),
            reindex: bool_value(map, "reindex"),
            no_reindex: bool_value(map, "no-reindex"),
            dry_run: bool_value(map, "dry-run"),
            json: bool_value(map, "json"),
        }
    }
}

fn bool_value(map: &HashMap<String, ArgValue>, name: &str) -> bool {
    matches!(map.get(name), Some(ArgValue::Bool(true)))
}

pub async fn execute_remove(
    service: &FastSkillService,
    args: RemoveArgs,
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
    if !args.dry_run
        && !args.force
        && (args.json || crate::output::mode() == crate::output::Mode::Capture)
    {
        return Err(CliError::Validation(
            "Non-interactive removal requires --force; use --dry-run to preview changes"
                .to_string(),
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
    let bundles = BundleService::new(root, service.config().skill_storage_path.clone());
    let preview = bundles
        .preview_remove(&args.id)
        .map_err(CliError::Service)?;
    if args.dry_run {
        return emit_bundle_removal(&preview, true, args.json);
    }
    if !confirm_removal(&[format!("bundle {}", args.id)], args.force)? {
        crate::outln!("Removal cancelled.");
        return Ok(());
    }

    bundles.remove(&args.id).map_err(CliError::Service)?;
    if !args.json {
        crate::outln!("Removed bundle: {}", args.id);
        crate::outln!(
            "{}",
            crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
        );
    }
    crate::utils::reindex_utils::maybe_auto_reindex(
        service,
        "bundle remove",
        args.reindex,
        args.no_reindex,
        crate::config_file::load_auto_reindex_config(),
        false,
    )
    .await?;
    if args.json {
        emit_bundle_removal(&preview, false, true)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::await_holding_lock)]
mod tests {
    use super::*;
    use fastskill_core::ServiceConfig;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn args(id: &str) -> RemoveArgs {
        RemoveArgs {
            id: id.to_string(),
            force: false,
            reindex: false,
            no_reindex: true,
            dry_run: false,
            json: false,
        }
    }

    fn build_bundle(root: &Path) -> PathBuf {
        let author = root.join("author");
        fs::create_dir_all(author.join("skills/demo")).unwrap();
        fs::write(
            author.join("skill-project.toml"),
            "[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"1.0.0\"\n[bundle.members.demo]\noverridable = false\n[dependencies]\ndemo = \"1.0.0\"\n",
        )
        .unwrap();
        fs::write(
            author.join("skills/demo/SKILL.md"),
            "---\nname: demo\nversion: \"1.0.0\"\ndescription: demo\n---\n# demo\n",
        )
        .unwrap();
        BundleService::new(&author, author.join("skills"))
            .build(&author.join("dist"))
            .unwrap()
            .artifact
    }

    #[test]
    fn argument_map_uses_bundle_id_positional() {
        let args = RemoveArgs::from_arg_value_map(&HashMap::from([
            (
                "bundle-id".to_string(),
                ArgValue::Str("payments-team".to_string()),
            ),
            ("force".to_string(), ArgValue::Bool(true)),
        ]));
        assert_eq!(args.id, "payments-team");
        assert!(args.force);

        let defaults = RemoveArgs::from_arg_value_map(&HashMap::new());
        assert!(defaults.id.is_empty());
        assert!(!defaults.force);

        let spec = RemoveArgs::command_spec();
        assert_eq!(spec.syntax, Some("bundle remove <BUNDLE_ID> [OPTIONS]"));
        assert_eq!(spec.category, Some("skills-projects"));
        assert_eq!(spec.help_order, Some(50));
    }

    #[tokio::test]
    async fn bundle_remove_rejects_scope_flag_conflicts_and_noninteractive_confirmation() {
        let root = tempfile::tempdir().unwrap();
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();

        assert!(matches!(
            execute_remove(&service, args("team"), true).await,
            Err(CliError::Validation(message)) if message.contains("do not support --global")
        ));

        let mut conflicting = args("team");
        conflicting.reindex = true;
        assert!(matches!(
            execute_remove(&service, conflicting, false).await,
            Err(CliError::Validation(message)) if message.contains("--reindex and --no-reindex")
        ));

        let mut noninteractive = args("team");
        noninteractive.json = true;
        assert!(matches!(
            execute_remove(&service, noninteractive, false).await,
            Err(CliError::Validation(message)) if message.contains("requires --force")
        ));
    }

    #[tokio::test]
    async fn bundle_remove_requires_a_project_manifest() {
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
        let mut remove = args("team");
        remove.force = true;

        assert!(matches!(
            execute_remove(&service, remove, false).await,
            Err(CliError::Config(message)) if message.contains("skill-project.toml")
        ));
    }

    #[tokio::test]
    async fn bundle_remove_previews_and_applies_human_and_json_lifecycles() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("skill-project.toml"), "[dependencies]\n").unwrap();
        let artifact = build_bundle(root.path());
        let bundles = BundleService::new(root.path(), root.path().join("skills"));
        bundles.install(&artifact).unwrap();
        let original = env::current_dir().ok();
        let _guard = fastskill_core::test_utils::DirGuard(original);
        env::set_current_dir(root.path()).unwrap();
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();

        let mut preview = args("team");
        preview.dry_run = true;
        preview.json = true;
        let (result, rendered) =
            crate::output::capture(execute_remove(&service, preview, false)).await;
        result.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&rendered).unwrap()["dry_run"],
            true
        );

        let mut human = args("team");
        human.force = true;
        let (result, rendered) =
            crate::output::capture(execute_remove(&service, human, false)).await;
        result.unwrap();
        assert!(rendered.contains("Removed bundle: team"));

        bundles.install(&artifact).unwrap();
        let mut json = args("team");
        json.force = true;
        json.json = true;
        let (result, rendered) =
            crate::output::capture(execute_remove(&service, json, false)).await;
        result.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&rendered).unwrap()["dry_run"],
            false
        );
    }
}

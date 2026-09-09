//! Remove command implementation

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

#[path = "remove/confirmation.rs"]
mod confirmation;
#[path = "remove/global.rs"]
mod global_remove;
#[path = "remove/output.rs"]
mod output;
use confirmation::confirm_removal;

/// Uninstall skills (only way to stop using skills)
///
/// This is the only command to completely stop using skills after the removal of 'disable'.
///
/// Behavior:
/// - For manifest-managed projects: Removes from skill-project.toml [dependencies] and local installation
/// - For local-only skills: Removes from local installation only
/// - Always updates skills.lock to reflect removals (when manifest exists)
///
/// Use this command when you want to completely stop using a skill.
#[derive(Debug)]
pub struct RemoveArgs {
    /// Skill IDs to remove
    pub skill_ids: Vec<String>,

    /// Force removal without confirmation
    pub force: bool,

    /// Remove an installed bundle and its ownership
    pub bundle: Option<String>,

    /// Skills directory path (overrides default discovery)
    #[allow(dead_code)]
    pub skills_dir: Option<std::path::PathBuf>,

    /// Trigger reindex after removal (overrides config)
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
            summary: "Uninstall skills (removes from manifest and local installation)",
            syntax: Some("remove <SKILL_ID>... [OPTIONS]"),
            category: Some("packages"),
            examples: vec![
                "fastskill remove pptx",
                "fastskill remove pptx docx --force",
            ],
            args: vec![
                ArgSpec {
                    name: "skill-ids",
                    long: None,
                    short: None,
                    help: "Skill IDs to remove",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "force",
                    long: Some("force"),
                    short: Some('f'),
                    help: "Force removal without confirmation",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "bundle",
                    long: Some("bundle"),
                    short: None,
                    help: "Installed bundle identity to remove",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "skills-dir",
                    long: Some("skills-dir"),
                    short: None,
                    help: "Skills directory path (overrides default discovery)",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "reindex",
                    long: Some("reindex"),
                    short: None,
                    help: "Trigger reindex after removal (overrides config)",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-reindex",
                    long: Some("no-reindex"),
                    short: None,
                    help: "Skip reindex after removal",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    long: Some("dry-run"),
                    help: "Validate and preview without changing managed state",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    long: Some("json"),
                    help: "Emit one machine-readable lifecycle result",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for RemoveArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        RemoveArgs {
            skill_ids: match map.get("skill-ids") {
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
            force: matches!(map.get("force"), Some(ArgValue::Bool(true))),
            bundle: map.get("bundle").and_then(|value| {
                if let ArgValue::Str(bundle) = value {
                    Some(bundle.clone())
                } else {
                    None
                }
            }),
            skills_dir: map.get("skills-dir").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
            reindex: matches!(map.get("reindex"), Some(ArgValue::Bool(true))),
            no_reindex: matches!(map.get("no-reindex"), Some(ArgValue::Bool(true))),
            dry_run: matches!(map.get("dry-run"), Some(ArgValue::Bool(true))),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Unregister skill from the service
async fn unregister_skill_from_service(
    service: &FastSkillService,
    skill_id: fastskill_core::SkillId,
) -> CliResult<()> {
    match service.skill_manager().unregister_skill(&skill_id).await {
        Ok(_) => Ok(()),
        Err(fastskill_core::ServiceError::SkillNotFound(_)) => {
            // Skill not in registry, but that's okay - we still want to clean up files
            Ok(())
        }
        Err(e) => Err(CliError::Service(fastskill_core::ServiceError::Custom(
            format!("Failed to remove skill {}: {}", skill_id, e),
        ))),
    }
}

/// Remove skill from vector index
async fn remove_from_vector_index(service: &FastSkillService, skill_id: &str) {
    if let Some(vector_index_service) = service.vector_index_service() {
        if let Err(e) = vector_index_service.remove_skill(skill_id).await {
            tracing::warn!(
                "Failed to remove skill {} from vector index: {}",
                skill_id,
                e
            );
        }
    }
}

pub async fn execute_remove(
    service: &FastSkillService,
    args: RemoveArgs,
    global: bool,
) -> CliResult<()> {
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
    let reindex = args.reindex;
    let no_reindex = args.no_reindex;

    if let Some(bundle) = &args.bundle {
        if global {
            return Err(CliError::Validation(
                "Bundle removal requires a project Manifest and does not support --global"
                    .to_string(),
            ));
        }
        if !args.skill_ids.is_empty() {
            return Err(CliError::Validation(
                "Use either skill IDs or --bundle <bundle-id>, not both".to_string(),
            ));
        }
        let current = env::current_dir().map_err(|error| {
            CliError::Config(format!("Failed to determine current directory: {error}"))
        })?;
        let project = resolve_project_file(&current);
        if !project.found {
            return Err(CliError::Config(
                "skill-project.toml not found in this directory or any parent".to_string(),
            ));
        }
        let root = project.path.parent().unwrap_or(Path::new("."));
        let bundles = fastskill_core::core::bundle::BundleService::new(
            root,
            service.config().skill_storage_path.clone(),
        );
        let preview = bundles.preview_remove(bundle).map_err(CliError::Service)?;
        if args.dry_run {
            return output::emit_bundle_removal(&preview, true, args.json);
        }
        if !confirm_removal(&[format!("bundle {bundle}")], args.force)? {
            crate::outln!("Removal cancelled.");
            return Ok(());
        }
        bundles.remove(bundle).map_err(CliError::Service)?;
        if !args.json {
            crate::outln!("Removed bundle: {bundle}");
            crate::outln!(
                "{}",
                crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
            );
        }
        let auto_reindex = crate::config_file::load_auto_reindex_config();
        crate::utils::reindex_utils::maybe_auto_reindex(
            service,
            "remove",
            reindex,
            no_reindex,
            auto_reindex,
            false,
        )
        .await?;
        if args.json {
            output::emit_bundle_removal(&preview, false, true)?;
        }
        return Ok(());
    }

    // Validate inputs
    if args.skill_ids.is_empty() {
        return Err(CliError::Config("No skill IDs provided".to_string()));
    }

    if !global {
        for raw_id in &args.skill_ids {
            fastskill_core::SkillId::new(raw_id.clone())
                .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {raw_id}")))?;
        }
        let current = env::current_dir().map_err(|error| {
            CliError::Config(format!("Failed to determine current directory: {error}"))
        })?;
        let project = resolve_project_file(&current);
        if !project.found {
            return Err(CliError::Config(
                "skill-project.toml not found in this directory or any parent".to_string(),
            ));
        }
        let root = project.path.parent().unwrap_or(Path::new("."));
        let removal = fastskill_core::core::project_removal::ProjectRemovalService::new(
            root,
            service.config().skill_storage_path.clone(),
        );
        let preview = removal
            .preview(&args.skill_ids)
            .map_err(CliError::Service)?;
        if args.dry_run {
            return output::emit_project_removal(&args.skill_ids, &preview, true, args.json);
        }
        if !confirm_removal(&args.skill_ids, args.force)? {
            crate::outln!("Removal cancelled.");
            return Ok(());
        }
        let plan = removal.remove(&args.skill_ids).map_err(CliError::Service)?;
        for id in &plan.delete_files {
            if let Ok(skill_id) = fastskill_core::SkillId::new(id.clone()) {
                if let Err(error) = unregister_skill_from_service(service, skill_id).await {
                    tracing::warn!(
                        "removed '{id}' from managed state, but the in-memory registry could not be refreshed: {error}"
                    );
                }
            }
            remove_from_vector_index(service, id).await;
        }
        for id in &plan.remove_manifest_dependencies {
            if args.json {
                continue;
            }
            if plan.retained_files.contains(id) {
                crate::outln!("Removed direct requirement: {id} (files retained by another owner)");
            } else {
                crate::outln!("Removed skill: {id}");
            }
        }
        for id in &plan.unchanged {
            if !args.json {
                crate::outln!("Skill '{id}' was already absent; no changes made");
            }
        }
        if !args.json
            && (!plan.remove_manifest_dependencies.is_empty()
                || !plan.remove_lock_entries.is_empty())
        {
            crate::outln!(
                "{}",
                crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
            );
        }
        let auto_reindex = crate::config_file::load_auto_reindex_config();
        crate::utils::reindex_utils::maybe_auto_reindex(
            service,
            "remove",
            reindex,
            no_reindex,
            auto_reindex,
            false,
        )
        .await?;
        if args.json {
            output::emit_project_removal(&args.skill_ids, &plan, false, true)?;
        }
        return Ok(());
    }

    if args.dry_run {
        let plan = global_remove::preview(service, &args.skill_ids)?;
        return output::emit_global_removal_plan(&args.skill_ids, &plan, true, args.json);
    }

    // Get user confirmation
    if !confirm_removal(&args.skill_ids, args.force)? {
        crate::outln!("Removal cancelled.");
        return Ok(());
    }

    let plan = global_remove::remove(service, &args.skill_ids).await?;
    let removed_count = plan.remove_lock_entries.len();
    if !args.json {
        for id in &plan.remove_roots {
            if plan.retained_files.contains(id) {
                crate::outln!("Removed global requirement: {id} (retained by another root)");
            } else {
                crate::outln!("Removed global skill: {id}");
            }
        }
        for id in &plan.unchanged {
            crate::outln!("Global skill '{id}' was already absent; no changes made");
        }
    }

    // Display success message
    if removed_count > 0 && !args.json {
        crate::outln!(
            "{}",
            crate::utils::messages::ok("Updated global-skills.lock")
        );
    }

    let auto_reindex = crate::config_file::load_auto_reindex_config();
    crate::utils::reindex_utils::maybe_auto_reindex(
        service,
        "remove",
        reindex,
        no_reindex,
        auto_reindex,
        false,
    )
    .await?;
    if args.json {
        output::emit_global_removal_plan(&args.skill_ids, &plan, false, true)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock
)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fastskill_core::core::lock::{
        GlobalLockedSkillEntry, GlobalSkillsLock, ProjectLockedSkillEntry, ProjectSkillsLock,
    };
    use fastskill_core::core::manifest::{DependenciesSection, DependencySpec, SkillProjectToml};
    use fastskill_core::core::origin::{Origin, Resolved};
    use fastskill_core::test_utils::DirGuard;
    use fastskill_core::ServiceConfig;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    struct EnvGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(name: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn args(ids: &[&str]) -> RemoveArgs {
        RemoveArgs {
            skill_ids: ids.iter().map(|id| (*id).to_string()).collect(),
            force: true,
            bundle: None,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
            dry_run: false,
            json: false,
        }
    }

    #[test]
    fn remove_argument_map_ignores_wrong_typed_values() {
        let parsed = RemoveArgs::from_arg_value_map(&HashMap::from([
            (
                "skill-ids".to_string(),
                ArgValue::List(vec![ArgValue::Str("one".to_string()), ArgValue::Bool(true)]),
            ),
            ("bundle".to_string(), ArgValue::Bool(true)),
            ("skills-dir".to_string(), ArgValue::Bool(true)),
        ]));
        assert_eq!(parsed.skill_ids, vec!["one"]);
        assert!(parsed.bundle.is_none());
        assert!(parsed.skills_dir.is_none());
    }

    #[tokio::test]
    async fn remove_rejects_conflicting_modes_before_project_discovery() {
        let root = TempDir::new().unwrap();
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();

        let mut conflict = args(&["demo"]);
        conflict.reindex = true;
        conflict.no_reindex = true;
        assert!(execute_remove(&service, conflict, false)
            .await
            .unwrap_err()
            .to_string()
            .contains("cannot be used together"));

        let mut global_bundle = args(&[]);
        global_bundle.bundle = Some("team".to_string());
        assert!(execute_remove(&service, global_bundle, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not support --global"));

        let mut mixed = args(&["demo"]);
        mixed.bundle = Some("team".to_string());
        assert!(execute_remove(&service, mixed, false)
            .await
            .unwrap_err()
            .to_string()
            .contains("either skill IDs or --bundle"));
    }

    #[tokio::test]
    async fn bundle_remove_reports_a_missing_project_before_planning() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _directory = DirGuard(std::env::current_dir().ok());
        std::env::set_current_dir(root.path()).unwrap();
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();
        let mut remove = args(&[]);
        remove.bundle = Some("team".to_string());

        let error = execute_remove(&service, remove, false).await.unwrap_err();

        assert!(matches!(error, CliError::Config(message) if message.contains("not found")));
    }

    #[tokio::test]
    async fn project_remove_reports_a_missing_project_before_planning() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _directory = DirGuard(std::env::current_dir().ok());
        std::env::set_current_dir(root.path()).unwrap();
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();

        let error = execute_remove(&service, args(&["managed"]), false)
            .await
            .unwrap_err();

        assert!(matches!(error, CliError::Config(message) if message.contains("not found")));
    }

    #[tokio::test]
    async fn global_absent_remove_reports_noop_in_preview_and_apply_json() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", root.path().join("config"));
        let service = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();

        let mut preview = args(&["absent"]);
        preview.dry_run = true;
        preview.json = true;
        let (preview_result, preview_output) =
            crate::output::capture(async { execute_remove(&service, preview, true).await }).await;
        preview_result.unwrap();
        let preview_json: serde_json::Value = serde_json::from_str(&preview_output).unwrap();
        assert_eq!(preview_json["outcome"], "unchanged");
        assert_eq!(preview_json["dry_run"], true);

        let mut apply = args(&["absent"]);
        apply.json = true;
        let (apply_result, apply_output) =
            crate::output::capture(async { execute_remove(&service, apply, true).await }).await;
        apply_result.unwrap();
        let apply_json: serde_json::Value = serde_json::from_str(&apply_output).unwrap();
        assert_eq!(apply_json["outcome"], "unchanged");
        assert_eq!(apply_json["dry_run"], false);

        if let Some(value) = previous {
            std::env::set_var("XDG_CONFIG_HOME", value);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[tokio::test]
    async fn global_remove_applies_and_reports_a_managed_root() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = TempDir::new().unwrap();
        let _xdg = EnvGuard::set("XDG_CONFIG_HOME", &root.path().join("config"));
        let skills = root.path().join("skills");
        let installed = skills.join("managed");
        fs::create_dir_all(&installed).unwrap();
        fs::write(
            installed.join("SKILL.md"),
            "---\nname: managed\nversion: 1.0.0\ndescription: fixture\n---\nmanaged\n",
        )
        .unwrap();
        let origin = Origin::Local {
            path: root.path().join("source/managed"),
            editable: false,
        };
        let mut lock = GlobalSkillsLock::new_empty();
        lock.covered_roots.push("managed".to_string());
        lock.skills.push(GlobalLockedSkillEntry {
            id: "managed".to_string(),
            name: "managed".to_string(),
            origin,
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum: Some(fastskill_core::core::install::content_digest(&installed).unwrap()),
            },
            dependencies: Vec::new(),
            groups: Vec::new(),
            installed_at: Utc::now(),
            last_checked_at: None,
            last_updated_at: None,
        });
        lock.save_to_file(&root.path().join("config/fastskill/global-skills.lock"))
            .unwrap();
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: skills,
            skill_cache_root: Some(root.path().join("cache")),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();

        let mut cancelled = args(&["managed"]);
        cancelled.force = false;
        let (result, output) =
            crate::output::capture(async { execute_remove(&service, cancelled, true).await }).await;
        assert!(result.unwrap_err().to_string().contains("requires --force"));
        assert!(output.is_empty());
        assert!(installed.exists());

        let (result, output) = crate::output::capture(async {
            execute_remove(&service, args(&["managed"]), true).await
        })
        .await;
        result.unwrap();

        assert!(output.contains("Removed global skill: managed"));
        assert!(output.contains("Updated global-skills.lock"));
        assert!(!installed.exists());
    }

    #[tokio::test]
    async fn test_execute_remove_empty_args() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec![],
            force: false,
            bundle: None,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
            dry_run: false,
            json: false,
        };

        let result = execute_remove(&service, args, false).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("No skill IDs provided"));
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_execute_remove_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["invalid@skill@id".to_string()],
            force: true,
            bundle: None,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
            dry_run: false,
            json: false,
        };

        let result = execute_remove(&service, args, false).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("Invalid skill ID format"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_execute_remove_nonexistent_skill() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["nonexistent@1.0.0".to_string()],
            force: true,
            bundle: None,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
            dry_run: false,
            json: false,
        };

        // This should fail because the skill doesn't exist
        let result = execute_remove(&service, args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_remove_applies_a_valid_plan_and_reports_changed_and_absent_targets() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let skills_dir = temp_dir.path().join(".claude/skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_content =
            "---\nname: test-skill\nversion: 1.0.0\ndescription: fixture\n---\ncontent\n";
        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();
        SkillProjectToml {
            schema_version: None,
            metadata: None,
            dependencies: Some(DependenciesSection {
                dependencies: HashMap::from([(
                    "test-skill".to_string(),
                    DependencySpec::Version("1.0.0".to_string()),
                )]),
            }),
            tool: None,
        }
        .save_to_file(&temp_dir.path().join("skill-project.toml"))
        .unwrap();
        let mut lock = ProjectSkillsLock::new_empty();
        lock.covered_roots.push("test-skill".to_string());
        lock.skills.push(ProjectLockedSkillEntry {
            id: "test-skill".to_string(),
            name: "test-skill".to_string(),
            origin: Origin::Local {
                path: temp_dir.path().join("source/test-skill"),
                editable: false,
            },
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum: Some(
                    fastskill_core::core::project_removal::managed_tree_digest(&skill_dir).unwrap(),
                ),
            },
            dependencies: Vec::new(),
            groups: Vec::new(),
            depth: 0,
            parent_skill: None,
            required_by: Vec::new(),
        });
        lock.save_to_file(&temp_dir.path().join("skills.lock"))
            .unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let mut cancelled = args(&["test-skill"]);
        cancelled.force = false;
        let (result, output) =
            crate::output::capture(async { execute_remove(&service, cancelled, false).await })
                .await;
        assert!(result.unwrap_err().to_string().contains("requires --force"));
        assert!(output.is_empty());
        assert!(skill_dir.exists());

        let (result, output) = crate::output::capture(async {
            execute_remove(&service, args(&["test-skill", "absent"]), false).await
        })
        .await;
        result.unwrap();

        assert!(output.contains("Removed skill: test-skill"));
        assert!(output.contains("already absent"));
        assert!(!skill_dir.exists());
        let manifest =
            SkillProjectToml::load_from_file(&temp_dir.path().join("skill-project.toml")).unwrap();
        assert!(manifest.dependencies.unwrap().dependencies.is_empty());
        assert!(
            ProjectSkillsLock::load_from_file(&temp_dir.path().join("skills.lock"))
                .unwrap()
                .skills
                .is_empty()
        );
    }
}

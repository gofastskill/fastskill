//! Update command - updates skills in the configured skills directory

use crate::config::{create_service_config, resolve_skills_storage_directory};
use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::messages;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::{
    lock::ProjectSkillsLock, manifest::SkillProjectToml, project::resolve_project_file,
};
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[path = "update/global.rs"]
pub(crate) mod global;
#[path = "update/options.rs"]
mod options;
#[path = "update/output.rs"]
mod output;
use global::execute_update_global;
use options::{controlled_origin, validate_update_args};
use output::print_update_json;

/// Update one declared skill or every declared skill from its recorded origin.
/// Check and dry-run modes resolve the same candidates without changing state.
#[derive(Debug, Clone)]
pub struct UpdateArgs {
    /// Skill ID to update (if not specified, updates all)
    skill_id: Option<String>,

    /// Check for updates without installing
    check: bool,

    /// Show what would be updated without actually updating
    dry_run: bool,

    /// Emit one machine-readable lifecycle result
    json: bool,

    /// Update to specific version
    version: Option<String>,

    /// Update from specific source
    source: Option<String>,

    /// Configured repository to use for a repository-backed target
    repository: Option<String>,

    /// Installed bundle identity to update
    bundle: Option<String>,

    /// Replacement bundle artifact
    from: Option<String>,

    /// Update strategy: latest, patch, minor, major
    strategy: String,

    /// Whether --strategy was explicitly provided instead of defaulted.
    strategy_explicit: bool,

    /// Trigger reindex after update (overrides config)
    reindex: bool,

    /// Skip reindex after update
    no_reindex: bool,

    /// Use verified local sources and cached artifacts only
    offline: bool,
}

impl IntoCommandSpec for UpdateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Update skills to latest versions",
            syntax: Some("update [SKILL_ID] [OPTIONS]"),
            category: Some("packages"),
            examples: vec![
                "fastskill update",
                "fastskill update pptx",
                "fastskill update --check",
            ],
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Skill ID to update (if not specified, updates all)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "check",
                    kind: ArgKind::Flag,
                    long: Some("check"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Check for updates without installing",
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    kind: ArgKind::Flag,
                    long: Some("dry-run"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Show what would be updated without actually updating",
                    ..Default::default()
                },
                ArgSpec {
                    name: "to-version",
                    kind: ArgKind::Option,
                    long: Some("to-version"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Update to specific version",
                    ..Default::default()
                },
                ArgSpec {
                    name: "source",
                    kind: ArgKind::Option,
                    long: Some("source"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Deprecated alias of --repository",
                    ..Default::default()
                },
                ArgSpec {
                    name: "repository",
                    kind: ArgKind::Option,
                    long: Some("repository"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Use this configured repository for one repository-backed skill",
                    ..Default::default()
                },
                ArgSpec {
                    name: "bundle",
                    kind: ArgKind::Option,
                    long: Some("bundle"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Update this installed bundle",
                    ..Default::default()
                },
                ArgSpec {
                    name: "from",
                    kind: ArgKind::Option,
                    long: Some("from"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Replacement bundle ZIP artifact",
                    ..Default::default()
                },
                ArgSpec {
                    name: "strategy",
                    kind: ArgKind::Option,
                    long: Some("strategy"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    help: "Update strategy: latest, patch, minor, major",
                    ..Default::default()
                },
                ArgSpec {
                    name: "reindex",
                    kind: ArgKind::Flag,
                    long: Some("reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Trigger reindex after update (overrides config)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-reindex",
                    kind: ArgKind::Flag,
                    long: Some("no-reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Skip reindex after update",
                    ..Default::default()
                },
                ArgSpec {
                    name: "offline",
                    kind: ArgKind::Flag,
                    long: Some("offline"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Use verified local sources and cached artifacts only",
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

fn opt_str(v: &ArgValue) -> Option<String> {
    if let ArgValue::Str(s) = v {
        Some(s.clone())
    } else {
        None
    }
}

fn is_remote_bundle_artifact(value: &str) -> bool {
    value.starts_with("https://") || (cfg!(test) && value.starts_with("http://127.0.0.1"))
}

#[allow(clippy::panic)]
impl FromArgValueMap for UpdateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            skill_id: map.get("skill-id").and_then(opt_str),
            check: matches!(map.get("check"), Some(ArgValue::Bool(true))),
            dry_run: matches!(map.get("dry-run"), Some(ArgValue::Bool(true))),
            version: map.get("to-version").and_then(opt_str),
            source: map.get("source").and_then(opt_str),
            repository: map.get("repository").and_then(opt_str),
            bundle: map.get("bundle").and_then(opt_str),
            from: map.get("from").and_then(opt_str),
            strategy: map
                .get("strategy")
                .and_then(opt_str)
                .unwrap_or_else(|| "latest".to_string()),
            strategy_explicit: map.contains_key("strategy"),
            reindex: matches!(map.get("reindex"), Some(ArgValue::Bool(true))),
            no_reindex: matches!(map.get("no-reindex"), Some(ArgValue::Bool(true))),
            offline: matches!(map.get("offline"), Some(ArgValue::Bool(true))),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

pub async fn execute_update(
    args: UpdateArgs,
    global: bool,
    skills_dir_override: Option<PathBuf>,
) -> CliResult<()> {
    validate_update_args(&args)?;
    if global && skills_dir_override.is_some() {
        return Err(CliError::Validation(
            "--global and --skills-dir cannot be combined".to_string(),
        ));
    }
    if args.source.is_some() {
        eprintln!("warning: --source is deprecated; use --repository");
    }
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    if args.bundle.is_some() || args.from.is_some() {
        if global {
            return Err(CliError::Validation(
                "Bundle updates require a project Manifest and do not support --global".to_string(),
            ));
        }
        let bundle = args.bundle.as_deref().ok_or_else(|| {
            CliError::Validation("--from requires --bundle <bundle-id>".to_string())
        })?;
        let artifact = args.from.as_deref().ok_or_else(|| {
            CliError::Validation("--bundle requires --from <bundle.zip>".to_string())
        })?;
        if args.skill_id.is_some() {
            return Err(CliError::Validation(
                "A bundle update does not accept a skill ID positional argument".to_string(),
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
        let downloaded_artifact;
        let artifact_path = if is_remote_bundle_artifact(artifact) {
            let response = reqwest::get(artifact)
                .await
                .map_err(|error| {
                    CliError::InvalidSource(format!("Failed to download '{artifact}': {error}"))
                })?
                .error_for_status()
                .map_err(|error| {
                    CliError::InvalidSource(format!("Failed to download '{artifact}': {error}"))
                })?;
            let bytes = response.bytes().await.map_err(|error| {
                CliError::InvalidSource(format!("Failed to read '{artifact}': {error}"))
            })?;
            downloaded_artifact = TempDir::new().map_err(CliError::Io)?;
            let path = downloaded_artifact.path().join("bundle.zip");
            fs::write(&path, bytes).map_err(CliError::Io)?;
            path
        } else if artifact.contains("://") {
            return Err(CliError::Validation(
                "Bundle URLs must use HTTPS. Download private artifacts with your authenticated tool, then pass the local ZIP."
                    .to_string(),
            ));
        } else {
            PathBuf::from(artifact)
        };
        let skills_directory = match skills_dir_override {
            Some(path) => path,
            None => resolve_skills_storage_directory(false)?,
        };
        let service =
            fastskill_core::core::bundle::BundleService::new(root, skills_directory.clone());
        let preview = service
            .plan_update(bundle, &artifact_path)
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
                output::emit_bundle_update_result(&preview, true, &indexing)?;
            } else if preview.changes.is_empty() {
                crate::outln!("Bundle {} is already unchanged", preview.id);
            }
            return Ok(());
        }
        let result = service
            .update(bundle, &artifact_path)
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
        let indexing = bundle_update_indexing(&args, skills_directory).await;
        if args.json {
            output::emit_bundle_update_result(&preview, false, &indexing)?;
        } else {
            crate::utils::reindex_utils::report_lifecycle_index_result(&indexing);
        }
        return Ok(());
    }
    if global {
        execute_update_global(args, skills_dir_override).await?;
    } else {
        execute_update_project(args, skills_dir_override).await?;
    }

    Ok(())
}

async fn bundle_update_indexing(
    args: &UpdateArgs,
    skills_directory: PathBuf,
) -> crate::utils::reindex_utils::LifecycleIndexResult {
    let failed = |diagnostic: String| crate::utils::reindex_utils::LifecycleIndexResult {
        outcome: "failed",
        count: 0,
        diagnostic: Some(diagnostic),
    };
    let config = match create_service_config(false, Some(skills_directory)) {
        Ok(config) => config,
        Err(error) => return failed(format!("Index setup failed after bundle update: {error}")),
    };
    let mut service = match FastSkillService::new(config).await {
        Ok(service) => service,
        Err(error) => return failed(format!("Index setup failed after bundle update: {error}")),
    };
    if let Err(error) = service.initialize().await {
        return failed(format!("Index setup failed after bundle update: {error}"));
    }
    let service = match crate::config::inject_edge_services(service) {
        Ok(service) => service,
        Err(error) => return failed(format!("Index setup failed after bundle update: {error}")),
    };
    crate::utils::reindex_utils::lifecycle_reindex_result(
        &service,
        "bundle update",
        args.reindex,
        args.no_reindex,
        crate::config_file::load_auto_reindex_config(),
    )
    .await
}

/// Resolve and validate every selected project update before applying any of it.
/// The same prepared candidates drive check, preview, and apply output so the
/// reported target versions are the versions that will actually be installed.
async fn execute_update_project(
    args: UpdateArgs,
    skills_dir_override: Option<PathBuf>,
) -> CliResult<()> {
    if !args.json {
        crate::outln!("Updating skills...");
        crate::outln!();
    }

    // T034: Resolve skill-project.toml from project root
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    let project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;

    // Validate context
    let context = project_file_result.context;
    project
        .validate_for_context(context)
        .map_err(|e| CliError::Config(format!("skill-project.toml validation failed: {}", e)))?;

    // Convert dependencies to SkillEntry format. Local origins are recorded
    // relative to the Manifest, so they resolve against its directory.
    let mut entries = project
        .to_skill_entries(
            project_file_path
                .parent()
                .unwrap_or(std::path::Path::new(".")),
        )
        .map_err(|e| CliError::Config(format!("Failed to parse dependencies: {}", e)))?;

    // Filter by skill_id if specified
    if let Some(skill_id) = &args.skill_id {
        entries.retain(|e| e.id == *skill_id);
    }

    // Sort entries alphabetically for deterministic output
    entries.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    if entries.is_empty() {
        if let Some(skill_id) = &args.skill_id {
            return Err(CliError::Validation(format!(
                "Skill '{}' is not a declared dependency",
                skill_id
            )));
        }
        crate::outln!("{}", messages::info("No skills to update"));
        return Ok(());
    }

    // T033: Resolve the lock file path from the project root
    let lock_path = if let Some(parent) = project_file_path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };
    if !lock_path.exists() {
        return Err(CliError::Config(
            "skills.lock not found. Run 'fastskill install' first.".to_string(),
        ));
    }
    let lock = ProjectSkillsLock::load_from_file(&lock_path)
        .map_err(|error| CliError::Config(format!("Failed to load skills.lock: {error}")))?;

    let mut change_roots = Vec::new();
    for entry in &mut entries {
        let locked = lock
            .skills
            .iter()
            .find(|locked| locked.id == entry.id.as_str())
            .ok_or_else(|| {
                CliError::Validation(format!(
                    "Skill '{}' has no locked version to update from",
                    entry.id
                ))
            })?;
        entry.origin =
            controlled_origin(&entry.origin, &locked.resolved.version, &args).map_err(|error| {
                CliError::Validation(format!("Cannot update '{}': {error}", entry.id))
            })?;
        change_roots.push(crate::commands::install::change::ChangeRoot {
            origin: entry.origin.clone(),
            expected_id: Some(entry.id.clone()),
            groups: entry.groups.clone(),
            locked: Some(locked.resolved.clone()),
        });
    }

    // Initialize the (edge-injected) service: embedding provider + repository
    // manager (ADR-0005). Needed even in --check/--dry-run mode, since
    // `preflight` on an `Origin::Repository` entry consults the repository
    // manager.
    let config = create_service_config(false, skills_dir_override)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;
    let service = crate::config::inject_edge_services(service)?;

    let manifest_dir = project_file_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let max_levels = project
        .tool
        .as_ref()
        .and_then(|tool| tool.fastskill.as_ref())
        .map_or(5, |config| config.install_depth);
    let (prepared, previews) = crate::commands::install::change::prepare_changes(
        &service,
        &lock_path,
        manifest_dir,
        change_roots,
        max_levels,
        args.offline,
        args.dry_run || args.check,
    )
    .await?;
    let planned_refreshes = prepared.refreshed_repositories().to_vec();
    if args.check || args.dry_run {
        crate::commands::install::plan::validate_prepared(&service, manifest_dir, prepared)?;
        if !args.json {
            output::render_update_previews(&previews);
        }
        if args.json {
            let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
                &service,
                "update",
                args.reindex,
                true,
                crate::config_file::load_auto_reindex_config(),
            )
            .await;
            print_update_json(&previews, true, &planned_refreshes, &indexing)?;
            return Ok(());
        }
        crate::outln!("{}", messages::info("No changes were applied"));
        return Ok(());
    }

    if !args.json {
        output::render_update_previews(&previews);
    }

    let report = crate::commands::install::plan::apply_prepared(
        &service,
        &lock_path,
        manifest_dir,
        prepared,
    )
    .await?;
    let updated_count = previews
        .iter()
        .filter(|preview| !preview.changes.is_empty())
        .count();

    let indexing = crate::utils::reindex_utils::lifecycle_reindex_result(
        &service,
        "update",
        args.reindex,
        args.no_reindex || args.offline,
        crate::config_file::load_auto_reindex_config(),
    )
    .await;
    if args.json {
        print_update_json(&previews, false, &report.refreshed_repositories, &indexing)?;
        return Ok(());
    }

    crate::outln!();
    crate::outln!(
        "{}",
        messages::ok(&format!("Updated {} skill(s)", updated_count))
    );
    if updated_count > 0 {
        crate::outln!("   Updated skills.lock");
    }

    for repository in report.refreshed_repositories {
        crate::outln!("   Refreshed repository metadata: {repository}");
    }
    crate::utils::reindex_utils::report_lifecycle_index_result(&indexing);

    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock
)]
#[path = "update/tests.rs"]
mod tests;

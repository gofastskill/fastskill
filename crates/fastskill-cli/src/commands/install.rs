//! Install command - installs skills from skill-project.toml dependencies

use crate::config::{create_service_config, inject_edge_services};
use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::messages;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::{
    bundle::BundleService, lifecycle_transaction::LifecycleTransaction, lock::project_lock_path,
    manifest::SkillProjectToml, project::resolve_project_file,
};
use fastskill_core::FastSkillService;
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use tempfile::TempDir;

#[cfg(test)]
static FAIL_AFTER_BUNDLE_APPLY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static FAIL_DURING_BUNDLE_APPLY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) mod change;
mod global;
pub(crate) mod plan;

/// Apply manifest: install skills from the `skill-project.toml` `[dependencies]` table.
///
/// This is the canonical command for manifest-driven workflow.
///
/// Reads dependencies from the `skill-project.toml` `[dependencies]` table at the project root.
/// Installs to the skills directory configured in [tool.fastskill].skills_directory.
/// Creates or updates skills.lock for reproducible installations.
///
/// Use --lock to install exact versions from skills.lock instead of resolving from manifest.
#[derive(Debug, Clone)]
pub struct InstallArgs {
    /// Exclude skills from these groups (like poetry --without dev)
    without: Option<Vec<String>>,

    /// Only install skills from these groups
    only: Option<Vec<String>>,

    /// Install from skills.lock (exact versions) instead of resolving from skill-project.toml
    lock: bool,

    /// Maximum transitive dependency depth (overrides config file setting)
    depth: Option<i64>,

    /// Prohibit network access and use verified cached/local inputs only
    offline: bool,

    /// Validate and report the complete plan without changing persistent state
    dry_run: bool,

    /// Emit one structured JSON result
    json: bool,

    /// Trigger reindex after install (overrides config)
    reindex: bool,

    /// Skip reindex after install
    no_reindex: bool,
}

impl IntoCommandSpec for InstallArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Apply manifest: install skills from skill-project.toml [dependencies]",
            syntax: Some("install [OPTIONS]"),
            category: Some("packages"),
            examples: vec![
                "fastskill install",
                "fastskill install --lock",
                "fastskill install --without dev",
            ],
            args: vec![
                ArgSpec {
                    name: "without",
                    kind: ArgKind::Option,
                    long: Some("without"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Exclude skills from these groups (like poetry --without dev)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "only",
                    kind: ArgKind::Option,
                    long: Some("only"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Only install skills from these groups",
                    ..Default::default()
                },
                ArgSpec {
                    name: "lock",
                    kind: ArgKind::Flag,
                    long: Some("lock"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Install from skills.lock (exact versions) instead of resolving from skill-project.toml",
                    ..Default::default()
                },
                ArgSpec {
                    name: "depth",
                    kind: ArgKind::Option,
                    long: Some("depth"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Maximum transitive dependency depth (overrides config file setting)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "offline",
                    kind: ArgKind::Flag,
                    long: Some("offline"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Use verified cached and local inputs without network access",
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    kind: ArgKind::Flag,
                    long: Some("dry-run"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Validate and show the install plan without changing state",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Output one machine-readable JSON result",
                    ..Default::default()
                },
                ArgSpec {
                    name: "reindex",
                    kind: ArgKind::Flag,
                    long: Some("reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Trigger reindex after install (overrides config)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-reindex",
                    kind: ArgKind::Flag,
                    long: Some("no-reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Skip reindex after install",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

fn repeated_str_list(v: &ArgValue) -> Option<Vec<String>> {
    if let ArgValue::List(items) = v {
        let strings: Vec<String> = items
            .iter()
            .filter_map(|item| {
                if let ArgValue::Str(s) = item {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();
        if strings.is_empty() {
            None
        } else {
            Some(strings)
        }
    } else {
        None
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for InstallArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            without: map.get("without").and_then(repeated_str_list),
            only: map.get("only").and_then(repeated_str_list),
            lock: matches!(map.get("lock"), Some(ArgValue::Bool(true))),
            depth: map.get("depth").and_then(|v| {
                if let ArgValue::Int(n) = v {
                    Some(*n)
                } else {
                    None
                }
            }),
            offline: matches!(map.get("offline"), Some(ArgValue::Bool(true))),
            dry_run: matches!(map.get("dry-run"), Some(ArgValue::Bool(true))),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            reindex: matches!(map.get("reindex"), Some(ArgValue::Bool(true))),
            no_reindex: matches!(map.get("no-reindex"), Some(ArgValue::Bool(true))),
        }
    }
}

#[derive(Serialize)]
struct InstallJsonTarget {
    id: String,
    outcome: String,
    current_revision: Option<String>,
    target_revision: Option<String>,
    changes: Vec<String>,
    retained: Vec<String>,
}

#[derive(Serialize)]
struct InstallJsonResult {
    scope: &'static str,
    outcome: String,
    dry_run: bool,
    targets: Vec<InstallJsonTarget>,
    diagnostics: Vec<String>,
}

#[cfg(test)]
async fn execute_install(args: InstallArgs) -> CliResult<()> {
    execute_install_scoped(args, false, None).await
}

pub async fn execute_install_scoped(
    args: InstallArgs,
    global: bool,
    skills_dir: Option<std::path::PathBuf>,
) -> CliResult<()> {
    let json = args.json;
    let dry_run = args.dry_run;
    let result = if global {
        if skills_dir.is_some() {
            Err(CliError::Validation(
                "--global and --skills-dir cannot be used together".to_string(),
            ))
        } else {
            global::execute_global_install(args).await
        }
    } else {
        execute_install_inner(args, skills_dir).await
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            if json {
                print_install_json(InstallJsonResult {
                    scope: if global { "global" } else { "project" },
                    outcome: "blocked".to_string(),
                    dry_run,
                    targets: Vec::new(),
                    diagnostics: vec![error.to_string()],
                })?;
            }
            Err(error)
        }
    }
}

async fn execute_install_inner(
    args: InstallArgs,
    skills_dir_override: Option<std::path::PathBuf>,
) -> CliResult<()> {
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
    if args.only.is_some() && args.without.is_some() {
        return Err(CliError::Validation(
            "--only and --without cannot be used together".to_string(),
        ));
    }

    if !args.json {
        crate::outln!("Installing skills...");
        crate::outln!();
    }

    // Validate depth argument (must be > 0 if provided)
    if let Some(depth) = args.depth {
        if depth < 1 || u32::try_from(depth).is_err() {
            return Err(CliError::InvalidDepth(
                "Depth must be between 1 and 4294967295. Use --depth 1 for roots only.".to_string(),
            ));
        }
    }

    // T027: Resolve skill-project.toml from project root
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    // Require manifest unless installing from lock
    if !args.lock && !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    // T033: Lock file at project root (skills.lock)
    let lock_path = project_lock_path(&project_file_path);

    // Check for lock file early if lock mode is requested (before service initialization)
    if args.lock && !lock_path.exists() {
        return Err(CliError::Config(
            "skills.lock not found. Run 'fastskill install' first to create it.".to_string(),
        ));
    }

    // Resolve skills directory from config
    let skills_dir = match &skills_dir_override {
        Some(path) => path.clone(),
        None => crate::config::resolve_skills_storage_directory(false)?,
    };

    let declared_bundle_members = if project_file_result.found {
        let project_root = project_file_path.parent().ok_or_else(|| {
            CliError::Config("skill-project.toml has no project directory".to_string())
        })?;
        let bundle_service = BundleService::new(project_root, skills_dir.clone());
        bundle_service
            .validated_declared_members(args.lock)
            .map_err(CliError::Service)?
    } else {
        Vec::new()
    };

    // Initialize service
    // Note: install command doesn't have access to CLI sources_path, so uses env var or walk-up
    let config = create_service_config(false, skills_dir_override)?;
    let mut service = inject_edge_services(
        FastSkillService::new(config)
            .await
            .map_err(CliError::Service)?,
    )?;
    service.initialize().await.map_err(CliError::Service)?;

    // Online dry runs resolve through disposable cache/index roots. This keeps
    // freshness validation real without persisting catalog or content indexes.
    let preview_storage;
    let preview_cache;
    let preview_service;
    let planning_service = if args.dry_run && !args.offline {
        preview_storage = TempDir::new().map_err(CliError::Io)?;
        preview_cache = TempDir::new().map_err(CliError::Io)?;
        let mut config = service.config().clone();
        config.skill_storage_path = preview_storage.path().to_path_buf();
        config.skill_cache_root = Some(preview_cache.path().to_path_buf());
        let mut isolated = FastSkillService::new(config)
            .await
            .map_err(CliError::Service)?;
        if let Some(manager) = service.repository_manager() {
            isolated = isolated.with_repository_manager(manager.clone());
        }
        isolated.initialize().await.map_err(CliError::Service)?;
        preview_service = isolated;
        &preview_service
    } else {
        &service
    };

    let project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|error| CliError::Config(format!("Failed to load skill-project.toml: {error}")))?;
    project
        .validate_for_context(project_file_result.context)
        .map_err(|error| {
            CliError::Config(format!("skill-project.toml validation failed: {error}"))
        })?;
    let manifest_dir = project_file_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let roots = project
        .to_skill_entries(manifest_dir)
        .map_err(|error| CliError::Config(format!("Failed to parse dependencies: {error}")))?;
    let (config_depth, config_skip_transitive) = project
        .tool
        .as_ref()
        .and_then(|tool| tool.fastskill.as_ref())
        .map(|config| (config.install_depth, config.skip_transitive))
        .unwrap_or((5, false));
    let max_levels = args
        .depth
        .map(|depth| u32::try_from(depth).expect("depth validated above"))
        .unwrap_or(config_depth);

    let prepared = match plan::prepare(
        planning_service,
        &lock_path,
        manifest_dir,
        plan::InstallSelection {
            roots,
            only: args.only.as_deref(),
            without: args.without.as_deref(),
            max_levels,
            skip_transitive: config_skip_transitive,
            strict: args.lock,
            offline: args.offline,
        },
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(error) => {
            if !args.json {
                crate::outln!("skills.lock was not modified");
            }
            return Err(error);
        }
    };
    plan::validate_declared_bundle_members(&prepared, &declared_bundle_members)?;
    let targets = build_install_targets(&service, &prepared).await?;

    if args.dry_run {
        plan::validate_prepared(&service, manifest_dir, prepared)?;
        if args.json {
            let outcome = if targets.iter().all(|target| target.outcome == "unchanged") {
                "unchanged"
            } else {
                "changed"
            };
            print_install_json(InstallJsonResult {
                scope: "project",
                outcome: outcome.to_string(),
                dry_run: true,
                targets,
                diagnostics: Vec::new(),
            })?;
        } else if targets.is_empty() {
            crate::outln!("No skills selected; no changes would be applied");
        } else {
            for target in &targets {
                crate::outln!(
                    "  {}: {} -> {} ({})",
                    target.id,
                    target
                        .current_revision
                        .as_deref()
                        .unwrap_or("not installed"),
                    target.target_revision.as_deref().unwrap_or("unknown"),
                    target.outcome
                );
            }
            crate::outln!("Dry run complete; no changes were applied");
        }
        return Ok(());
    }

    let mut affected = prepared.affected_ids();
    affected.extend(
        declared_bundle_members
            .iter()
            .map(|member| member.id.clone()),
    );
    affected.sort();
    affected.dedup();
    let state_guard = fastskill_core::core::state_guard::StateMutationGuard::acquire_for(
        manifest_dir,
        Some(&skills_dir),
        "install bundles and skills",
    )
    .map_err(CliError::Service)?;
    let lifecycle = match LifecycleTransaction::capture(manifest_dir, &skills_dir, &affected) {
        Ok(lifecycle) => lifecycle,
        Err(error) => {
            state_guard.recovered().map_err(CliError::Service)?;
            return Err(CliError::Service(error));
        }
    };
    let restored_bundles = if project_file_result.found {
        let bundle_service = BundleService::new(manifest_dir, skills_dir);
        #[cfg(test)]
        let injected_failure =
            FAIL_DURING_BUNDLE_APPLY.swap(false, std::sync::atomic::Ordering::SeqCst);
        #[cfg(not(test))]
        let injected_failure = false;
        let restored = if injected_failure {
            Err(fastskill_core::core::service::ServiceError::Config(
                "injected failure during bundle apply".to_string(),
            ))
        } else if args.lock {
            bundle_service.install_declared_locked_with_guard(&state_guard)
        } else {
            bundle_service.install_declared_with_guard(&state_guard)
        };
        match restored {
            Ok(restored) => restored,
            Err(error) => {
                if let Err(recovery) = lifecycle.rollback() {
                    drop(state_guard);
                    return Err(CliError::Config(format!(
                        "{error}; combined lifecycle recovery failed: {recovery}"
                    )));
                }
                state_guard.recovered().map_err(CliError::Service)?;
                return Err(CliError::Service(error));
            }
        }
    } else {
        Vec::new()
    };
    #[cfg(test)]
    if FAIL_AFTER_BUNDLE_APPLY.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let error = CliError::Config("injected failure after bundle apply".to_string());
        if let Err(recovery) = lifecycle.rollback() {
            drop(state_guard);
            return Err(CliError::Config(format!(
                "{error}; combined lifecycle recovery failed: {recovery}"
            )));
        }
        state_guard.recovered().map_err(CliError::Service)?;
        return Err(error);
    }
    let report = match plan::apply_prepared_with_guard(
        &service,
        &lock_path,
        manifest_dir,
        prepared,
        &state_guard,
    )
    .await
    {
        Ok(report) => report,
        Err(error) => {
            if let Err(recovery) = lifecycle.rollback() {
                drop(state_guard);
                return Err(CliError::Config(format!(
                    "{error}; combined lifecycle recovery failed: {recovery}"
                )));
            }
            state_guard.recovered().map_err(CliError::Service)?;
            return Err(error);
        }
    };
    lifecycle.commit();
    state_guard.commit().map_err(CliError::Service)?;

    for bundle in restored_bundles
        .iter()
        .filter(|bundle| !bundle.unchanged && !args.json)
    {
        crate::outln!(
            "  {}",
            messages::ok(&format!("Restored bundle {}@{}", bundle.id, bundle.version))
        );
    }

    for repository in report.refreshed_repositories.iter().filter(|_| !args.json) {
        crate::outln!("   Refreshed repository metadata: {repository}");
    }
    if !args.json {
        for id in &report.mutable_sources {
            crate::outln!(
                "   Editable skill {id} remains mutable; identity and target were verified, but content integrity is not pinned"
            );
        }
    }

    let auto_reindex = crate::config_file::load_auto_reindex_config();
    let index_result = crate::utils::reindex_utils::lifecycle_reindex_result(
        &service,
        "install",
        args.reindex,
        args.no_reindex || args.offline,
        auto_reindex,
    )
    .await;

    if args.json {
        let outcome = if targets.iter().all(|target| target.outcome == "unchanged")
            && restored_bundles.iter().all(|bundle| bundle.unchanged)
        {
            "unchanged"
        } else {
            "changed"
        };
        print_install_json(InstallJsonResult {
            scope: "project",
            outcome: outcome.to_string(),
            dry_run: false,
            targets,
            diagnostics: report
                .refreshed_repositories
                .iter()
                .map(|repository| format!("refreshed repository metadata: {repository}"))
                .chain(report.mutable_sources.iter().map(|id| {
                    format!("editable skill {id} remains mutable; content integrity is not pinned")
                }))
                .chain(
                    index_result.diagnostic.iter().map(|diagnostic| {
                        format!("indexing {}: {diagnostic}", index_result.outcome)
                    }),
                )
                .collect(),
        })?;
    } else if report.installed.is_empty() {
        crate::outln!("{}", messages::info("No skills selected for installation"));
    } else {
        for id in &report.installed {
            crate::outln!("  {}", messages::ok(&format!("Installed {id}")));
        }
        crate::outln!();
        crate::outln!("{}", messages::ok("Installation complete"));
        if report.used_lock {
            crate::outln!("   Restored verified selections from skills.lock");
        } else {
            crate::outln!("   Updated skills.lock");
        }
        if let Some(diagnostic) = index_result.diagnostic {
            crate::outln!("   Indexing {}: {diagnostic}", index_result.outcome);
        } else if index_result.outcome == "succeeded" {
            crate::outln!("   Indexed {} skill(s)", index_result.count);
        }
    }
    Ok(())
}

async fn build_install_targets(
    service: &FastSkillService,
    prepared: &plan::PreparedInstallPlan,
) -> CliResult<Vec<InstallJsonTarget>> {
    let mut targets = Vec::new();
    for target in prepared.targets() {
        let id = fastskill_core::SkillId::new(target.id.clone()).map_err(CliError::Service)?;
        let current = service
            .skill_manager()
            .get_skill(&id)
            .await
            .map_err(CliError::Service)?;
        let unchanged = current.as_ref().is_some_and(|installed| {
            installed.version == target.resolved_version
                && target.checksum.as_ref().is_some_and(|checksum| {
                    fastskill_core::core::install::content_digest(
                        &service.config().skill_storage_path.join(&target.id),
                    )
                    .is_ok_and(|actual| &actual == checksum)
                })
        });
        let current_revision = current.map(|installed| installed.version);
        let outcome = if unchanged { "unchanged" } else { "changed" };
        targets.push(InstallJsonTarget {
            id: target.id,
            outcome: outcome.to_string(),
            current_revision,
            target_revision: Some(target.resolved_version),
            changes: if unchanged {
                Vec::new()
            } else {
                vec!["install verified content".to_string()]
            },
            retained: if unchanged {
                vec!["verified content already installed".to_string()]
            } else {
                Vec::new()
            },
        });
    }
    Ok(targets)
}

fn print_install_json(result: InstallJsonResult) -> CliResult<()> {
    let json = serde_json::to_string(&result).map_err(|error| {
        CliError::Config(format!("Failed to serialize install result: {error}"))
    })?;
    crate::outln!("{json}");
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock,
    clippy::collapsible_if
)]
#[path = "install/tests.rs"]
mod tests;

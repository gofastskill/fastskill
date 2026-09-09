//! List locally installed skills command
//!
//! Similar to `pip list` or `uv list`, this command lists all locally installed skills
//! and reconciles them against skill-project.toml and skills.lock.
//!
//! Requires skill-project.toml in the hierarchy. Uses three sources: installed skills (target
//! folder), the `skill-project.toml` `[dependencies]` table, and `skills.lock`. Outputs one table with flags
//! for missing from folder, missing from lock, missing from manifest.

use crate::commands::common::validate_format_args;
use crate::error::{manifest_required_message, CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::lock::ProjectSkillsLock;
use fastskill_core::core::manifest::SkillProjectToml;
use fastskill_core::core::origin::Origin;
use fastskill_core::core::ownership::ProjectOwnership;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::core::project_removal::managed_tree_digest;
use fastskill_core::core::service::FastSkillService;
use fastskill_core::OutputFormat;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;

#[path = "list/global.rs"]
mod global;
#[path = "list/output.rs"]
mod output;
use global::execute_global_list;
use output::ListRow;

/// List locally installed skills
#[derive(Debug, Clone)]
pub struct ListArgs {
    /// Output format: table, json, grid, xml (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,

    /// Show detailed information (version, manifest/lock/installed status, source path, type)
    pub details: bool,

    /// List installed bundles rather than individual skills
    pub bundles: bool,

    /// Return nonzero when selected managed state needs reconciliation
    pub check: bool,

    /// Check only roots in these groups
    pub only: Option<Vec<String>>,

    /// Exclude roots in these groups from the check
    pub without: Option<Vec<String>>,

    /// Skills directory path (overrides default discovery)
    #[allow(dead_code)]
    pub skills_dir: Option<std::path::PathBuf>,
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

impl IntoCommandSpec for ListArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List locally installed skills",
            syntax: Some("list [OPTIONS]"),
            category: Some("discovery"),
            examples: vec![
                "fastskill list",
                "fastskill list --details --format json",
                "fastskill list --bundles",
            ],
            args: vec![
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
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
                ArgSpec {
                    name: "details",
                    kind: ArgKind::Flag,
                    long: Some("details"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Show detailed information",
                    ..Default::default()
                },
                ArgSpec {
                    name: "bundles",
                    kind: ArgKind::Flag,
                    long: Some("bundles"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "List installed bundles",
                    ..Default::default()
                },
                ArgSpec {
                    name: "check",
                    kind: ArgKind::Flag,
                    long: Some("check"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Return nonzero when selected managed state needs reconciliation",
                    ..Default::default()
                },
                ArgSpec {
                    name: "only",
                    kind: ArgKind::Option,
                    long: Some("only"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Check only roots in these groups",
                    ..Default::default()
                },
                ArgSpec {
                    name: "without",
                    kind: ArgKind::Option,
                    long: Some("without"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Exclude roots in these groups from the check",
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
            details: matches!(map.get("details"), Some(ArgValue::Bool(true))),
            bundles: matches!(map.get("bundles"), Some(ArgValue::Bool(true))),
            check: matches!(map.get("check"), Some(ArgValue::Bool(true))),
            only: map.get("only").and_then(repeated_strings),
            without: map.get("without").and_then(repeated_strings),
            // skills_dir is omitted from the spec; rely on the global --skills-dir flag
            skills_dir: None,
        }
    }
}

fn repeated_strings(value: &ArgValue) -> Option<Vec<String>> {
    let ArgValue::List(values) = value else {
        return None;
    };
    let values = values
        .iter()
        .filter_map(|value| match value {
            ArgValue::Str(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

/// Short origin-type label (git/local/zip-url/repository) for display.
/// Mirrors `fastskill_core::output::origin_type_label` (private to that crate).
fn origin_type_label(origin: &Origin) -> &'static str {
    match origin {
        Origin::Git { .. } => "git",
        Origin::Local { .. } => "local",
        Origin::ZipUrl { .. } => "zip-url",
        Origin::Repository { .. } => "repository",
    }
}

/// Location string (URL/path/repo-skill) for display.
/// Mirrors `fastskill_core::output::origin_location_label` (private to that crate).
fn origin_location_label(origin: &Origin) -> String {
    match origin {
        Origin::Git { url, .. } => url.clone(),
        Origin::Local { path, .. } => path.display().to_string(),
        Origin::ZipUrl { url } => url.clone(),
        Origin::Repository { repo, skill, .. } => format!("{repo}/{skill}"),
    }
}

/// Extract source path and type from an `Origin`.
fn format_source_info(origin: &Origin) -> (Option<String>, Option<String>) {
    (
        Some(origin_location_label(origin)),
        Some(origin_type_label(origin).to_string()),
    )
}

/// Execute the list command
pub async fn execute_list(
    service: &FastSkillService,
    args: ListArgs,
    global: bool,
) -> CliResult<()> {
    // Validate format arguments
    let format = validate_format_args(&args.format, args.json)?;
    if args.only.is_some() && args.without.is_some() {
        return Err(CliError::Validation(
            "--only and --without cannot be used together".to_string(),
        ));
    }
    if args.bundles && (args.check || args.only.is_some() || args.without.is_some()) {
        return Err(CliError::Validation(
            "--bundles cannot be combined with --check, --only, or --without".to_string(),
        ));
    }
    if global {
        return execute_global_list(service, args, format).await;
    }

    // Require manifest: resolve from current directory
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    let project_file_path = project_file_result.path;
    if args.bundles {
        let root = project_file_path.parent().ok_or_else(|| {
            CliError::Config("skill-project.toml has no project directory".to_string())
        })?;
        let bundles = fastskill_core::core::bundle::BundleService::new(
            root,
            service.config().skill_storage_path.clone(),
        )
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
        return Ok(());
    }
    let lock_path = project_file_path
        .parent()
        .map(|p| p.join("skills.lock"))
        .unwrap_or_else(|| PathBuf::from("skills.lock"));

    // Load skill-project.toml and skills.lock
    let project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;
    let manifest_ids: HashMap<String, ()> = project
        .dependencies
        .as_ref()
        .map(|d| d.dependencies.keys().cloned().map(|k| (k, ())).collect())
        .unwrap_or_default();

    let lock = if lock_path.exists() {
        ProjectSkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load skills.lock: {}", e)))?
    } else {
        ProjectSkillsLock::new_empty()
    };
    let manifest_directory = project_file_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let desired_entries = project
        .to_skill_entries(manifest_directory)
        .map_err(|error| CliError::Config(format!("Failed to parse dependencies: {error}")))?
        .into_iter()
        .map(|entry| (entry.id.to_string(), entry))
        .collect::<HashMap<_, _>>();
    let ownership = ProjectOwnership::new(&project, &lock);
    let root_groups = desired_entries
        .values()
        .map(|entry| {
            let groups = if entry.groups.is_empty() {
                vec!["default".to_string()]
            } else {
                entry.groups.clone()
            };
            (entry.id.clone(), groups)
        })
        .collect::<HashMap<_, _>>();
    let known_groups = root_groups
        .values()
        .flatten()
        .cloned()
        .collect::<HashSet<_>>();
    for requested in args.only.iter().chain(args.without.iter()).flatten() {
        if !known_groups.contains(requested) {
            return Err(CliError::Validation(format!("Unknown group '{requested}'")));
        }
    }
    let selected_roots = root_groups
        .iter()
        .filter(|(_, groups)| {
            args.only
                .as_ref()
                .is_none_or(|only| groups.iter().any(|group| only.contains(group)))
                && args
                    .without
                    .as_ref()
                    .is_none_or(|without| !groups.iter().any(|group| without.contains(group)))
        })
        .map(|(id, _)| id.clone())
        .collect::<HashSet<_>>();

    // Build lock map with additional metadata
    let lock_map: HashMap<String, (String, String, Origin)> = lock
        .skills
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                (s.resolved.version.clone(), s.name.clone(), s.origin.clone()),
            )
        })
        .collect();

    // Installed skills from service
    let skill_manager = service.skill_manager();
    let installed_skills = skill_manager.list_skills().await.map_err(|e| {
        CliError::Service(fastskill_core::ServiceError::Custom(format!(
            "Failed to list installed skills: {}",
            e
        )))
    })?;

    // Build installed map with full skill definitions
    let installed_map: HashMap<String, fastskill_core::core::skill_manager::SkillDefinition> =
        installed_skills
            .into_iter()
            .map(|s| (s.id.to_string(), s))
            .collect();

    // Union of all skill IDs
    let all_ids: HashSet<String> = manifest_ids
        .keys()
        .chain(lock_map.keys())
        .chain(installed_map.keys())
        .chain(
            lock.bundles
                .iter()
                .flat_map(|bundle| bundle.members.iter().map(|member| &member.id)),
        )
        .chain(lock.overrides.iter().map(|entry| &entry.id))
        .cloned()
        .collect();

    let mut check_failures = Vec::new();
    let mut rows: Vec<ListRow> = all_ids
        .into_iter()
        .map(|id| {
            let in_manifest = manifest_ids.contains_key(&id);
            let bundle_members = lock
                .bundles
                .iter()
                .flat_map(|bundle| bundle.members.iter())
                .filter(|member| member.id == id)
                .collect::<Vec<_>>();
            let override_entry = lock.overrides.iter().find(|entry| entry.id == id);
            let in_lock = lock_map.contains_key(&id)
                || !bundle_members.is_empty()
                || override_entry.is_some();
            let installed = installed_map.contains_key(&id);

            // Get name: prefer installed, fallback to lock, fallback to id
            let name = if let Some(skill) = installed_map.get(&id) {
                skill.name.clone()
            } else if let Some((_, lock_name, _)) = lock_map.get(&id) {
                lock_name.clone()
            } else {
                id.clone()
            };

            // Get description: prefer installed, fallback to "-"
            let description = if let Some(skill) = installed_map.get(&id) {
                skill.description.clone()
            } else {
                "-".to_string()
            };

            // Get version: prefer installed, fallback to lock
            let version = if let Some(skill) = installed_map.get(&id) {
                Some(skill.version.clone())
            } else {
                lock_map.get(&id).map(|(v, _, _)| v.clone())
            };

            // Get source path and type
            let (source_path, source_type) = if let Some(skill) = installed_map.get(&id) {
                // For installed skills, use skill_file path and the origin's type label.
                let path = Some(skill.skill_file.display().to_string());
                let stype = Some(origin_type_label(&skill.origin).to_string());
                (path, stype)
            } else if let Some((_, _, origin)) = lock_map.get(&id) {
                // For lock-only skills, extract from Origin
                format_source_info(origin)
            } else {
                (None, None)
            };

            let missing_from_folder = (in_manifest || in_lock) && !installed;
            let missing_from_lock = (in_manifest || installed) && !in_lock;
            let mut owners = ownership.roots_requiring_skill(&id);
            owners.extend(
                lock.bundles
                    .iter()
                    .filter(|bundle| bundle.members.iter().any(|member| member.id == id))
                    .map(|bundle| format!("bundle:{}", bundle.id)),
            );
            let override_active = lock.overrides.iter().any(|entry| entry.id == id);
            if override_active {
                owners.push("personal-override".to_string());
            }
            owners.sort();
            owners.dedup();
            let selected = owners.iter().any(|owner| {
                owner.starts_with("bundle:")
                    || owner == "personal-override"
                    || selected_roots.contains(owner)
            });
            let extraneous = owners.is_empty() && installed;
            let missing_from_manifest = !in_manifest && owners.is_empty() && (in_lock || installed);
            let locked_entry = lock.skills.iter().find(|entry| entry.id == id);
            let desired_entry = desired_entries.get(&id);
            let mutable = locked_entry
                .is_some_and(|entry| matches!(entry.origin, Origin::Local { editable: true, .. }));
            let reconciliation = if !selected && !owners.is_empty() {
                "excluded"
            } else if selected && desired_entry.is_some() && locked_entry.is_none() {
                "missing-lock"
            } else if selected && !installed {
                "missing-content"
            } else if let (Some(desired), Some(locked)) = (desired_entry, locked_entry) {
                if desired.origin != locked.origin.resolved_against(manifest_directory) {
                    "intent-mismatch"
                } else if installed_map
                    .get(&id)
                    .is_some_and(|actual| actual.version != locked.resolved.version)
                {
                    "revision-mismatch"
                } else if mutable {
                    "ok"
                } else if let Some(expected) = &locked.resolved.checksum {
                    match managed_tree_digest(&service.config().skill_storage_path.join(&id)) {
                        Ok(actual) if &actual == expected => "ok",
                        Ok(_) => "content-mismatch",
                        Err(_) => "integrity-error",
                    }
                } else {
                    "insufficient-integrity"
                }
            } else if selected && (!bundle_members.is_empty() || override_entry.is_some()) {
                let expected = override_entry
                    .map(|entry| entry.digest.as_str())
                    .or_else(|| bundle_members.first().map(|member| member.digest.as_str()));
                let owners_agree = override_entry.is_some()
                    || bundle_members
                        .iter()
                        .all(|member| Some(member.digest.as_str()) == expected);
                match (owners_agree, expected) {
                    (false, _) => "ownership-conflict",
                    (true, Some(expected)) => {
                        match managed_tree_digest(&service.config().skill_storage_path.join(&id)) {
                            Ok(actual) if actual == expected => "ok",
                            Ok(_) => "content-mismatch",
                            Err(_) => "integrity-error",
                        }
                    }
                    _ => "insufficient-integrity",
                }
            } else if let (Some(locked), Some(actual)) = (locked_entry, installed_map.get(&id)) {
                if actual.version != locked.resolved.version {
                    "revision-mismatch"
                } else if mutable {
                    "ok"
                } else if let Some(expected) = &locked.resolved.checksum {
                    match managed_tree_digest(&service.config().skill_storage_path.join(&id)) {
                        Ok(actual) if &actual == expected => "ok",
                        Ok(_) => "content-mismatch",
                        Err(_) => "integrity-error",
                    }
                } else {
                    "insufficient-integrity"
                }
            } else if extraneous {
                "extraneous"
            } else {
                "ok"
            };
            if args.check && selected && !matches!(reconciliation, "ok" | "excluded") {
                check_failures.push(format!("{id}: {reconciliation}"));
            }
            let desired_constraint = desired_entry.map(|entry| match &entry.origin {
                Origin::Repository { version, .. } => version
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "latest".to_string()),
                origin => origin_type_label(origin).to_string(),
            });

            ListRow {
                id: id.clone(),
                name,
                description,
                version,
                in_manifest,
                in_lock,
                installed,
                source_path,
                source_type,
                missing_from_folder,
                missing_from_lock,
                missing_from_manifest,
                desired_constraint,
                locked_version: locked_entry.map(|entry| entry.resolved.version.clone()),
                actual_version: installed_map.get(&id).map(|skill| skill.version.clone()),
                reconciliation: reconciliation.to_string(),
                owners,
                groups: desired_entry
                    .map(|entry| entry.groups.clone())
                    .or_else(|| locked_entry.map(|entry| entry.groups.clone()))
                    .unwrap_or_default(),
                mutable,
                override_active,
                extraneous,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));

    let formatted_output =
        output::format_list_results(&rows, format, args.details).map_err(CliError::Config)?;
    crate::outln!("{}", formatted_output);

    if args.check && !check_failures.is_empty() {
        return Err(CliError::Config(format!(
            "Managed state needs reconciliation: {}",
            check_failures.join("; ")
        )));
    }

    Ok(())
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::await_holding_lock
)]
#[cfg(test)]
#[path = "list/tests.rs"]
mod tests;

//! `project repin`: replace legacy content digests with the current form (ADR-0017).

use crate::config::create_service_config;
use crate::error::{manifest_required_message, CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::lock::global_lock_path;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::core::repin::{
    repin_global, repin_project, RepinEntry, RepinRecord, RepinReport,
};
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

/// Re-pin legacy content digests from the installed content, without changing it.
#[derive(Debug, Clone)]
pub struct RepinArgs {
    /// Report legacy values without changing anything; fail when any remain
    check: bool,
    /// Emit one structured JSON result
    json: bool,
}

impl IntoCommandSpec for RepinArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Rewrite legacy content digests from the installed content",
            syntax: Some("project repin [OPTIONS]"),
            category: Some("packages"),
            help_order: Some(25),
            examples: vec![
                "fastskill project repin",
                "fastskill project repin --check",
                "fastskill --global project repin",
            ],
            args: vec![
                ArgSpec {
                    name: "check",
                    kind: ArgKind::Flag,
                    long: Some("check"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Change nothing; exit non-zero when any legacy digest remains",
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
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for RepinArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            check: matches!(map.get("check"), Some(ArgValue::Bool(true))),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
        }
    }
}

#[derive(Serialize)]
struct RepinJsonTarget {
    id: String,
    record: RepinRecord,
    file: String,
    outcome: &'static str,
    current_revision: String,
    target_revision: Option<String>,
    changes: Vec<String>,
    retained: Vec<String>,
}

#[derive(Serialize)]
struct RepinJsonResult {
    scope: &'static str,
    outcome: &'static str,
    dry_run: bool,
    targets: Vec<RepinJsonTarget>,
    legacy_artifacts: Vec<String>,
    diagnostics: Vec<String>,
}

pub async fn execute_repin(
    args: RepinArgs,
    global: bool,
    skills_dir: Option<PathBuf>,
) -> CliResult<()> {
    if global && skills_dir.is_some() {
        return Err(CliError::Validation(
            "--global and --skills-dir cannot be used together".to_string(),
        ));
    }
    let report = if global {
        let lock_path = global_lock_path().map_err(|e| CliError::Config(e.to_string()))?;
        let skills = create_service_config(true, None)?.skill_storage_path;
        repin_global(&lock_path, &skills, args.check)?
    } else {
        let project = resolve_project_file(&env::current_dir()?);
        if !project.found {
            return Err(CliError::Config(manifest_required_message().to_string()));
        }
        let root = project
            .path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let skills = create_service_config(false, skills_dir)?.skill_storage_path;
        repin_project(&root, &skills, args.check)?
    };

    let remaining = report.legacy_remaining();
    if args.json {
        print_json(&report, global, args.check)?;
    } else {
        print_text(&report, args.check, global);
    }
    if remaining == 0 {
        return Ok(());
    }
    let command = if global {
        "fastskill --global project repin"
    } else {
        "fastskill project repin"
    };
    Err(CliError::Validation(
        if args.check && report.repinnable().next().is_some() {
            format!("{remaining} legacy content digest(s) remain; run `{command}`")
        } else {
            format!("{remaining} legacy content digest(s) remain; see the report above")
        },
    ))
}

fn describe(entry: &RepinEntry) -> String {
    let file = entry.file.file_name().unwrap_or(entry.file.as_os_str());
    let file = file.to_string_lossy();
    match &entry.version {
        Some(version) => format!("{} {}@{} ({file})", entry.record.label(), entry.id, version),
        None => format!("{} {} ({file})", entry.record.label(), entry.id),
    }
}

fn print_text(report: &RepinReport, check: bool, global: bool) {
    let repinnable: Vec<&RepinEntry> = report.repinnable().collect();
    let retained: Vec<&RepinEntry> = report.retained().collect();
    if repinnable.is_empty() && retained.is_empty() && report.legacy_artifacts.is_empty() {
        crate::outln!("No legacy content digests found.");
        return;
    }
    if !repinnable.is_empty() {
        if report.written {
            crate::outln!("Re-pinned {} legacy content digest(s):", repinnable.len());
        } else {
            crate::outln!(
                "{} legacy content digest(s) match the installed content and can be re-pinned:",
                repinnable.len()
            );
        }
        for entry in &repinnable {
            crate::outln!("  {}", describe(entry));
        }
    }
    if !retained.is_empty() {
        crate::outln!("Left as is ({}):", retained.len());
        for entry in &retained {
            crate::outln!(
                "  {}: {}",
                describe(entry),
                entry.reason.as_deref().unwrap_or_default()
            );
        }
    }
    if !report.legacy_artifacts.is_empty() {
        crate::outln!(
            "Bundle artifacts with legacy digests ({}); artifacts are immutable, so rebuild each with `fastskill bundle build`:",
            report.legacy_artifacts.len()
        );
        for artifact in &report.legacy_artifacts {
            crate::outln!("  {}", artifact.display());
        }
    }
    if check && !repinnable.is_empty() {
        let scope = if global { "--global " } else { "" };
        crate::outln!("Run `fastskill {scope}project repin` to re-pin them.");
    }
}

fn print_json(report: &RepinReport, global: bool, check: bool) -> CliResult<()> {
    let targets: Vec<RepinJsonTarget> = report
        .entries
        .iter()
        .map(|entry| {
            let changes: Vec<String> = match &entry.current {
                Some(_) => std::iter::once("digest".to_string())
                    .chain(
                        entry
                            .members
                            .iter()
                            .map(|member| format!("member:{}", member.id)),
                    )
                    .collect(),
                None => Vec::new(),
            };
            RepinJsonTarget {
                id: entry.version.as_ref().map_or_else(
                    || entry.id.clone(),
                    |version| format!("{}@{version}", entry.id),
                ),
                record: entry.record,
                file: entry.file.display().to_string(),
                outcome: if entry.current.is_some() {
                    "changed"
                } else {
                    "blocked"
                },
                current_revision: entry.legacy.clone(),
                target_revision: entry.current.clone(),
                changes,
                retained: entry.reason.iter().cloned().collect(),
            }
        })
        .collect();
    let outcome = if report.retained().next().is_some() || !report.legacy_artifacts.is_empty() {
        "blocked"
    } else if report.repinnable().next().is_some() {
        "changed"
    } else {
        "unchanged"
    };
    let result = RepinJsonResult {
        scope: if global { "global" } else { "project" },
        outcome,
        dry_run: check,
        targets,
        legacy_artifacts: report
            .legacy_artifacts
            .iter()
            .map(|artifact| artifact.display().to_string())
            .collect(),
        diagnostics: report
            .legacy_artifacts
            .iter()
            .map(|artifact| {
                format!(
                    "{} declares legacy digests; rebuild it with `fastskill bundle build`",
                    artifact.display()
                )
            })
            .collect(),
    };
    let json = serde_json::to_string(&result).map_err(|e| CliError::Config(e.to_string()))?;
    crate::outln!("{json}");
    Ok(())
}

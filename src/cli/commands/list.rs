//! List locally installed skills command
//!
//! Similar to `pip list` or `uv list`, this command lists all locally installed skills
//! and reconciles them against skill-project.toml and skills.lock.
//!
//! Requires skill-project.toml in the hierarchy. Uses three sources: installed skills (target
//! folder), skill-project.toml [dependencies], and skills.lock. Outputs one table with flags
//! for missing from folder, missing from lock, missing from manifest.

use crate::cli::error::{manifest_required_message, CliError, CliResult};
use crate::cli::utils::messages;
use clap::Args;
use fastskill::core::lock::SkillsLock;
use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::project::resolve_project_file;
use fastskill::core::service::FastSkillService;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;

/// One row for the list table: union of all skills with presence and gap flags.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ListRow {
    pub id: String,
    pub version: Option<String>,
    pub in_manifest: bool,
    pub in_lock: bool,
    pub installed: bool,
    pub missing_from_folder: bool,
    pub missing_from_lock: bool,
    pub missing_from_manifest: bool,
}

/// List locally installed skills
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    /// Output in grid format (default)
    #[arg(long)]
    pub grid: bool,
}

/// Execute the list command
pub async fn execute_list(service: &FastSkillService, args: ListArgs) -> CliResult<()> {
    // Validate conflicting flags
    if args.json && args.grid {
        return Err(CliError::Config(
            "Cannot use both --json and --grid flags. Use only one.".to_string(),
        ));
    }

    // Require manifest: resolve from current directory
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    let project_file_path = project_file_result.path;
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
        SkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load skills.lock: {}", e)))?
    } else {
        SkillsLock {
            metadata: fastskill::core::lock::LockMetadata {
                version: "1.0.0".to_string(),
                generated_at: chrono::Utc::now(),
                fastskill_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            skills: Vec::new(),
        }
    };
    let lock_map: HashMap<String, String> = lock
        .skills
        .iter()
        .map(|s| (s.id.clone(), s.version.clone()))
        .collect();

    // Installed skills from service
    let skill_manager = service.skill_manager();
    let installed_skills = skill_manager.list_skills(None).await.map_err(|e| {
        CliError::Service(fastskill::ServiceError::Custom(format!(
            "Failed to list installed skills: {}",
            e
        )))
    })?;
    let installed_map: HashMap<String, String> = installed_skills
        .iter()
        .map(|s| (s.id.to_string(), s.version.clone()))
        .collect();

    // Union of all skill IDs
    let all_ids: HashSet<String> = manifest_ids
        .keys()
        .chain(lock_map.keys())
        .chain(installed_map.keys())
        .cloned()
        .collect();

    let mut rows: Vec<ListRow> = all_ids
        .into_iter()
        .map(|id| {
            let in_manifest = manifest_ids.contains_key(&id);
            let in_lock = lock_map.contains_key(&id);
            let installed = installed_map.contains_key(&id);
            let version = installed_map
                .get(&id)
                .or_else(|| lock_map.get(&id))
                .cloned();
            let missing_from_folder = (in_manifest || in_lock) && !installed;
            let missing_from_lock = (in_manifest || installed) && !in_lock;
            let missing_from_manifest = (in_lock || installed) && !in_manifest;
            ListRow {
                id: id.clone(),
                version,
                in_manifest,
                in_lock,
                installed,
                missing_from_folder,
                missing_from_lock,
                missing_from_manifest,
            }
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));

    let output_format = if args.json { "json" } else { "grid" };
    match output_format {
        "json" => {
            let json_output = serde_json::to_string_pretty(&rows)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            println!("{}", json_output);
        }
        "grid" => {
            format_list_grid(&rows)?;
        }
        _ => unreachable!(),
    }

    Ok(())
}

/// Format list output as grid table: one row per skill with In manifest, In lock, Installed, and Flags.
fn format_list_grid(rows: &[ListRow]) -> CliResult<()> {
    if rows.is_empty() {
        println!(
            "{}",
            messages::info("No skills (manifest is empty and nothing installed)")
        );
        return Ok(());
    }

    let headers = [
        "ID",
        "Version",
        "In manifest",
        "In lock",
        "Installed",
        "Flags",
    ];
    let mut col_widths = vec![0; headers.len()];
    for (i, h) in headers.iter().enumerate() {
        col_widths[i] = h.len();
    }
    for row in rows {
        col_widths[0] = col_widths[0].max(row.id.len());
        col_widths[1] = col_widths[1].max(row.version.as_deref().unwrap_or("-").len());
        col_widths[2] = col_widths[2].max(1);
        col_widths[3] = col_widths[3].max(1);
        col_widths[4] = col_widths[4].max(1);
        let flags = build_flags_str(row);
        col_widths[5] = col_widths[5].max(flags.len());
    }

    let header_row: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:width$}", *h, width = col_widths[i]))
        .collect();
    println!();
    println!("{}", header_row.join("  "));
    println!("{}", "-".repeat(header_row.join("  ").len()));

    for row in rows {
        let version = row.version.as_deref().unwrap_or("-");
        let in_manifest = if row.in_manifest { "Y" } else { "-" };
        let in_lock = if row.in_lock { "Y" } else { "-" };
        let installed = if row.installed { "Y" } else { "-" };
        let flags = build_flags_str(row);
        let line = [
            format!("{:width$}", row.id, width = col_widths[0]),
            format!("{:width$}", version, width = col_widths[1]),
            format!("{:width$}", in_manifest, width = col_widths[2]),
            format!("{:width$}", in_lock, width = col_widths[3]),
            format!("{:width$}", installed, width = col_widths[4]),
            format!("{:width$}", flags, width = col_widths[5]),
        ];
        println!("{}", line.join("  "));
    }
    println!();
    Ok(())
}

fn build_flags_str(row: &ListRow) -> String {
    let mut parts = Vec::new();
    if row.missing_from_folder {
        parts.push("missing from folder");
    }
    if row.missing_from_lock {
        parts.push("missing from lock");
    }
    if row.missing_from_manifest {
        parts.push("missing from manifest");
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("; ")
    }
}

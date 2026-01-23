//! List locally installed skills command
//!
//! Similar to `pip list` or `uv list`, this command lists all locally installed skills
//! and reconciles them against skills-project.toml and skills-lock.toml

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use clap::Args;
use fastskill::core::reconciliation::{build_reconciliation_report, ReconciliationReport};
use fastskill::core::service::FastSkillService;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    let config = service.config();
    let skills_dir = &config.skill_storage_path;

    // Get all installed skills from the service
    let skill_manager = service.skill_manager();
    let installed_skills = skill_manager.list_skills(None).await.map_err(|e| {
        CliError::Service(fastskill::ServiceError::Custom(format!(
            "Failed to list installed skills: {}",
            e
        )))
    })?;

    // Resolve project and lock file paths
    let project_path = resolve_project_file(skills_dir);
    let lock_path = resolve_lock_file(skills_dir);

    // Load project and lock files
    let project_deps = load_project_file(&project_path).unwrap_or_default();
    let lock_deps = load_lock_file(&lock_path).unwrap_or_default();

    // Build reconciliation report
    let report =
        build_reconciliation_report(&installed_skills, &project_deps, &lock_deps, skills_dir)
            .map_err(CliError::Service)?;

    // Format output
    let output_format = if args.json { "json" } else { "grid" };
    match output_format {
        "json" => {
            let json_output = serde_json::to_string_pretty(&report)
                .map_err(|e| CliError::Config(format!("Failed to serialize JSON: {}", e)))?;
            println!("{}", json_output);
        }
        "grid" => {
            format_list_grid(&report)?;
        }
        _ => unreachable!(),
    }

    Ok(())
}

/// Resolve skills-project.toml path
fn resolve_project_file(skills_dir: &Path) -> PathBuf {
    // Try .claude/skills-project.toml first (common location)
    if let Some(parent) = skills_dir.parent() {
        let project_file = parent.join("skills-project.toml");
        if project_file.exists() {
            return project_file;
        }
    }

    // Try current directory
    let project_file = PathBuf::from("skills-project.toml");
    if project_file.exists() {
        return project_file;
    }

    // Default to .claude/skills-project.toml
    skills_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("skills-project.toml")
}

/// Resolve skills-lock.toml path
fn resolve_lock_file(skills_dir: &Path) -> PathBuf {
    // Try .claude/skills-lock.toml first (common location)
    if let Some(parent) = skills_dir.parent() {
        let lock_file = parent.join("skills-lock.toml");
        if lock_file.exists() {
            return lock_file;
        }
    }

    // Try current directory
    let lock_file = PathBuf::from("skills-lock.toml");
    if lock_file.exists() {
        return lock_file;
    }

    // Default to .claude/skills-lock.toml
    skills_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("skills-lock.toml")
}

/// Load dependencies from skills-project.toml
fn load_project_file(path: &Path) -> Result<HashMap<String, Option<String>>, CliError> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| CliError::Config(format!("Failed to read {}: {}", path.display(), e)))?;

    let toml_value: toml::Value = toml::from_str(&content)
        .map_err(|e| CliError::Config(format!("Failed to parse {}: {}", path.display(), e)))?;

    let mut deps = HashMap::new();

    // Parse [dependencies] section
    if let Some(deps_section) = toml_value.get("dependencies").and_then(|d| d.as_table()) {
        for (key, value) in deps_section {
            let version = value.as_str().map(|version_str| version_str.to_string());
            deps.insert(key.clone(), version);
        }
    }

    Ok(deps)
}

/// Load locked versions from skills-lock.toml
fn load_lock_file(path: &Path) -> Result<HashMap<String, String>, CliError> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| CliError::Config(format!("Failed to read {}: {}", path.display(), e)))?;

    let toml_value: toml::Value = toml::from_str(&content)
        .map_err(|e| CliError::Config(format!("Failed to parse {}: {}", path.display(), e)))?;

    let mut locked = HashMap::new();

    // Parse [dependencies] section
    if let Some(deps_section) = toml_value.get("dependencies").and_then(|d| d.as_table()) {
        for (key, value) in deps_section {
            if let Some(version_str) = value.get("version").and_then(|v| v.as_str()) {
                locked.insert(key.clone(), version_str.to_string());
            }
        }
    }

    Ok(locked)
}

/// Format list output as grid table
fn format_list_grid(report: &ReconciliationReport) -> CliResult<()> {
    if report.installed.is_empty() && report.missing.is_empty() && report.extraneous.is_empty() {
        println!("{}", messages::info("No skills installed"));
        return Ok(());
    }

    // Print installed skills
    if !report.installed.is_empty() {
        println!("\n{}", messages::info("Installed Skills:"));
        println!();

        let headers = ["ID", "Version", "Description", "Source", "Status"];
        let mut col_widths = vec![0; headers.len()];

        // Calculate column widths
        for (i, header) in headers.iter().enumerate() {
            col_widths[i] = header.len();
        }

        for skill in &report.installed {
            col_widths[0] = col_widths[0].max(skill.id.len());
            col_widths[1] = col_widths[1].max(skill.version.len());
            let desc_len = skill.description.len().min(50);
            col_widths[2] = col_widths[2].max(desc_len);
            let source = skill.source.as_deref().unwrap_or("local");
            col_widths[3] = col_widths[3].max(source.len());
            let status_str = format!("{:?}", skill.status);
            col_widths[4] = col_widths[4].max(status_str.len());
        }

        // Print header
        let header_row: Vec<String> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", h, width = col_widths[i]))
            .collect();
        println!("{}", header_row.join("  "));
        println!("{}", "-".repeat(header_row.join("  ").len()));

        // Print rows
        for skill in &report.installed {
            let description = if skill.description.len() > 50 {
                format!("{}...", &skill.description[..47])
            } else {
                skill.description.clone()
            };

            let source = skill.source.as_deref().unwrap_or("local");
            let status_str = format!("{:?}", skill.status);

            let row = [
                format!("{:width$}", skill.id, width = col_widths[0]),
                format!("{:width$}", skill.version, width = col_widths[1]),
                format!("{:width$}", description, width = col_widths[2]),
                format!("{:width$}", source, width = col_widths[3]),
                format!("{:width$}", status_str, width = col_widths[4]),
            ];
            println!("{}", row.join("  "));
        }
    }

    // Print missing dependencies
    if !report.missing.is_empty() {
        println!(
            "\n{}",
            messages::warning("Missing Dependencies (in skills-project.toml but not installed):")
        );
        for entry in &report.missing {
            println!("  • {} (not installed)", entry.id);
        }
    }

    // Print extraneous packages
    if !report.extraneous.is_empty() {
        println!(
            "\n{}",
            messages::warning("Extraneous Packages (installed but not in skills-project.toml):")
        );
        for skill in &report.extraneous {
            println!("  • {} v{}", skill.id, skill.version);
        }
    }

    // Print version mismatches
    if !report.version_mismatches.is_empty() {
        println!(
            "\n{}",
            messages::warning(
                "Version Mismatches (installed version differs from skills-lock.toml):"
            )
        );
        for mismatch in &report.version_mismatches {
            println!(
                "  • {}: installed={}, locked={}",
                mismatch.id, mismatch.installed_version, mismatch.locked_version
            );
        }
    }

    println!();
    Ok(())
}

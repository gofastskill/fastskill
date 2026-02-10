//! Sync command implementation
//!
//! Synchronizes installed skills into AGENTS.md and .cursor/rules/skills.mdc

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::agents_md::{
    generate_rules_content, generate_skills_xml, parse_current_skills, remove_skills_section,
    replace_skills_section, SkillLocation, SkillSummary,
};
use crate::cli::utils::messages;
use clap::Args;
use dirs;
use fastskill::core::skill_manager::SkillDefinition;
use fastskill::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Sync installed skills to AGENTS.md and .cursor/rules/skills.mdc
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Non-interactive mode: include all installed skills
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Path to AGENTS.md (defaults to ./AGENTS.md)
    #[arg(long)]
    pub agents_file: Option<String>,

    /// Do not update .cursor/rules/skills.mdc
    #[arg(long)]
    pub no_rules: bool,

    /// Override path to rules file
    #[arg(long)]
    pub rules_file: Option<String>,
}

/// Execute the sync command
pub async fn execute_sync(service: &FastSkillService, args: SyncArgs) -> CliResult<()> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    let agents_file = resolve_agents_file(&current_dir, args.agents_file)?;
    let rules_file = resolve_rules_file(&current_dir, args.rules_file)?;

    debug!(
        "Agents file: {}, Rules file: {}",
        agents_file.display(),
        rules_file.display()
    );

    // Get global skills directory
    let global_skills_dir = get_global_skills_directory()?;

    // Enumerate installed skills
    let installed_skills = service
        .skill_manager()
        .list_skills(None)
        .await
        .map_err(|e| {
            CliError::Service(fastskill::ServiceError::Custom(format!(
                "Failed to list installed skills: {}",
                e
            )))
        })?;

    debug!("Found {} installed skills", installed_skills.len());

    // Convert to SkillSummary with location info
    let skill_summaries: Vec<SkillSummary> = installed_skills
        .into_iter()
        .map(|skill| {
            let location = determine_skill_location(&skill, &global_skills_dir);
            SkillSummary {
                id: skill.id.to_string(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                location,
            }
        })
        .collect();

    // Read existing AGENTS.md if it exists
    let existing_content = if agents_file.exists() {
        fs::read_to_string(&agents_file).map_err(|e| {
            CliError::Io(std::io::Error::other(format!(
                "Failed to read {}: {}",
                agents_file.display(),
                e
            )))
        })?
    } else {
        String::new()
    };

    // Parse currently selected skills from AGENTS.md
    let current_skill_ids: HashMap<String, bool> = parse_current_skills(&existing_content)
        .into_iter()
        .map(|id| (id.clone(), true))
        .collect();

    debug!("Current skill IDs in AGENTS.md: {:?}", current_skill_ids);

    // Select skills to sync
    let selected_skills = if args.yes {
        // Non-interactive mode: select all skills
        skill_summaries.clone()
    } else {
        // Interactive mode: let user choose
        interactive_select_skills(&skill_summaries, &current_skill_ids)?
    };

    debug!("Selected {} skills", selected_skills.len());

    // Generate and write AGENTS.md
    if selected_skills.is_empty() {
        // Remove skills section
        let new_content = remove_skills_section(&existing_content);
        fs::write(&agents_file, new_content).map_err(|e| {
            CliError::Io(std::io::Error::other(format!(
                "Failed to write {}: {}",
                agents_file.display(),
                e
            )))
        })?;
        println!("{}", messages::ok("Removed all skills from AGENTS.md"));
    } else {
        // Generate and update skills section
        let skills_xml = generate_skills_xml(&selected_skills);
        let new_content = replace_skills_section(&existing_content, &skills_xml);
        fs::write(&agents_file, new_content).map_err(|e| {
            CliError::Io(std::io::Error::other(format!(
                "Failed to write {}: {}",
                agents_file.display(),
                e
            )))
        })?;
        println!(
            "{}",
            messages::ok(&format!(
                "Synced {} skill(s) to AGENTS.md",
                selected_skills.len()
            ))
        );
    }

    // Update .cursor/rules/skills.mdc unless --no-rules
    if !args.no_rules {
        update_rules_file(&rules_file, &selected_skills)?;
    }

    Ok(())
}

/// Resolve AGENTS.md file path
fn resolve_agents_file(current_dir: &Path, override_path: Option<String>) -> CliResult<PathBuf> {
    if let Some(path) = override_path {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(current_dir.join(path))
        }
    } else {
        Ok(current_dir.join("AGENTS.md"))
    }
}

/// Resolve .cursor/rules/skills.mdc file path
fn resolve_rules_file(current_dir: &Path, override_path: Option<String>) -> CliResult<PathBuf> {
    if let Some(path) = override_path {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(current_dir.join(path))
        }
    } else {
        Ok(current_dir.join(".cursor/rules/skills.mdc"))
    }
}

/// Get the global skills directory path
fn get_global_skills_directory() -> CliResult<PathBuf> {
    let config_dir = dirs::config_dir().ok_or_else(|| {
        CliError::Config("Failed to determine system config directory".to_string())
    })?;
    Ok(config_dir.join("fastskill/skills"))
}

/// Determine if a skill is project or global
fn determine_skill_location(skill: &SkillDefinition, global_dir: &Path) -> SkillLocation {
    let skill_dir = skill.skill_file.parent().unwrap_or_else(|| Path::new("."));
    let skill_dir = skill_dir
        .canonicalize()
        .unwrap_or_else(|_| skill_dir.to_path_buf());
    let global_dir = global_dir
        .canonicalize()
        .unwrap_or_else(|_| global_dir.to_path_buf());

    // Check if skill is under global directory
    if skill_dir.starts_with(&global_dir) {
        SkillLocation::Global
    } else {
        SkillLocation::Project
    }
}

/// Interactive skill selection using checkboxes
fn interactive_select_skills(
    all_skills: &[SkillSummary],
    current_ids: &HashMap<String, bool>,
) -> CliResult<Vec<SkillSummary>> {
    use inquire::MultiSelect;

    if all_skills.is_empty() {
        println!("{}", messages::warning("No skills available to sync"));
        return Ok(Vec::new());
    }

    // Create label for each skill and build mapping
    let skill_labels: Vec<String> = all_skills
        .iter()
        .map(|skill| {
            let location_str = if skill.location == SkillLocation::Project {
                "project"
            } else {
                "global"
            };
            format!("{} ({})", skill.id, location_str)
        })
        .collect();

    // Build mapping from label to skill summary
    let label_to_skill: std::collections::HashMap<String, SkillSummary> = skill_labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let skill = &all_skills[i];
            (label.clone(), skill.clone())
        })
        .collect();

    // Default selection: project skills, skills already in AGENTS.md (by index)
    let default_indices: Vec<usize> = all_skills
        .iter()
        .enumerate()
        .filter(|(_i, skill)| {
            skill.location == SkillLocation::Project || current_ids.contains_key(&skill.id)
        })
        .map(|(i, _)| i)
        .collect();

    let message = "Select skills to expose in AGENTS.md";
    let prompt = MultiSelect::new(message, skill_labels)
        .with_default(&default_indices)
        .with_page_size(15)
        .with_help_message("↑/↓: move, Space: select/deselect, Enter: confirm, Esc: cancel");

    match prompt.prompt() {
        Ok(selected_labels) => {
            let selected_skills: Vec<SkillSummary> = selected_labels
                .iter()
                .filter_map(|label| label_to_skill.get(label).cloned())
                .collect();
            Ok(selected_skills)
        }
        Err(inquire::InquireError::OperationCanceled) => {
            println!("{}", messages::info("Cancelled by user"));
            Ok(Vec::new())
        }
        Err(e) => Err(CliError::Config(format!(
            "Interactive selection failed: {}",
            e
        ))),
    }
}

/// Update .cursor/rules/skills.mdc with selected skills
fn update_rules_file(rules_file: &Path, selected_skills: &[SkillSummary]) -> CliResult<()> {
    let rules_dir = rules_file
        .parent()
        .ok_or_else(|| CliError::Config("Invalid rules file path".to_string()))?;

    fs::create_dir_all(rules_dir).map_err(|e| {
        CliError::Io(std::io::Error::other(format!(
            "Failed to create {}: {}",
            rules_dir.display(),
            e
        )))
    })?;

    let content = generate_rules_content(selected_skills);
    fs::write(rules_file, content).map_err(|e| {
        CliError::Io(std::io::Error::other(format!(
            "Failed to write {}: {}",
            rules_file.display(),
            e
        )))
    })?;

    println!("{}", messages::ok("Updated .cursor/rules/skills.mdc"));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_skill_location_project() {
        let global_dir = PathBuf::from("/home/user/.config/fastskill/skills");
        let skill = SkillDefinition::new(
            "test-skill".to_string().into(),
            "Test Skill".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
        );
        let mut skill = skill;
        skill.skill_file = PathBuf::from("/home/user/project/.claude/skills/test-skill/SKILL.md");

        let location = determine_skill_location(&skill, &global_dir);
        assert_eq!(location, SkillLocation::Project);
    }

    #[test]
    fn test_determine_skill_location_global() {
        let global_dir = PathBuf::from("/home/user/.config/fastskill/skills");
        let skill = SkillDefinition::new(
            "test-skill".to_string().into(),
            "Test Skill".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
        );
        let mut skill = skill;
        skill.skill_file = PathBuf::from("/home/user/.config/fastskill/skills/test-skill/SKILL.md");

        let location = determine_skill_location(&skill, &global_dir);
        assert_eq!(location, SkillLocation::Global);
    }

    #[test]
    fn test_resolve_agents_file_default() {
        let current_dir = PathBuf::from("/home/user/project");
        let result = resolve_agents_file(&current_dir, None).unwrap();
        assert_eq!(result, PathBuf::from("/home/user/project/AGENTS.md"));
    }

    #[test]
    fn test_resolve_agents_file_override() {
        let current_dir = PathBuf::from("/home/user/project");
        let result = resolve_agents_file(&current_dir, Some("custom.md".to_string())).unwrap();
        assert_eq!(result, PathBuf::from("/home/user/project/custom.md"));
    }

    #[test]
    fn test_resolve_agents_file_absolute() {
        let current_dir = PathBuf::from("/home/user/project");
        let result =
            resolve_agents_file(&current_dir, Some("/absolute/path/AGENTS.md".to_string()))
                .unwrap();
        assert_eq!(result, PathBuf::from("/absolute/path/AGENTS.md"));
    }

    #[test]
    fn test_resolve_rules_file_default() {
        let current_dir = PathBuf::from("/home/user/project");
        let result = resolve_rules_file(&current_dir, None).unwrap();
        assert_eq!(
            result,
            PathBuf::from("/home/user/project/.cursor/rules/skills.mdc")
        );
    }

    #[test]
    fn test_resolve_rules_file_override() {
        let current_dir = PathBuf::from("/home/user/project");
        let result = resolve_rules_file(&current_dir, Some("custom.mdc".to_string())).unwrap();
        assert_eq!(result, PathBuf::from("/home/user/project/custom.mdc"));
    }
}

//! Sync command implementation
//!
//! Synchronizes installed skills into the agent's metadata file (AGENTS.md, CLAUDE.md, etc.)

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::agents_md::{
    generate_skills_xml, parse_current_skills, remove_skills_section, replace_skills_section,
    SkillLocation, SkillSummary,
};
use crate::cli::utils::messages;
use aikit_sdk::{instruction_file_with_override, validate_agent_key, DeployError};
use clap::Args;
use dirs;
use fastskill::core::skill_manager::SkillDefinition;
use fastskill::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Sync installed skills to the agent's metadata file
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Non-interactive mode: include all installed skills
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// AI agent to target (auto-detected if not specified)
    #[arg(long, short = 'a')]
    pub agent: Option<String>,

    /// Path to metadata file (overrides auto-detection)
    #[arg(long)]
    pub agents_file: Option<String>,
}

/// Execute the sync command
pub async fn execute_sync(service: &FastSkillService, args: SyncArgs) -> CliResult<()> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    let agents_file = resolve_agents_file(&current_dir, args.agents_file, args.agent)?;

    let metadata_file_name = agents_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| agents_file.display().to_string());

    debug!("Agents file: {}", agents_file.display());

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

    // Read existing metadata file if it exists
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

    // Parse currently selected skills from metadata file
    let current_skill_ids: HashMap<String, bool> = parse_current_skills(&existing_content)
        .into_iter()
        .map(|id| (id.clone(), true))
        .collect();

    debug!(
        "Current skill IDs in {}: {:?}",
        metadata_file_name, current_skill_ids
    );

    // Select skills to sync
    let selected_skills = if args.yes {
        // Non-interactive mode: select all skills
        skill_summaries.clone()
    } else {
        // Interactive mode: let user choose
        interactive_select_skills(&skill_summaries, &current_skill_ids, &metadata_file_name)?
    };

    debug!("Selected {} skills", selected_skills.len());

    // Generate and write metadata file
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
        println!(
            "{}",
            messages::ok(&format!("Removed all skills from {}", metadata_file_name))
        );
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
                "Synced {} skill(s) to {}",
                selected_skills.len(),
                metadata_file_name
            ))
        );
    }

    Ok(())
}

/// Resolve the agent metadata file path using aikit-sdk.
///
/// Resolution priority:
/// 1. Explicit `--agents-file` override (absolute path used as-is; relative resolved from `current_dir`)
/// 2. Explicit `--agent` flag (uses the agent's primary instruction file; falls back to AGENTS.md
///    if that file exists but the primary does not)
/// 3. Auto-detect: scans `current_dir` for AGENTS.md, CLAUDE.md, GEMINI.md in that order and
///    returns the first found; defaults to AGENTS.md if none exist
fn resolve_agents_file(
    current_dir: &Path,
    override_path: Option<String>,
    agent_key: Option<String>,
) -> CliResult<PathBuf> {
    let override_path = override_path.as_ref().map(PathBuf::from);

    // `--agents-file` has unconditional precedence; ignore `--agent` validation/resolution in that case.
    let resolved_agent_key = if override_path.is_some() {
        None
    } else {
        agent_key.as_deref()
    };

    // Validate agent key before calling the SDK so the error message is actionable.
    if let Some(key) = resolved_agent_key {
        validate_agent_key(key).map_err(|_| {
            CliError::Config(format!(
                "Invalid agent key '{}'. Valid keys include: claude, cursor-agent, gemini, \
                 codex, windsurf, roo, newton. See https://github.com/goaikit/aikit for the full list.",
                key
            ))
        })?;
    }

    let path = instruction_file_with_override(
        current_dir,
        resolved_agent_key,
        override_path.as_deref(),
    )
    .map_err(|e| match e {
        // AgentNotFound cannot fire here in practice because validate_agent_key above
        // already rejects unknown keys — but kept as a defensive fallback.
        DeployError::AgentNotFound(ref key) => CliError::Config(format!(
            "Agent '{}' not found. Valid keys include: claude, cursor-agent, gemini, codex, windsurf.",
            key
        )),
        DeployError::UnsupportedConcept { ref agent_key, .. } => CliError::Config(format!(
            "Agent '{}' does not support metadata files. Use --agents-file to specify a custom path.",
            agent_key
        )),
        _ => CliError::Config(format!("Failed to resolve metadata file: {}", e)),
    })?;

    debug!("Resolved metadata file: {}", path.display());
    debug!("Agent key: {:?}", resolved_agent_key);
    debug!("Override path: {:?}", override_path);

    Ok(path)
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
    metadata_file_name: &str,
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

    // Default selection: project skills, skills already in metadata file (by index)
    let default_indices: Vec<usize> = all_skills
        .iter()
        .enumerate()
        .filter(|(_i, skill)| {
            skill.location == SkillLocation::Project || current_ids.contains_key(&skill.id)
        })
        .map(|(i, _)| i)
        .collect();

    let message = format!("Select skills to expose in {}", metadata_file_name);
    let prompt = MultiSelect::new(&message, skill_labels)
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use fastskill::SkillId;

    #[test]
    fn test_determine_skill_location_project() {
        let global_dir = PathBuf::from("/home/user/.config/fastskill/skills");
        let skill_id = SkillId::new("test-skill".to_string()).unwrap();
        let skill = SkillDefinition::new(
            skill_id,
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
        let skill_id = SkillId::new("test-skill".to_string()).unwrap();
        let skill = SkillDefinition::new(
            skill_id,
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
    fn test_resolve_agents_file_default_no_files() {
        // Empty directory: SDK defaults to AGENTS.md when no known file exists
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();

        let result = resolve_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("AGENTS.md"));
    }

    #[test]
    fn test_resolve_agents_file_default_claude_md_present() {
        // aikit-sdk scans AGENTS.md first, then CLAUDE.md, then GEMINI.md
        // When only CLAUDE.md exists, it is returned
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();
        std::fs::write(current_dir.join("CLAUDE.md"), "# Claude").unwrap();

        let result = resolve_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("CLAUDE.md"));
    }

    #[test]
    fn test_resolve_agents_file_override() {
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();
        let result = resolve_agents_file(current_dir, Some("custom.md".to_string()), None).unwrap();
        assert_eq!(result, current_dir.join("custom.md"));
    }

    #[test]
    fn test_resolve_agents_file_absolute() {
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();
        let result = resolve_agents_file(
            current_dir,
            Some("/absolute/path/AGENTS.md".to_string()),
            None,
        )
        .unwrap();
        assert_eq!(result, PathBuf::from("/absolute/path/AGENTS.md"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod aikit_integration_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_agents_file_with_explicit_agent() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result = resolve_agents_file(current_dir, None, Some("claude".to_string())).unwrap();
        assert_eq!(result, current_dir.join("CLAUDE.md"));

        let result =
            resolve_agents_file(current_dir, None, Some("cursor-agent".to_string())).unwrap();
        assert_eq!(result, current_dir.join("AGENTS.md"));
    }

    #[test]
    fn test_resolve_agents_file_with_override() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result = resolve_agents_file(current_dir, Some("custom.md".to_string()), None).unwrap();
        assert_eq!(result, current_dir.join("custom.md"));
    }

    #[test]
    fn test_resolve_agents_file_absolute_override() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result = resolve_agents_file(
            current_dir,
            Some("/absolute/path/AGENTS.md".to_string()),
            None,
        )
        .unwrap();
        assert_eq!(result, PathBuf::from("/absolute/path/AGENTS.md"));
    }

    #[test]
    fn test_resolve_agents_file_auto_detect_claude() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        // Create CLAUDE.md
        fs::write(current_dir.join("CLAUDE.md"), "# CLAUDE").unwrap();

        let result = resolve_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("CLAUDE.md"));
    }

    #[test]
    fn test_resolve_agents_file_auto_detect_cursor() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        // Create AGENTS.md
        fs::write(current_dir.join("AGENTS.md"), "# AGENTS").unwrap();

        let result = resolve_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("AGENTS.md"));
    }

    #[test]
    fn test_resolve_agents_file_invalid_agent() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result = resolve_agents_file(current_dir, None, Some("invalid".to_string()));
        assert!(result.is_err());
        match result.unwrap_err() {
            CliError::Config(msg) => {
                assert!(msg.contains("Invalid agent key"));
                assert!(msg.contains("invalid"));
            }
            _ => panic!("Expected Config error"),
        }
    }

    #[test]
    fn test_resolve_agents_file_override_precedence() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        // Create both CLAUDE.md and specify override
        fs::write(current_dir.join("CLAUDE.md"), "# CLAUDE").unwrap();

        let result = resolve_agents_file(
            current_dir,
            Some("override.md".to_string()),
            Some("claude".to_string()),
        )
        .unwrap();

        // Override should take precedence over agent flag and auto-detect
        assert_eq!(result, current_dir.join("override.md"));
    }

    #[test]
    fn test_resolve_agents_file_override_ignores_invalid_agent() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result = resolve_agents_file(
            current_dir,
            Some("override.md".to_string()),
            Some("notarealkey".to_string()),
        )
        .unwrap();

        assert_eq!(result, current_dir.join("override.md"));
    }
}

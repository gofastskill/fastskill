//! Sync command implementation
//!
//! Synchronizes installed skills into the agent's metadata file (AGENTS.md, CLAUDE.md, etc.)

use crate::commands::common::runtime_selection_error_to_cli;
use crate::error::{CliError, CliResult};
use crate::utils::agents_md::{
    generate_skills_xml, parse_current_skills, remove_skills_section, replace_skills_section,
    SkillLocation, SkillSummary,
};
use crate::utils::messages;
use aikit_sdk::{instruction_file_with_override, DeployError};
use clap::Args;
use dirs;
use fastskill_core::core::agent_runtime_selector::RuntimeSelectionInput;
use fastskill_core::core::skill_manager::SkillDefinition;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Sync installed skills into the agent's metadata file (CLAUDE.md, AGENTS.md, etc.).
///
/// This command writes skill identifiers and descriptions into the agent's instruction
/// file so the agent can discover available skills. It does NOT manage filesystem links
/// or symlinks; editable skill links are established by `fastskill add -e` or
/// `fastskill install`.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Non-interactive mode: include all installed skills
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Target runtime(s) for this operation; repeatable (mutually exclusive with --all)
    #[arg(long, short = 'a', action = clap::ArgAction::Append)]
    pub agent: Vec<String>,

    /// Target all runtimes discovered by aikit (mutually exclusive with --agent)
    #[arg(long)]
    pub all: bool,

    /// Path to metadata file (overrides auto-detection)
    #[arg(long)]
    pub agents_file: Option<String>,
}

impl From<&SyncArgs> for RuntimeSelectionInput {
    fn from(args: &SyncArgs) -> Self {
        RuntimeSelectionInput {
            agents: args.agent.clone(),
            all: args.all,
        }
    }
}

/// Execute the sync command
pub async fn execute_sync(service: &FastSkillService, args: SyncArgs) -> CliResult<()> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;

    // `--agents-file` bypasses agent resolution entirely.
    if let Some(ref override_path) = args.agents_file {
        let agents_file = resolve_single_agents_file(&current_dir, None, Some(override_path))?;
        return sync_to_file(service, &args, &agents_file).await;
    }

    // Resolve runtime selection.
    let input = RuntimeSelectionInput::from(&args);
    let selection = fastskill_core::core::agent_runtime_selector::resolve_runtime_selection(&input)
        .map_err(runtime_selection_error_to_cli)?;

    match selection {
        Some(sel) => {
            // Iterate over each resolved runtime and sync to its metadata file.
            for agent_key in &sel.runtimes {
                match resolve_single_agents_file(&current_dir, Some(agent_key.as_str()), None) {
                    Ok(agents_file) => {
                        sync_to_file(service, &args, &agents_file).await?;
                    }
                    Err(CliError::Config(ref msg))
                        if msg.contains("not found")
                            || msg.contains("does not support metadata files") =>
                    {
                        // Skip runtimes that don't support instruction files; warn only.
                        eprintln!("warning: skipping agent '{}': {}", agent_key, msg);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        None => {
            // No --agent or --all provided: fail with RUNTIME_NO_SELECTION.
            return Err(CliError::Config(
                "RUNTIME_NO_SELECTION: No runtime selected. Use --agent <id> or --all to \
                 specify a target runtime."
                    .to_string(),
            ));
        }
    }

    Ok(())
}

/// Perform the skill sync operation for a single metadata file path.
async fn sync_to_file(
    service: &FastSkillService,
    args: &SyncArgs,
    agents_file: &Path,
) -> CliResult<()> {
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
            CliError::Service(fastskill_core::ServiceError::Custom(format!(
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
        fs::read_to_string(agents_file).map_err(|e| {
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
        fs::write(agents_file, new_content).map_err(|e| {
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
        fs::write(agents_file, new_content).map_err(|e| {
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

/// Resolve the agent metadata file path for a single agent key or override path.
///
/// Resolution priority:
/// 1. Explicit override path (absolute used as-is; relative resolved from `current_dir`)
/// 2. Explicit agent key (uses the agent's primary instruction file)
/// 3. Auto-detect: scans `current_dir` for AGENTS.md, CLAUDE.md, GEMINI.md; defaults to AGENTS.md
fn resolve_single_agents_file(
    current_dir: &Path,
    agent_key: Option<&str>,
    override_path: Option<&str>,
) -> CliResult<PathBuf> {
    let override_pb = override_path.map(PathBuf::from);

    let path = instruction_file_with_override(
        current_dir,
        agent_key,
        override_pb.as_deref(),
    )
    .map_err(|e| match e {
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
    use fastskill_core::SkillId;

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
    fn test_resolve_single_agents_file_default_no_files() {
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();

        let result = resolve_single_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("AGENTS.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_default_claude_md_present() {
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();
        std::fs::write(current_dir.join("CLAUDE.md"), "# Claude").unwrap();

        let result = resolve_single_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("CLAUDE.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_override() {
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();
        let result = resolve_single_agents_file(current_dir, None, Some("custom.md")).unwrap();
        assert_eq!(result, current_dir.join("custom.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_absolute() {
        let temp = tempfile::tempdir().unwrap();
        let current_dir = temp.path();
        let result =
            resolve_single_agents_file(current_dir, None, Some("/absolute/path/AGENTS.md"))
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
    fn test_resolve_single_agents_file_with_explicit_agent() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result = resolve_single_agents_file(current_dir, Some("claude"), None).unwrap();
        assert_eq!(result, current_dir.join("CLAUDE.md"));

        let result = resolve_single_agents_file(current_dir, Some("cursor-agent"), None).unwrap();
        assert_eq!(result, current_dir.join("AGENTS.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_with_override() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result = resolve_single_agents_file(current_dir, None, Some("custom.md")).unwrap();
        assert_eq!(result, current_dir.join("custom.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_absolute_override() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        let result =
            resolve_single_agents_file(current_dir, None, Some("/absolute/path/AGENTS.md"))
                .unwrap();
        assert_eq!(result, PathBuf::from("/absolute/path/AGENTS.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_auto_detect_claude() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        fs::write(current_dir.join("CLAUDE.md"), "# CLAUDE").unwrap();

        let result = resolve_single_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("CLAUDE.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_auto_detect_cursor() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        fs::write(current_dir.join("AGENTS.md"), "# AGENTS").unwrap();

        let result = resolve_single_agents_file(current_dir, None, None).unwrap();
        assert_eq!(result, current_dir.join("AGENTS.md"));
    }

    #[test]
    fn test_resolve_single_agents_file_override_precedence() {
        let temp = TempDir::new().unwrap();
        let current_dir = temp.path();

        fs::write(current_dir.join("CLAUDE.md"), "# CLAUDE").unwrap();

        let result =
            resolve_single_agents_file(current_dir, Some("claude"), Some("override.md")).unwrap();

        assert_eq!(result, current_dir.join("override.md"));
    }

    #[test]
    fn test_runtime_selection_input_from_sync_args() {
        let args = SyncArgs {
            yes: false,
            agent: vec!["claude".to_string(), "codex".to_string()],
            all: false,
            agents_file: None,
        };
        let input = RuntimeSelectionInput::from(&args);
        assert_eq!(input.agents, vec!["claude", "codex"]);
        assert!(!input.all);
    }

    #[test]
    fn test_runtime_selection_input_all_flag() {
        let args = SyncArgs {
            yes: false,
            agent: vec![],
            all: true,
            agents_file: None,
        };
        let input = RuntimeSelectionInput::from(&args);
        assert!(input.agents.is_empty());
        assert!(input.all);
    }
}

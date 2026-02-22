//! E2E tests for sync command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_sync_all_skills_yes() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create test skills
    for skill_name in &["skill1", "skill2"] {
        let skill_dir = skills_dir.join(skill_name);
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = format!(
            r#"---
name: {}
description: A test skill
version: 1.0.0
---
# {} Skill

This is a test skill for sync command.
"#,
            skill_name, skill_name
        );
        fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();
    }

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Create initial AGENTS.md
    fs::write(
        temp_dir.path().join("AGENTS.md"),
        "# AGENTS.md\n\nThis file contains agent instructions.\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["sync", "--yes"], Some(temp_dir.path()));

    assert!(result.success);

    // Verify AGENTS.md was updated
    let agents_md_content = fs::read_to_string(temp_dir.path().join("AGENTS.md")).unwrap();
    assert!(agents_md_content.contains("<skills_system"));
    assert!(agents_md_content.contains("<name>skill1</name>"));
    assert!(agents_md_content.contains("<name>skill2</name>"));

    assert_snapshot_with_settings(
        "sync_all_skills_yes",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_project_vs_global() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create project skill
    let project_skill_dir = skills_dir.join("project-skill");
    fs::create_dir_all(&project_skill_dir).unwrap();
    let project_skill = r#"---
name: project-skill
description: A project-level skill
version: 1.0.0
---
# Project Skill

This skill is at project level.
"#;
    fs::write(project_skill_dir.join("SKILL.md"), project_skill).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Create AGENTS.md
    fs::write(temp_dir.path().join("AGENTS.md"), "# AGENTS.md\n\n").unwrap();

    let result = run_fastskill_command(&["sync", "--yes"], Some(temp_dir.path()));

    assert!(result.success);

    // Verify project skill is marked as location="project"
    let agents_md_content = fs::read_to_string(temp_dir.path().join("AGENTS.md")).unwrap();
    assert!(agents_md_content.contains("<name>project-skill</name>"));
    assert!(agents_md_content.contains("<location>project</location>"));

    assert_snapshot_with_settings(
        "sync_project_vs_global",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_with_existing_agents_md() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create test skill
    let skill_dir = skills_dir.join("new-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: new-skill
description: A new skill
version: 1.0.0
---
# New Skill

This is a new skill.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Create AGENTS.md with existing content
    let initial_agents_md = r#"# AGENTS.md

## Project Setup

This file contains project-specific agent instructions.

<skills_system priority="1">
<available_skills>
<skill>
  <name>old-skill</name>
  <description>Old skill</description>
  <location>project</location>
</skill>
</available_skills>
</skills_system>

## Additional Instructions

Additional configuration and instructions for agents.
"#;
    fs::write(temp_dir.path().join("AGENTS.md"), initial_agents_md).unwrap();

    let result = run_fastskill_command(&["sync", "--yes"], Some(temp_dir.path()));

    assert!(result.success);

    // Verify skills section was replaced but other content preserved
    let agents_md_content = fs::read_to_string(temp_dir.path().join("AGENTS.md")).unwrap();
    assert!(agents_md_content.contains("# AGENTS.md"));
    assert!(agents_md_content.contains("## Project Setup"));
    assert!(agents_md_content.contains("## Additional Instructions"));
    assert!(!agents_md_content.contains("old-skill"));
    assert!(agents_md_content.contains("<name>new-skill</name>"));

    assert_snapshot_with_settings(
        "sync_with_existing_agents_md",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_remove_all_skills() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Create AGENTS.md with skills section
    let initial_agents_md = r#"# AGENTS.md

<skills_system priority="1">
<available_skills>
<skill>
  <name>skill-to-remove</name>
  <description>Should be removed</description>
  <location>project</location>
</skill>
</available_skills>
</skills_system>

Additional content here.
"#;
    fs::write(temp_dir.path().join("AGENTS.md"), initial_agents_md).unwrap();

    let result = run_fastskill_command(&["sync", "--yes"], Some(temp_dir.path()));

    assert!(result.success);

    // Verify skills section was removed but other content preserved
    let agents_md_content = fs::read_to_string(temp_dir.path().join("AGENTS.md")).unwrap();
    assert!(agents_md_content.contains("# AGENTS.md"));
    assert!(!agents_md_content.contains("<skills_system"));
    assert!(!agents_md_content.contains("skill-to-remove"));
    assert!(agents_md_content.contains("Additional content here"));

    assert_snapshot_with_settings(
        "sync_remove_all_skills",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_custom_files() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create test skill
    let skill_dir = skills_dir.join("custom-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: custom-skill
description: A custom skill
version: 1.0.0
---
# Custom Skill

Custom skill content.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &["sync", "--yes", "--agents-file", "CUSTOM.md"],
        Some(temp_dir.path()),
    );

    assert!(result.success);

    // Verify custom AGENTS.md was created
    let custom_agents = temp_dir.path().join("CUSTOM.md");
    assert!(custom_agents.exists());
    let content = fs::read_to_string(&custom_agents).unwrap();
    assert!(content.contains("<skills_system"));

    assert_snapshot_with_settings(
        "sync_custom_files",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_with_empty_skills_list() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Create AGENTS.md
    fs::write(temp_dir.path().join("AGENTS.md"), "# AGENTS.md\n\n").unwrap();

    let result = run_fastskill_command(&["sync", "--yes"], Some(temp_dir.path()));

    assert!(result.success);

    // When no skills are available, the skills section should be removed
    let _agents_md_content = fs::read_to_string(temp_dir.path().join("AGENTS.md")).unwrap();
    // The message should indicate removal
    assert!(result.stdout.contains("Removed all skills") || result.stdout.contains("No skills"));

    assert_snapshot_with_settings(
        "sync_empty_skills_list",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_auto_detects_claude_metadata_file() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a skill
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: test-skill\ndescription: A test skill\nversion: 1.0.0\n---\n# Test Skill\n",
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Create CLAUDE.md (not AGENTS.md) — aikit-sdk auto-detects by file existence,
    // not by installed agent directories. AGENTS.md is checked first; since it does
    // not exist here, CLAUDE.md is picked up instead.
    fs::write(temp_dir.path().join("CLAUDE.md"), "# Claude instructions\n").unwrap();

    let result = run_fastskill_command(&["sync", "--yes"], Some(temp_dir.path()));

    assert!(result.success);

    // CLAUDE.md should be updated with skills section
    let claude_md = temp_dir.path().join("CLAUDE.md");
    let content = fs::read_to_string(&claude_md).unwrap();
    assert!(
        content.contains("<skills_system"),
        "CLAUDE.md must contain skills section"
    );
    assert!(content.contains("<name>test-skill</name>"));

    // AGENTS.md must NOT be created as a side effect
    assert!(
        !temp_dir.path().join("AGENTS.md").exists(),
        "AGENTS.md must not be created when CLAUDE.md was detected"
    );

    // The success message must reference CLAUDE.md (dynamic filename, not hardcoded "AGENTS.md")
    assert!(
        result.stdout.contains("CLAUDE.md"),
        "stdout success message must reference CLAUDE.md, got: {}",
        result.stdout
    );

    assert_snapshot_with_settings(
        "sync_auto_detects_claude_metadata_file",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_with_explicit_agent_flag() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: test-skill\ndescription: A test skill\nversion: 1.0.0\n---\n# Test Skill\n",
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // No metadata file exists — explicit --agent flag determines which file to create
    let result = run_fastskill_command(
        &["sync", "--yes", "--agent", "claude"],
        Some(temp_dir.path()),
    );

    assert!(result.success);

    let claude_md = temp_dir.path().join("CLAUDE.md");
    assert!(
        claude_md.exists(),
        "CLAUDE.md must be created by --agent claude"
    );
    let content = fs::read_to_string(&claude_md).unwrap();
    assert!(content.contains("<skills_system"));

    assert!(!temp_dir.path().join("AGENTS.md").exists());

    assert!(
        result.stdout.contains("CLAUDE.md"),
        "stdout success message must reference CLAUDE.md, got: {}",
        result.stdout
    );

    assert_snapshot_with_settings(
        "sync_with_explicit_agent_flag",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sync_error_for_unsupported_agent() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Copilot is a known agent key (validate_agent_key passes) but it has no instruction_file
    // in the catalog, so instruction_file_with_override returns UnsupportedConcept
    let result = run_fastskill_command(
        &["sync", "--yes", "--agent", "copilot"],
        Some(temp_dir.path()),
    );

    assert!(!result.success, "sync with --agent copilot must fail");
    assert!(
        result.stdout.contains("does not support metadata files")
            || result.stderr.contains("does not support metadata files"),
        "error must explain that copilot does not support metadata files"
    );
    assert!(
        result.stdout.contains("--agents-file") || result.stderr.contains("--agents-file"),
        "error must suggest --agents-file as alternative"
    );
}

#[test]
fn test_sync_error_for_unknown_agent_key() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // An entirely unknown key must be rejected by the pre-flight validate_agent_key check
    let result = run_fastskill_command(
        &["sync", "--yes", "--agent", "notarealkey"],
        Some(temp_dir.path()),
    );

    assert!(!result.success, "sync with unknown agent key must fail");
    assert!(
        result.stdout.contains("Invalid agent key") || result.stderr.contains("Invalid agent key"),
        "error must identify the bad key"
    );
}

#[test]
fn test_sync_override_takes_precedence_over_invalid_agent() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: test-skill\ndescription: A test skill\nversion: 1.0.0\n---\n# Test Skill\n",
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "sync",
            "--yes",
            "--agents-file",
            "CUSTOM.md",
            "--agent",
            "notarealkey",
        ],
        Some(temp_dir.path()),
    );

    assert!(
        result.success,
        "--agents-file override must take precedence over invalid --agent: stdout={} stderr={}",
        result.stdout, result.stderr
    );
    assert!(temp_dir.path().join("CUSTOM.md").exists());
    assert!(!result.stdout.contains("Invalid agent key"));
    assert!(!result.stderr.contains("Invalid agent key"));
}

#[test]
fn test_sync_no_agents_md_creates_file() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create test skill
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: test-skill
description: A test skill
version: 1.0.0
---
# Test Skill

Test skill content.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Note: We don't create AGENTS.md here
    let agents_path = temp_dir.path().join("AGENTS.md");
    assert!(!agents_path.exists());

    let result = run_fastskill_command(&["sync", "--yes"], Some(temp_dir.path()));

    assert!(result.success);

    // Verify AGENTS.md was created
    assert!(agents_path.exists());
    let agents_md_content = fs::read_to_string(&agents_path).unwrap();
    assert!(agents_md_content.contains("<skills_system"));

    assert_snapshot_with_settings(
        "sync_no_agents_md_creates_file",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

//! Integration tests for fastskill init command

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::core::manifest::SkillProjectToml;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn get_binary_path() -> String {
    format!("{}/target/debug/fastskill", env!("CARGO_MANIFEST_DIR"))
}

/// T037: Test fastskill init creating skill-project.toml with metadata
#[test]
fn test_init_creates_skill_project_toml_with_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md to indicate skill-level context
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
version: 1.0.0
description: A test skill
author: Test Author
tags:
  - test
  - example
capabilities:
  - capability1
---
# Test Skill
"#,
    )
    .unwrap();

    // Run init command with --yes flag to skip prompts
    let output = Command::new(get_binary_path())
        .arg("init")
        .arg("--yes")
        .arg("--version")
        .arg("1.0.0")
        .arg("--description")
        .arg("A test skill")
        .arg("--author")
        .arg("Test Author")
        .current_dir(&skill_dir)
        .output()
        .expect("Failed to execute init command");

    assert!(
        output.status.success(),
        "init should succeed. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify skill-project.toml was created
    let project_file = skill_dir.join("skill-project.toml");
    assert!(
        project_file.exists(),
        "skill-project.toml should be created"
    );

    // Verify it contains metadata
    let content = fs::read_to_string(&project_file).unwrap();
    let project: SkillProjectToml = toml::from_str(&content).unwrap();

    assert!(project.metadata.is_some());
    let metadata = project.metadata.as_ref().unwrap();
    assert_eq!(metadata.id, Some("test-skill".to_string()));
    assert_eq!(metadata.version, Some("1.0.0".to_string()));
    assert_eq!(metadata.description, Some("A test skill".to_string()));
    assert_eq!(metadata.author, Some("Test Author".to_string()));
}

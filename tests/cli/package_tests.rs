//! Integration tests for fastskill package command

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::packaging::package_skill;
use std::fs;
use tempfile::TempDir;

/// T038: Test fastskill package reading metadata from skill-project.toml
#[test]
fn test_package_reads_metadata_from_skill_project_toml() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md (required for packaging) with proper YAML frontmatter
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: test-skill
description: A test skill for packaging
version: 1.0.0
author: Test Author
tags:
  - test
  - packaging
capabilities:
  - testing
---

# Test Skill

This is a test skill for packaging tests.
"#,
    )
    .unwrap();

    // Create skill-project.toml with metadata
    fs::write(
        skill_dir.join("skill-project.toml"),
        r#"
[metadata]
id = "test-skill"
version = "1.0.0"
description = "A test skill"
author = "Test Author"
"#,
    )
    .unwrap();

    // Create output directory
    let output_dir = temp_dir.path().join("output");
    fs::create_dir_all(&output_dir).unwrap();

    // Package the skill
    let result = package_skill(&skill_dir, &output_dir, "1.0.0");
    assert!(result.is_ok(), "packaging should succeed");

    // Verify ZIP was created
    let zip_path = result.unwrap();
    assert!(zip_path.exists(), "ZIP file should be created");
    assert!(zip_path.to_string_lossy().contains("test-skill-1.0.0.zip"));
}

/// T039: Test skill dependencies in skill-project.toml
#[test]
fn test_skill_dependencies_in_skill_project_toml() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md (required for packaging)
    fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n").unwrap();

    // Create skill-project.toml with metadata and dependencies
    fs::write(
        skill_dir.join("skill-project.toml"),
        r#"
[metadata]
id = "test-skill"
version = "1.0.0"

[dependencies]
dependency1 = "1.0.0"
dependency2 = { source = "git", url = "https://github.com/org/dep2.git" }
"#,
    )
    .unwrap();

    // Verify dependencies can be read
    let content = fs::read_to_string(skill_dir.join("skill-project.toml")).unwrap();
    let project: SkillProjectToml = toml::from_str(&content).unwrap();

    assert!(project.dependencies.is_some());
    let deps = project.dependencies.as_ref().unwrap();
    assert_eq!(deps.dependencies.len(), 2);
    assert!(deps.dependencies.contains_key("dependency1"));
    assert!(deps.dependencies.contains_key("dependency2"));
}

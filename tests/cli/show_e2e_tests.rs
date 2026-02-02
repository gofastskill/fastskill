//! E2E tests for show command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_show_all_skills() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a test skill in .skills directory
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: test-skill
description: A test skill for show command
version: 1.0.0
tags: [test, show]
capabilities: [show_capability]
---
# Test Skill

This is a test skill for show command E2E testing.

## Usage

Usage example for show command.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    let result = run_fastskill_command(&["show"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("test-skill") || result.stdout.contains("No skills"));

    assert_snapshot_with_settings("show_all_skills", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_show_specific_skill() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a test skill in .skills directory
    let skill_dir = skills_dir.join("web-scraper");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: web-scraper
description: A web scraping skill
version: 1.2.3
tags: [scraping, web]
capabilities: [http_client]
---
# Web Scraper Skill

Advanced web scraping capabilities.

## Features

- Extract structured data
- Handle pagination
- Support multiple formats
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    let result = run_fastskill_command(&["show", "web-scraper"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("web-scraper") && result.stdout.contains("1.2.3"));

    assert_snapshot_with_settings(
        "show_specific_skill",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_show_with_dependency_tree() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a complex skill with dependencies
    let skill_dir = skills_dir.join("complex-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: complex-skill
description: A complex skill with dependencies
version: 2.0.0
tags: [complex, dependencies]
dependencies:
  - web-scraper
  - test-skill
capabilities: [complex_capability]
---
# Complex Skill

This skill has dependencies.

## Usage

Requires web-scraper and test-skill.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Also create the dependencies
    let dep1_dir = skills_dir.join("web-scraper");
    fs::create_dir_all(&dep1_dir).unwrap();
    let dep1_content = r#"---
name: web-scraper
description: A web scraping skill
version: 1.2.3
---
# Web Scraper
"#;
    fs::write(dep1_dir.join("SKILL.md"), dep1_content).unwrap();

    let dep2_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&dep2_dir).unwrap();
    let dep2_content = r#"---
name: test-skill
description: Test skill
version: 1.0.0
---
# Test Skill
"#;
    fs::write(dep2_dir.join("SKILL.md"), dep2_content).unwrap();

    let result = run_fastskill_command(&["show", "--tree", "complex-skill"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("complex-skill") || result.stdout.contains("tree"));

    assert_snapshot_with_settings(
        "show_dependency_tree",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_show_nonexistent_skill_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["show", "nonexistent-skill"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("not found"));

    assert_snapshot_with_settings(
        "show_nonexistent_skill",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_show_invalid_skill_id_format() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["show", "invalid skill id!"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("Invalid"));

    assert_snapshot_with_settings(
        "show_invalid_id_format",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

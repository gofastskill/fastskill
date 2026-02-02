//! E2E tests for list command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_list_default_grid_format() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    // Should show no skills installed in empty directory

    assert_snapshot_with_settings(
        "list_default_grid_format",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_list_json_format() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["list", "--json"], Some(temp_dir.path()));

    assert!(result.success);
    // Should output valid JSON with empty arrays
    assert!(result.stdout.contains("installed") && result.stdout.contains("[]"));

    assert_snapshot_with_settings("list_json_format", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_list_empty_skills_directory() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("No skills installed") || result.stdout.contains("[INFO]"));

    assert_snapshot_with_settings(
        "list_empty_skills_directory",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_list_missing_dependencies() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml with dependencies but no installed skills
    let project_content = r#"[project]
name = "test-project"
version = "1.0.0"

[dependencies]
web-scraper = "1.2.3"
dev-tools = "2.0.0"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), project_content).unwrap();

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("missing") || result.stdout.contains("Missing"));

    assert_snapshot_with_settings(
        "list_missing_dependencies",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_list_version_mismatch() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create an installed skill
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: test-skill
description: Test skill
version: 1.0.0
tags: [test]
---
# Test Skill
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create skill-project.toml with different version
    let project_content = r#"[project]
name = "test-project"
version = "1.0.0"

[dependencies]
test-skill = "2.0.0"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), project_content).unwrap();

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("test-skill") && result.stdout.contains("1.0.0"));

    assert_snapshot_with_settings(
        "list_version_mismatch",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_list_conflicting_flags_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["list", "--json", "--grid"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("cannot be used with"));

    assert_snapshot_with_settings(
        "list_conflicting_flags",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

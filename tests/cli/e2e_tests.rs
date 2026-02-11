//! End-to-end tests for CLI functionality

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]
//!
//! These tests simulate complete workflows through the CLI binary.
//! Note: These tests require the CLI binary to be built first.

use std::fs;
use std::path::Path;
use tempfile::TempDir;
use super::snapshot_helpers::{
    run_fastskill_command, cli_snapshot_settings, assert_snapshot_with_settings
};

/// Create a test skill with valid SKILL.md in the given directory
fn create_test_skill(dir: &Path, skill_name: &str) {
    let skill_dir = dir.join(skill_name);
    fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = skill_dir.join("SKILL.md");
    let content = format!(r#"---
name: {}
description: A test skill for E2E testing
version: 1.0.0
tags: [test, e2e]
capabilities: [test_capability]
---
# {}

This is a test skill for end-to-end CLI testing.
"#, skill_name, skill_name);

    fs::write(&skill_md, content).unwrap();
}

#[test]
fn test_e2e_add_search_remove_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    let skill_name = "test-skill-e2e";

    // Create a test skill
    create_test_skill(&skills_dir, skill_name);

    // Test 1: Add skill from folder
    let result = run_fastskill_command(&["add", &skills_dir.join(skill_name).to_string_lossy().to_string()], Some(&skills_dir));

    assert_snapshot_with_settings("e2e_add_skill", &format!("{}{}", result.stdout, result.stderr), &cli_snapshot_settings());

    // Test 2: Search for the skill
    let result = run_fastskill_command(&[skill_name], Some(&skills_dir));

    assert_snapshot_with_settings("e2e_search_skill", &result.stdout, &cli_snapshot_settings());

    // Test 3: Try to remove the skill (with force to avoid confirmation)
    let result = run_fastskill_command(&["remove", "--force", skill_name], Some(&skills_dir));

    assert_snapshot_with_settings("e2e_remove_skill", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_e2e_help_and_validation() {
    // Test help output
    let result = run_fastskill_command(&["--help"], None);

    assert!(result.success);
    assert_snapshot_with_settings("e2e_help_output", &result.stdout, &cli_snapshot_settings());

    // Test invalid command
    let result = run_fastskill_command(&["invalid-command"], None);

    assert_snapshot_with_settings("e2e_invalid_command", &format!("{}{}", result.stdout, result.stderr), &cli_snapshot_settings());
}

#[test]
fn test_e2e_directory_resolution() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Test that CLI can be invoked from directory with .skills
    let result = run_fastskill_command(&["--help"], Some(temp_dir.path()));

    assert!(result.success);
    assert_snapshot_with_settings("e2e_directory_resolution", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_e2e_command_help() {
    // Test individual command help
    let commands = vec!["add", "search", "disable", "remove"];

    for cmd in commands {
        let result = run_fastskill_command(&[cmd, "--help"], None);

        assert!(result.success, "Help for {} command failed", cmd);
        assert!(!result.stdout.is_empty(), "Help for {} command was empty", cmd);
        assert_snapshot_with_settings(&format!("e2e_{}_help", cmd), &result.stdout, &cli_snapshot_settings());
    }
}

#[test]
fn test_e2e_invalid_skills_dir() {
    let invalid_dir = "/definitely/does/not/exist/skills";

    // Test that CLI handles invalid skills directory gracefully
    let result = run_fastskill_command(&["--skills-dir", invalid_dir, "--help"], None);

    // Should still show help even with invalid directory
    assert_snapshot_with_settings("e2e_invalid_skills_dir", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_e2e_search_with_format_options() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Test search with different format options
    let formats = vec!["table", "json"];

    for format in formats {
        let result = run_fastskill_command(&["search", "nonexistent", "--format", format], Some(&skills_dir));

        // Should succeed (even if no results found)
        assert_snapshot_with_settings(&format!("e2e_search_format_{}", format), &result.stdout, &cli_snapshot_settings());
    }
}

//! End-to-end tests for CLI edge cases and error handling

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use crate::snapshot_helpers::{
    run_fastskill_command, cli_snapshot_settings, assert_snapshot_with_settings
};
use std::env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn get_binary_path() -> String {
    format!("{}/target/debug/fastskill", env!("CARGO_MANIFEST_DIR"))
}

/// Create a skill with invalid SKILL.md
fn create_invalid_skill(dir: &Path, skill_name: &str) {
    let skill_dir = dir.join(skill_name);
    fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = skill_dir.join("SKILL.md");
    let content = format!(r#"---
name: {}
description: Invalid skill - missing closing ---
tags: [test]
---
# {}

This skill is missing the closing frontmatter delimiter.
"#, skill_name, skill_name);

    fs::write(&skill_md, content).unwrap();
}

/// Create a skill without SKILL.md
fn create_skill_without_md(dir: &Path, skill_name: &str) {
    let skill_dir = dir.join(skill_name);
    fs::create_dir_all(&skill_dir).unwrap();

    // Create some other files but no SKILL.md
    let other_file = skill_dir.join("README.txt");
    fs::write(&other_file, "This is just a readme").unwrap();
}

#[test]
fn test_e2e_invalid_skill_handling() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Create invalid skill
    create_invalid_skill(&skills_dir, "invalid-skill");

    // Try to add invalid skill
    let result = run_fastskill_command(&["add", &skills_dir.join("invalid-skill").to_string_lossy().to_string()], Some(&skills_dir));

    // Should fail gracefully
    assert_snapshot_with_settings("e2e_invalid_skill", &format!("{}{}", result.stdout, result.stderr), &cli_snapshot_settings());

    // Create skill without SKILL.md
    create_skill_without_md(&skills_dir, "no-md-skill");

    let result = run_fastskill_command(&["add", &skills_dir.join("no-md-skill").to_string_lossy().to_string()], Some(&skills_dir));

    // Should fail gracefully
    assert_snapshot_with_settings("e2e_skill_without_md", &format!("{}{}", result.stdout, result.stderr), &cli_snapshot_settings());
}

#[test]
fn test_e2e_duplicate_skill_handling() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    let skill_name = "duplicate-skill";

    // Create and add first skill
    create_test_skill(&skills_dir, skill_name);

    // Add first time
    let _result1 = run_fastskill_command(&["add", &skills_dir.join(skill_name).to_string_lossy().to_string()], Some(&skills_dir));

    // Try to add again without force
    let result2 = run_fastskill_command(&["add", &skills_dir.join(skill_name).to_string_lossy().to_string()], Some(&skills_dir));

    // Should fail (skill already exists)
    assert_snapshot_with_settings("e2e_duplicate_skill_no_force", &format!("{}{}", result2.stdout, result2.stderr), &cli_snapshot_settings());

    // Try to add again with force
    let result3 = run_fastskill_command(&["add", "--force", &skills_dir.join(skill_name).to_string_lossy().to_string()], Some(&skills_dir));

    // Should succeed with force
    assert_snapshot_with_settings("e2e_duplicate_skill_with_force", &format!("{}{}", result3.stdout, result3.stderr), &cli_snapshot_settings());
        let stderr = String::from_utf8_lossy(&output3.stderr);
        println!("Force add failed: {}", stderr);
        // Continue test - might fail due to compilation issues
    }
}

#[test]
fn test_e2e_nonexistent_commands() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Test various invalid operations
    let invalid_commands = vec![
        (vec!["disable", "nonexistent-skill"], "e2e_disable_nonexistent"),
        (vec!["remove", "nonexistent-skill"], "e2e_remove_nonexistent"),
        (vec!["add", "/nonexistent/path"], "e2e_add_nonexistent_path"),
    ];

    for (cmd_args, snapshot_name) in invalid_commands {
        let result = run_fastskill_command(&cmd_args, Some(&skills_dir));

        // Should fail gracefully (non-zero exit code expected)
        assert_snapshot_with_settings(snapshot_name, &format!("{}{}", result.stdout, result.stderr), &cli_snapshot_settings());
    }
}

#[test]
fn test_e2e_permission_issues() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Try to use a directory we can't write to
    let readonly_dir = "/etc"; // Should be read-only for regular users

    let result = run_fastskill_command(&["search", "test"], Some(std::path::Path::new(readonly_dir)));

    // Should handle gracefully (might succeed if /etc has no skills)
    assert_snapshot_with_settings("e2e_permission_issues", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_e2e_empty_and_whitespace_handling() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Test search with empty query
    let result1 = run_fastskill_command(&["search", ""], Some(&skills_dir));

    // Should succeed (empty search might return all results)
    assert_snapshot_with_settings("e2e_empty_query", &result1.stdout, &cli_snapshot_settings());

    // Test search with whitespace query
    let result2 = run_fastskill_command(&["search", "   "], Some(&skills_dir));

    // Should succeed
    assert_snapshot_with_settings("e2e_whitespace_query", &result2.stdout, &cli_snapshot_settings());
}

/// Create a test skill with valid SKILL.md in the given directory
fn create_test_skill(dir: &Path, skill_name: &str) {
    let skill_dir = dir.join(skill_name);
    fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = skill_dir.join("SKILL.md");
    let content = format!(r#"---
name: {}
description: A test skill for edge case testing
version: 1.0.0
tags: [test, edge-case]
capabilities: [test_capability]
---
# {}

This is a test skill for edge case CLI testing.
"#, skill_name, skill_name);

    fs::write(&skill_md, content).unwrap();
}

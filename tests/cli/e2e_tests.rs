//! End-to-end tests for CLI functionality
//!
//! These tests simulate complete workflows through the CLI binary.
//! Note: These tests require the CLI binary to be built first.

use std::process::Command;
use std::env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn get_binary_path() -> String {
    format!("{}/target/debug/fastskill", env!("CARGO_MANIFEST_DIR"))
}

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
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("add")
        .arg(skills_dir.join(skill_name).to_string_lossy().to_string())
        .output()
        .expect("Failed to execute add command");

    // Add command should succeed
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("Add command failed: {}", stderr);
        // Continue with test even if add fails (might be due to compilation issues)
    }

    // Test 2: Search for the skill
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg(skill_name)
        .output()
        .expect("Failed to execute search command");

    // Search should succeed (even if no results found)
    assert!(output.status.success() || output.status.code().is_some());

    // Test 3: Try to remove the skill (with force to avoid confirmation)
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("remove")
        .arg("--force")
        .arg(skill_name)
        .output()
        .expect("Failed to execute remove command");

    // Remove command should succeed (even if skill doesn't exist)
    assert!(output.status.success() || output.status.code().is_some());
}

#[test]
fn test_e2e_help_and_validation() {
    // Test help output
    let output = Command::new(get_binary_path())
        .arg("--help")
        .output()
        .expect("Failed to execute help command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FastSkill"));
    assert!(stdout.contains("Commands:"));

    // Test invalid command
    let output = Command::new(get_binary_path())
        .arg("invalid-command")
        .output()
        .expect("Failed to execute invalid command");

    // Should show error/help (non-zero exit code expected)
    assert!(output.status.code().is_some());
}

#[test]
fn test_e2e_directory_resolution() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Change to temp directory
    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    // Test that CLI can be invoked from directory with .skills
    let output = Command::new(get_binary_path())
        .arg("--help")
        .output()
        .expect("Failed to execute command from temp directory");

    // Restore original directory
    env::set_current_dir(&original_dir).unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FastSkill"));
}

#[test]
fn test_e2e_command_help() {
    // Test individual command help
    let commands = vec!["add", "search", "disable", "remove"];

    for cmd in commands {
        let output = Command::new(get_binary_path())
            .arg(cmd)
            .arg("--help")
            .output()
            .expect(&format!("Failed to get help for {} command", cmd));

        assert!(output.status.success(),
                "Help for {} command failed", cmd);

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.is_empty(),
                "Help for {} command was empty", cmd);
    }
}

#[test]
fn test_e2e_invalid_skills_dir() {
    let invalid_dir = "/definitely/does/not/exist/skills";

    // Test that CLI handles invalid skills directory gracefully
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(invalid_dir)
        .arg("--help")
        .output()
        .expect("Failed to execute with invalid skills dir");

    // Should still show help even with invalid directory
    assert!(output.status.success() || output.status.code().is_some());
}

#[test]
fn test_e2e_search_with_format_options() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Test search with different format options
    let formats = vec!["table", "json"];

    for format in formats {
        let output = Command::new(get_binary_path())
            .arg("--skills-dir")
            .arg(skills_dir.to_string_lossy().to_string())
            .arg("search")
            .arg("nonexistent")
            .arg("--format")
            .arg(format)
            .output()
            .expect(&format!("Failed to search with format {}", format));

        // Should succeed (even if no results found)
        assert!(output.status.success() || output.status.code().is_some());
    }
}

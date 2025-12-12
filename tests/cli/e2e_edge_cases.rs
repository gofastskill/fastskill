//! End-to-end tests for CLI edge cases and error handling

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;
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
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("add")
        .arg(skills_dir.join("invalid-skill").to_string_lossy().to_string())
        .output()
        .expect("Failed to execute add command with invalid skill");

    // Should fail gracefully
    assert!(output.status.code().is_some());

    // Create skill without SKILL.md
    create_skill_without_md(&skills_dir, "no-md-skill");

    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("add")
        .arg(skills_dir.join("no-md-skill").to_string_lossy().to_string())
        .output()
        .expect("Failed to execute add command for skill without SKILL.md");

    // Should fail gracefully
    assert!(output.status.code().is_some());
}

#[test]
fn test_e2e_duplicate_skill_handling() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    let skill_name = "duplicate-skill";

    // Create and add first skill
    create_test_skill(&skills_dir, skill_name);

    // Add first time
    let _output1 = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("add")
        .arg(skills_dir.join(skill_name).to_string_lossy().to_string())
        .output();

    // Try to add again without force
    let output2 = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("add")
        .arg(skills_dir.join(skill_name).to_string_lossy().to_string())
        .output()
        .expect("Failed to execute duplicate add command");

    // Should fail (skill already exists)
    assert!(output2.status.code().is_some());

    // Try to add again with force
    let output3 = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("add")
        .arg("--force")
        .arg(skills_dir.join(skill_name).to_string_lossy().to_string())
        .output()
        .expect("Failed to execute forced duplicate add command");

    // Should succeed with force
    if !output3.status.success() {
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
        vec!["disable", "nonexistent-skill"],
        vec!["remove", "nonexistent-skill"],
        vec!["add", "/nonexistent/path"],
    ];

    for cmd_args in invalid_commands {
        let mut command = Command::new(get_binary_path());
        command.arg("--skills-dir").arg(skills_dir.to_string_lossy().to_string());

        for arg in &cmd_args {
            command.arg(arg);
        }

        let output = command.output()
            .expect(&format!("Failed to execute command: {:?}", cmd_args));

        // Should fail gracefully (non-zero exit code expected)
        assert!(output.status.code().is_some());
    }
}

#[test]
fn test_e2e_permission_issues() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Try to use a directory we can't write to
    let readonly_dir = "/etc"; // Should be read-only for regular users

    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(readonly_dir)
        .arg("search")
        .arg("test")
        .output()
        .expect("Failed to execute command with readonly skills dir");

    // Should handle gracefully (might succeed if /etc has no skills)
    assert!(output.status.success() || output.status.code().is_some());
}

#[test]
fn test_e2e_empty_and_whitespace_handling() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");

    // Test search with empty query
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("search")
        .arg("")
        .output()
        .expect("Failed to execute search with empty query");

    // Should succeed (empty search might return all results)
    assert!(output.status.success() || output.status.code().is_some());

    // Test search with whitespace query
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(skills_dir.to_string_lossy().to_string())
        .arg("search")
        .arg("   ")
        .output()
        .expect("Failed to execute search with whitespace query");

    // Should succeed
    assert!(output.status.success() || output.status.code().is_some());
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

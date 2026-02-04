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
fn test_list_no_manifest_fails_with_instructions() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    // No skill-project.toml

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(
        result.stderr.contains("skill-project.toml not found")
            && result.stderr.contains("fastskill init"),
        "stderr should mention skill-project.toml and fastskill init: {}",
        result.stderr
    );
}

#[test]
fn test_list_default_grid_format() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    // Should show no skills (empty manifest)

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
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["list", "--json"], Some(temp_dir.path()));

    assert!(result.success);
    // Should output valid JSON (array of list rows, may be empty)
    assert!(result.stdout.contains("[]") || result.stdout.contains("\"id\""));

    assert_snapshot_with_settings("list_json_format", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_list_empty_skills_directory() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(
        result.stdout.contains("No skills") || result.stdout.contains("[INFO]"),
        "stdout: {}",
        result.stdout
    );

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

    let project_content = r#"[dependencies]
web-scraper = "1.2.3"
dev-tools = "2.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), project_content).unwrap();

    let result = run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(
        result.stdout.contains("missing")
            || result.stdout.contains("Missing")
            || result.stdout.contains("web-scraper"),
        "stdout: {}",
        result.stdout
    );

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

    let project_content = r#"[dependencies]
test-skill = "2.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"
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
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["list", "--json", "--grid"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(
        result.stderr.contains("error")
            || result.stderr.contains("Cannot use both")
            || result.stderr.contains("cannot be used with")
    );

    assert_snapshot_with_settings(
        "list_conflicting_flags",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

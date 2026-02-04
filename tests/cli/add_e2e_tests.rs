//! E2E tests for add command: compatibility warning only with --verbose

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn project_toml_with_skills_dir() -> &'static str {
    r#"[dependencies]

[tool.fastskill]
skills_directory = ".cursor/skills"
"#
}

/// Add from local folder without --verbose: "No compatibility field specified" must not appear.
#[test]
fn test_add_from_folder_no_verbose_hides_compatibility_warning() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".cursor/skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        project_toml_with_skills_dir(),
    )
    .unwrap();

    let fixture_skill =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli/fixtures/minimal-skill");
    let result = run_fastskill_command(
        &["add", fixture_skill.to_str().unwrap(), "--force"],
        Some(temp_dir.path()),
    );

    assert!(result.success, "add should succeed: {}", result.stderr);
    assert!(
        !result.stderr.contains("No compatibility")
            && !result.stderr.contains("compatibility field"),
        "stderr must not contain compatibility warning when not verbose: stderr={}",
        result.stderr
    );

    let output = format!("{}{}", result.stdout, result.stderr);
    assert_snapshot_with_settings(
        "add_from_folder_no_verbose",
        &output,
        &cli_snapshot_settings(),
    );
}

/// Add from local folder with --verbose: "No compatibility field specified" must appear.
#[test]
fn test_add_from_folder_verbose_shows_compatibility_warning() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".cursor/skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        project_toml_with_skills_dir(),
    )
    .unwrap();

    let fixture_skill =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli/fixtures/minimal-skill");
    // --verbose is a global flag, must come before subcommand
    let result = run_fastskill_command(
        &[
            "--verbose",
            "add",
            fixture_skill.to_str().unwrap(),
            "--force",
        ],
        Some(temp_dir.path()),
    );

    assert!(result.success, "add should succeed: {}", result.stderr);
    assert!(
        result.stderr.contains("No compatibility") || result.stderr.contains("compatibility"),
        "stderr must contain compatibility warning when verbose: stderr={}",
        result.stderr
    );

    let output = format!("{}{}", result.stdout, result.stderr);
    assert_snapshot_with_settings("add_from_folder_verbose", &output, &cli_snapshot_settings());
}

/// Helper function to create a minimal skill directory
fn create_minimal_skill(path: &Path, skill_id: &str, skill_name: &str) {
    // Ensure parent directories exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::create_dir_all(path).unwrap();

    // Create SKILL.md
    let skill_md = format!(
        r#"---
name: {}
description: Test skill for recursive add
version: 1.0.0
---
# {}
Test skill content.
"#,
        skill_name, skill_name
    );
    fs::write(path.join("SKILL.md"), skill_md).unwrap();

    // Create skill-project.toml
    let skill_toml = format!(
        r#"[metadata]
id = "{}"
skills_directory = ".skills"
"#,
        skill_id
    );
    fs::write(path.join("skill-project.toml"), skill_toml).unwrap();
}

/// Test recursive add with multiple skills
#[test]
fn test_add_recursive_multiple_skills() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".cursor/skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        project_toml_with_skills_dir(),
    )
    .unwrap();

    // Create a directory structure with multiple skills
    let skills_source = temp_dir.path().join("source-skills");
    create_minimal_skill(&skills_source.join("skill1"), "skill1", "skill-one");
    create_minimal_skill(&skills_source.join("skill2"), "skill2", "skill-two");
    create_minimal_skill(
        &skills_source.join("nested/skill3"),
        "skill3",
        "skill-three",
    );

    // Run add with --recursive
    let result = run_fastskill_command(
        &["add", skills_source.to_str().unwrap(), "-r", "--force"],
        Some(temp_dir.path()),
    );

    assert!(
        result.success,
        "recursive add should succeed: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains("Found 3 skill(s)"),
        "Should find 3 skills: {}",
        result.stdout
    );
    assert!(
        result
            .stdout
            .contains("Successfully added skill: skill-one"),
        "Should add skill1: {}",
        result.stdout
    );
    assert!(
        result
            .stdout
            .contains("Successfully added skill: skill-two"),
        "Should add skill2: {}",
        result.stdout
    );
    assert!(
        result
            .stdout
            .contains("Successfully added skill: skill-three"),
        "Should add skill3: {}",
        result.stdout
    );
}

/// Test recursive add skips hidden directories
#[test]
fn test_add_recursive_skips_hidden_dirs() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".cursor/skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        project_toml_with_skills_dir(),
    )
    .unwrap();

    // Create skills including one in a hidden directory
    let skills_source = temp_dir.path().join("source-skills");
    create_minimal_skill(&skills_source.join("skill1"), "skill1", "skill-one");
    create_minimal_skill(
        &skills_source.join(".hidden/skill2"),
        "skill2",
        "hidden-skill",
    );

    // Run add with --recursive
    let result = run_fastskill_command(
        &["add", skills_source.to_str().unwrap(), "-r", "--force"],
        Some(temp_dir.path()),
    );

    assert!(
        result.success,
        "recursive add should succeed: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains("Found 1 skill(s)"),
        "Should find only 1 skill (hidden dir skipped): {}",
        result.stdout
    );
    assert!(
        result
            .stdout
            .contains("Successfully added skill: skill-one"),
        "Should add skill1: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("hidden-skill"),
        "Should not add hidden skill: {}",
        result.stdout
    );
}

/// Test recursive add on empty directory returns error
#[test]
fn test_add_recursive_empty_directory() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".cursor/skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        project_toml_with_skills_dir(),
    )
    .unwrap();

    // Create empty directory
    let skills_source = temp_dir.path().join("empty-skills");
    fs::create_dir_all(&skills_source).unwrap();

    // Run add with --recursive on empty directory
    let result = run_fastskill_command(
        &["add", skills_source.to_str().unwrap(), "-r"],
        Some(temp_dir.path()),
    );

    assert!(!result.success, "recursive add on empty dir should fail");
    assert!(
        result.stderr.contains("No skill directories found"),
        "Should error about no skills found: {}",
        result.stderr
    );
}

/// Test recursive add with non-folder source returns error
#[test]
fn test_add_recursive_with_skill_id_fails() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".cursor/skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        project_toml_with_skills_dir(),
    )
    .unwrap();

    // Try to use --recursive with a skill ID (registry source)
    let result = run_fastskill_command(&["add", "scope/skill@1.0.0", "-r"], Some(temp_dir.path()));

    assert!(!result.success, "recursive add with skill ID should fail");
    assert!(
        result
            .stderr
            .contains("Recursive add is only valid when source is a local directory"),
        "Should error about recursive only for local directory: {}",
        result.stderr
    );
}

/// Test recursive add with git URL fails
#[test]
fn test_add_recursive_with_git_url_fails() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".cursor/skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        project_toml_with_skills_dir(),
    )
    .unwrap();

    // Try to use --recursive with a git URL
    let result = run_fastskill_command(
        &["add", "https://github.com/user/repo.git", "-r"],
        Some(temp_dir.path()),
    );

    assert!(!result.success, "recursive add with git URL should fail");
    assert!(
        result
            .stderr
            .contains("Recursive add is only valid when source is a local directory"),
        "Should error about recursive only for local directory: {}",
        result.stderr
    );
}

//! E2E tests for install command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_install_from_project_toml() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Copy sample skill-project.toml
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();

    let result = run_fastskill_command(&["install"], Some(temp_dir.path()));

    // Test skills will fail to install since they don't exist
    assert!(!result.success);
    assert!(result.stdout.contains("Installing"));

    assert_snapshot_with_settings(
        "install_from_project",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_install_with_lock_file() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create both project and lock files
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    let sample_lock = include_str!("fixtures/sample-skills-lock.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();
    fs::write(temp_dir.path().join("skills.lock"), sample_lock).unwrap();

    let result = run_fastskill_command(&["install", "--lock"], Some(temp_dir.path()));

    // Using lock file - succeeds even if no skills to install (filtered by groups)
    assert!(result.success);
    assert!(
        result.stdout.contains("lock")
            || result.stdout.contains("Installing")
            || result.stdout.contains("No skills")
    );

    assert_snapshot_with_settings(
        "install_with_lock",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_install_without_dev_dependencies() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Copy sample skill-project.toml
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();

    let result = run_fastskill_command(&["install", "--without", "dev"], Some(temp_dir.path()));

    // Test skills will fail to install since they don't exist
    assert!(!result.success);
    assert!(result.stdout.contains("main") || result.stdout.contains("Installing"));

    assert_snapshot_with_settings(
        "install_without_dev",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_install_only_group() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Copy sample skill-project.toml
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();

    let result = run_fastskill_command(&["install", "--only", "main"], Some(temp_dir.path()));

    // Test skills will fail to install since they don't exist
    assert!(!result.success);
    assert!(result.stdout.contains("main") || result.stdout.contains("Installing"));

    assert_snapshot_with_settings(
        "install_only_group",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_install_missing_project_file_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["install"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("not found"));

    assert_snapshot_with_settings(
        "install_missing_project",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_install_invalid_skill_id_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create project with invalid skill ID
    let project_content = r#"[project]
name = "test-project"
version = "1.0.0"

[dependencies]
"invalid skill id!" = "1.0.0"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), project_content).unwrap();

    let result = run_fastskill_command(&["install"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("Invalid"));

    assert_snapshot_with_settings(
        "install_invalid_skill_id",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

//! E2E tests for update command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_update_all_skills() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create sample project and lock files
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    let sample_lock = include_str!("fixtures/sample-skills-lock.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();
    fs::write(temp_dir.path().join("skills.lock"), sample_lock).unwrap();

    let result = run_fastskill_command(&["update"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(
        result.stdout.contains("Checking")
            || result.stdout.contains("Updated")
            || result.stdout.contains("No updates")
    );

    assert_snapshot_with_settings(
        "update_all_skills",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_update_specific_skill() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create sample project and lock files
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    let sample_lock = include_str!("fixtures/sample-skills-lock.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();
    fs::write(temp_dir.path().join("skills.lock"), sample_lock).unwrap();

    let result = run_fastskill_command(
        &["update", "--skill-id", "web-scraper"],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("web-scraper") || result.stdout.contains("No updates"));

    assert_snapshot_with_settings(
        "update_specific_skill",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_update_check_only() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create sample project and lock files
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    let sample_lock = include_str!("fixtures/sample-skills-lock.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();
    fs::write(temp_dir.path().join("skills.lock"), sample_lock).unwrap();

    let result = run_fastskill_command(&["update", "--check"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("check") || result.stdout.contains("available"));

    assert_snapshot_with_settings(
        "update_check_only",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_update_dry_run() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create sample project and lock files
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    let sample_lock = include_str!("fixtures/sample-skills-lock.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();
    fs::write(temp_dir.path().join("skills.lock"), sample_lock).unwrap();

    let result = run_fastskill_command(&["update", "--dry-run"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(
        result.stdout.contains("dry")
            || result.stdout.contains("would")
            || result.stdout.contains("No updates")
    );

    assert_snapshot_with_settings("update_dry_run", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_update_strategy_patch() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create sample project and lock files
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    let sample_lock = include_str!("fixtures/sample-skills-lock.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();
    fs::write(temp_dir.path().join("skills.lock"), sample_lock).unwrap();

    let result = run_fastskill_command(&["update", "--strategy", "patch"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("patch") || result.stdout.contains("No updates"));

    assert_snapshot_with_settings(
        "update_strategy_patch",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_update_strategy_minor() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create sample project and lock files
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    let sample_lock = include_str!("fixtures/sample-skills-lock.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();
    fs::write(temp_dir.path().join("skills.lock"), sample_lock).unwrap();

    let result = run_fastskill_command(&["update", "--strategy", "minor"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("minor") || result.stdout.contains("No updates"));

    assert_snapshot_with_settings(
        "update_strategy_minor",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_update_missing_lock_file_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create only project file, no lock file
    let sample_project = include_str!("fixtures/sample-skill-project.toml");
    fs::write(temp_dir.path().join("skill-project.toml"), sample_project).unwrap();

    let result = run_fastskill_command(&["update"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("not found"));

    assert_snapshot_with_settings(
        "update_missing_lock",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

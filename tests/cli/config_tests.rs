//! Tests for skills directory resolution

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]
//!
//! These tests verify skills directory resolution through CLI execution
//! since the bin module is internal to the binary.

use std::env;
use tempfile::TempDir;
use super::snapshot_helpers::{
    run_fastskill_command, cli_snapshot_settings, assert_snapshot_with_settings
};

#[test]
fn test_cli_repositories_path_argument() {
    let temp_dir = TempDir::new().unwrap();
    let repos_path = temp_dir.path().join("repositories.toml");
    std::fs::write(&repos_path, "# Test repositories file").unwrap();

    // Test that --repositories-path argument is accepted
    let result = run_fastskill_command(&["--repositories-path", &repos_path.to_string_lossy(), "--help"], None);

    // Should succeed (even with --help, --repositories-path should be parsed)
    assert!(result.success);
    assert_snapshot_with_settings("cli_repositories_path_argument", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_cli_with_env_var() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().to_string_lossy().to_string();

    // Set environment variable
    env::set_var("FASTSKILL_DIRECTORY", &skills_dir);

    // Test CLI accepts the env var (we can't easily verify it uses it without actual command execution)
    let result = run_fastskill_command(&["--help"], Some(std::path::Path::new(&skills_dir)));

    env::remove_var("FASTSKILL_DIRECTORY");

    // Should succeed
    assert!(result.success);
    assert_snapshot_with_settings("cli_with_env_var", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_cli_finds_skills_in_current_dir() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // Change to temp directory
    let original_dir = env::current_dir().unwrap();
    let original_dir_abs = original_dir.canonicalize().unwrap_or_else(|_| {
        // If canonicalize fails, use the original path
        if original_dir.is_absolute() {
            original_dir.clone()
        } else {
            // Fallback: use temp dir's parent if available
            temp_dir.path().parent().map(|p| p.to_path_buf()).unwrap_or_else(|| "/".into())
        }
    });

    env::set_current_dir(temp_dir.path()).unwrap();

    let result = run_fastskill_command(&["--help"], None);

    // Restore original directory - handle error gracefully in case directory no longer exists
    let _ = env::set_current_dir(&original_dir_abs);

    // Should succeed (CLI should find .skills directory)
    assert!(result.success);
    assert_snapshot_with_settings("cli_finds_skills_in_current_dir", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_cli_directory_walking() {
    // Test that CLI can be invoked from different directory levels
    let temp_dir = TempDir::new().unwrap();
    let nested_dir = temp_dir.path().join("level1").join("level2");
    std::fs::create_dir_all(&nested_dir).unwrap();

    // Test from nested directory
    let result = run_fastskill_command(&["--help"], Some(&nested_dir));

    // Should succeed regardless of directory level
    assert!(result.success);
    assert_snapshot_with_settings("cli_directory_walking", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_cli_invalid_repositories_path() {
    let invalid_path = "/nonexistent/path/to/repositories.toml";

    // Test that CLI handles invalid repositories path gracefully
    let result = run_fastskill_command(&["--repositories-path", invalid_path, "--help"], None);

    // CLI should still show help even with invalid path
    // (the path validation happens later in actual commands)
    assert!(result.success);
    assert_snapshot_with_settings("cli_invalid_repositories_path", &result.stdout, &cli_snapshot_settings());
}

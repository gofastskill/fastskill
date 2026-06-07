//! E2E tests for the doctor command

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::run_fastskill_command;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_doctor_runs_successfully() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["doctor"], Some(temp_dir.path()));
    // Doctor should succeed (skills dir exists)
    assert!(result.success, "doctor failed: {}", result.stderr);
}

#[test]
fn test_doctor_json_flag() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["doctor", "--json"], Some(temp_dir.path()));
    assert!(result.success, "doctor --json failed: {}", result.stderr);
    // Output should be valid JSON with a checks array
    assert!(
        result.stdout.contains("\"checks\""),
        "Expected JSON output with 'checks' key, got: {}",
        result.stdout
    );
}

#[test]
fn test_doctor_help() {
    let result = run_fastskill_command(&["doctor", "--help"], None);
    assert!(result.success);
    assert!(result.stdout.contains("doctor"));
}

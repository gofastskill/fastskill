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

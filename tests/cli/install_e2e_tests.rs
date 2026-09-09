//! E2E tests for install command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_local_fixture(root: &Path) {
    for id in ["main-skill", "dev-skill"] {
        let source = root.join("sources").join(id);
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {id}\nversion: 1.0.0\ndescription: fixture\n---\nBody\n"),
        )
        .unwrap();
    }
    fs::create_dir_all(root.join(".skills")).unwrap();
    fs::write(
        root.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".skills\"\n\n[dependencies]\nmain-skill = { origin = { type = \"local\", path = \"sources/main-skill\" }, groups = [\"main\"] }\ndev-skill = { origin = { type = \"local\", path = \"sources/dev-skill\" }, groups = [\"dev\"] }\n",
    )
    .unwrap();
}

#[test]
fn test_install_from_project_toml() {
    let temp_dir = TempDir::new().unwrap();
    write_local_fixture(temp_dir.path());

    let result = run_fastskill_command(&["install"], Some(temp_dir.path()));

    assert!(result.success, "{}{}", result.stdout, result.stderr);
    assert!(result.stdout.contains("Installing"));
    assert!(temp_dir
        .path()
        .join(".skills/main-skill/SKILL.md")
        .is_file());
    assert!(temp_dir.path().join(".skills/dev-skill/SKILL.md").is_file());
    assert!(temp_dir.path().join("skills.lock").is_file());
}

#[test]
fn test_install_with_lock_file() {
    let temp_dir = TempDir::new().unwrap();
    write_local_fixture(temp_dir.path());
    let initial = run_fastskill_command(&["install"], Some(temp_dir.path()));
    assert!(initial.success, "{}{}", initial.stdout, initial.stderr);
    fs::remove_dir_all(temp_dir.path().join(".skills/main-skill")).unwrap();
    fs::remove_dir_all(temp_dir.path().join(".skills/dev-skill")).unwrap();

    let result = run_fastskill_command(&["install", "--lock"], Some(temp_dir.path()));

    assert!(result.success, "{}{}", result.stdout, result.stderr);
    assert!(temp_dir
        .path()
        .join(".skills/main-skill/SKILL.md")
        .is_file());
    assert!(temp_dir.path().join(".skills/dev-skill/SKILL.md").is_file());
}

#[test]
fn test_install_without_dev_dependencies() {
    let temp_dir = TempDir::new().unwrap();
    write_local_fixture(temp_dir.path());

    let result = run_fastskill_command(&["install", "--without", "dev"], Some(temp_dir.path()));

    assert!(result.success, "{}{}", result.stdout, result.stderr);
    assert!(temp_dir
        .path()
        .join(".skills/main-skill/SKILL.md")
        .is_file());
    assert!(!temp_dir.path().join(".skills/dev-skill").exists());
}

#[test]
fn test_install_only_group() {
    let temp_dir = TempDir::new().unwrap();
    write_local_fixture(temp_dir.path());

    let result = run_fastskill_command(&["install", "--only", "main"], Some(temp_dir.path()));

    assert!(result.success, "{}{}", result.stdout, result.stderr);
    assert!(temp_dir
        .path()
        .join(".skills/main-skill/SKILL.md")
        .is_file());
    assert!(!temp_dir.path().join(".skills/dev-skill").exists());
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

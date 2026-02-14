//! Integration tests for the analyze command

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper to create a fastskill binary command
fn fastskill_cmd() -> Command {
    Command::cargo_bin("fastskill").unwrap()
}

/// Helper to create a test skill directory structure
fn create_test_skill_dir() -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    temp_dir
}

/// Helper to create a SKILL.md file
fn create_skill_file(skills_dir: &std::path::Path, skill_id: &str, name: &str, description: &str) {
    let skill_dir = skills_dir.join(skill_id);
    fs::create_dir_all(&skill_dir).unwrap();

    let content = format!(
        r#"# Skill

Name: {}
Version: 1.0.0
Description: {}

## Usage

Example skill for testing.
"#,
        name, description
    );

    fs::write(skill_dir.join("SKILL.md"), content).unwrap();
}

#[test]
fn test_analyze_help() {
    fastskill_cmd()
        .arg("analyze")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Diagnostic and analysis"))
        .stdout(predicate::str::contains("matrix"))
        .stdout(predicate::str::contains("cluster"))
        .stdout(predicate::str::contains("duplicates"));
}

#[test]
fn test_matrix_help() {
    fastskill_cmd()
        .arg("analyze")
        .arg("matrix")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("pairwise similarity"))
        .stdout(predicate::str::contains("--threshold"))
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--full"))
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_matrix_no_index() {
    let temp_dir = create_test_skill_dir();

    fastskill_cmd()
        .arg("analyze")
        .arg("matrix")
        .env(
            "FASTSKILL_SKILLS_DIR",
            temp_dir.path().join(".claude/skills"),
        )
        .assert()
        .success()
        .stdout(
            predicate::str::contains("No skills indexed")
                .or(predicate::str::contains("Run 'fastskill reindex' first")),
        );
}

#[test]
fn test_matrix_invalid_threshold() {
    fastskill_cmd()
        .arg("analyze")
        .arg("matrix")
        .arg("--threshold")
        .arg("1.5") // Invalid: > 1.0
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "threshold must be between 0.0 and 1.0",
        ));
}

#[test]
fn test_matrix_negative_threshold() {
    fastskill_cmd()
        .arg("analyze")
        .arg("matrix")
        .arg("--threshold")
        .arg("-0.1") // Invalid: < 0.0
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "threshold must be between 0.0 and 1.0",
        ));
}

#[test]
fn test_cluster_not_implemented() {
    fastskill_cmd()
        .arg("analyze")
        .arg("cluster")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_duplicates_not_implemented() {
    fastskill_cmd()
        .arg("analyze")
        .arg("duplicates")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

// NOTE: Full end-to-end tests with actual embeddings would require:
// 1. OpenAI API key
// 2. Creating and indexing skills
// 3. Mocking the embedding API
//
// These tests can be added later as part of the comprehensive test suite.
// For now, we test the CLI interface and error handling.

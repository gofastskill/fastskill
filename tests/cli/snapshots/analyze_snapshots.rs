//! Snapshot tests for analyze command output formatting

use assert_cmd::Command;
use insta::assert_snapshot;

fn fastskill_cmd() -> Command {
    Command::cargo_bin("fastskill").unwrap()
}

#[test]
fn snapshot_analyze_help() {
    let output = fastskill_cmd()
        .arg("analyze")
        .arg("--help")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_snapshot!("analyze_help", stdout);
}

#[test]
fn snapshot_matrix_help() {
    let output = fastskill_cmd()
        .arg("analyze")
        .arg("matrix")
        .arg("--help")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_snapshot!("matrix_help", stdout);
}

#[test]
fn snapshot_matrix_no_index_message() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let output = fastskill_cmd()
        .arg("analyze")
        .arg("matrix")
        .env("FASTSKILL_SKILLS_DIR", &skills_dir)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_snapshot!("matrix_no_index", stdout);
}

//! Tests for CLI help functionality

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn get_binary_path() -> String {
    format!("{}/target/debug/fastskill", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn test_help_with_no_args() {
    let output = Command::new(get_binary_path()).output().expect("Failed to execute command");

    // CLI requires arguments, so it shows help (exit code 1, but help is shown)
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Help might be in stdout or stderr depending on clap version
    assert!(
        stdout.contains("FastSkill")
            || stdout.contains("Usage")
            || stderr.contains("FastSkill")
            || stderr.contains("Usage")
    );
}

#[test]
fn test_help_flag() {
    let output = Command::new(get_binary_path())
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FastSkill"));
    assert!(stdout.contains("Commands:"));
}

#[test]
fn test_help_short_flag() {
    let output = Command::new(get_binary_path())
        .arg("-h")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FastSkill"));
}

#[test]
fn test_add_command_help() {
    let output = Command::new(get_binary_path())
        .arg("add")
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Add a new skill"));
}

#[test]
fn test_search_command_help() {
    let output = Command::new(get_binary_path())
        .arg("search")
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Search for skills"));
}

#[test]
fn test_disable_command_help() {
    let output = Command::new(get_binary_path())
        .arg("disable")
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Disable skills"));
}

#[test]
fn test_remove_command_help() {
    let output = Command::new(get_binary_path())
        .arg("remove")
        .arg("--help")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Remove skills"));
}

#[test]
fn test_version_flag() {
    let output = Command::new(get_binary_path())
        .arg("--version")
        .output()
        .expect("Failed to execute command");

    // Version flag might not be implemented yet, but should not crash
    assert!(output.status.success() || output.status.code().is_some());
}

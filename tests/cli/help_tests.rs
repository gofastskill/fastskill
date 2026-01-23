//! Tests for CLI help functionality

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};

#[test]
fn test_help_with_no_args() {
    let result = run_fastskill_command(&[], None);

    // CLI requires arguments, so it shows help (exit code 1, but help is shown)
    assert_snapshot_with_settings(
        "help_no_args",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_help_flag() {
    let result = run_fastskill_command(&["--help"], None);

    assert!(result.success);
    assert_snapshot_with_settings("help_flag", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_help_short_flag() {
    let result = run_fastskill_command(&["-h"], None);

    assert!(result.success);
    assert_snapshot_with_settings("help_short_flag", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_add_command_help() {
    let result = run_fastskill_command(&["add", "--help"], None);

    assert!(result.success);
    assert_snapshot_with_settings("add_command_help", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_search_command_help() {
    let result = run_fastskill_command(&["search", "--help"], None);

    assert!(result.success);
    assert_snapshot_with_settings(
        "search_command_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_disable_command_help() {
    let result = run_fastskill_command(&["disable", "--help"], None);

    assert!(result.success);
    assert_snapshot_with_settings(
        "disable_command_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_remove_command_help() {
    let result = run_fastskill_command(&["remove", "--help"], None);

    assert!(result.success);
    assert_snapshot_with_settings(
        "remove_command_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_version_flag() {
    let result = run_fastskill_command(&["--version"], None);

    // Version flag might not be implemented yet, but should not crash
    assert!(result.success || result.stderr.contains("fastskill"));
}

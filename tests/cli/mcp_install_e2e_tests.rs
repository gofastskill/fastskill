//! E2E tests for mcp install, mcp register, and mcp list subcommands.
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::snapshot_helpers::run_fastskill_command;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_mcp_help_lists_install_register_list_serve() {
    let result = run_fastskill_command(&["mcp", "--help"], None);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(combined.contains("install"), "mcp --help should list 'install'");
    assert!(combined.contains("register"), "mcp --help should list 'register'");
    assert!(combined.contains("list"), "mcp --help should list 'list'");
    assert!(combined.contains("serve"), "mcp --help should list 'serve'");
}

#[test]
fn test_mcp_install_dry_run_exits_zero_and_prints_output() {
    let result = run_fastskill_command(
        &["mcp", "install", "--agent", "cursor", "--stdio", "--dry-run"],
        None,
    );
    assert!(result.success, "dry-run should exit 0; stderr: {}", result.stderr);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(!combined.is_empty(), "dry-run should produce non-empty output");
}

#[test]
fn test_mcp_install_cursor_writes_config_file() {
    let temp_dir = TempDir::new().unwrap();
    let result = run_fastskill_command(
        &[
            "mcp", "install",
            "--agent", "cursor",
            "--stdio",
            "--scope", "project",
            "--overwrite",
        ],
        Some(temp_dir.path()),
    );
    assert!(result.success, "mcp install should exit 0; stderr: {}", result.stderr);
    let config_path = temp_dir.path().join(".cursor").join("mcp.json");
    assert!(config_path.exists(), ".cursor/mcp.json should be created");
    let contents = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(json.get("fastskill").is_some(), "mcp.json should contain 'fastskill' key");
    let args = &json["fastskill"]["args"];
    let args_str = args.to_string();
    assert!(
        args_str.contains("mcp") && args_str.contains("serve") && args_str.contains("stdio"),
        "args should include mcp serve --transport stdio; got: {args_str}"
    );
}

#[test]
fn test_mcp_install_duplicate_without_overwrite_exits_nonzero() {
    let temp_dir = TempDir::new().unwrap();
    // First install
    let first = run_fastskill_command(
        &[
            "mcp", "install",
            "--agent", "cursor",
            "--stdio",
            "--scope", "project",
            "--overwrite",
        ],
        Some(temp_dir.path()),
    );
    assert!(first.success, "first install should succeed; stderr: {}", first.stderr);

    // Second install without --overwrite should fail
    let second = run_fastskill_command(
        &["mcp", "install", "--agent", "cursor", "--stdio", "--scope", "project"],
        Some(temp_dir.path()),
    );
    assert!(!second.success, "second install without --overwrite should exit non-zero");
    let combined = format!("{}{}", second.stdout, second.stderr);
    assert!(
        combined.to_lowercase().contains("already") || combined.to_lowercase().contains("exist"),
        "error should mention entry already exists; got: {combined}"
    );
}

#[test]
fn test_mcp_list_exits_zero() {
    let result = run_fastskill_command(&["mcp", "list"], None);
    assert!(result.success, "mcp list should exit 0; stderr: {}", result.stderr);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(!combined.is_empty(), "mcp list should produce output");
}

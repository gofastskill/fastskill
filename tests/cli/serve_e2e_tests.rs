//! E2E tests for serve command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use std::net::TcpStream;
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

fn wait_for_port(port: u16, timeout_secs: u64) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < timeout_secs {
        if TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn test_serve_invalid_port_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let result = run_fastskill_command(&["serve", "--port", "99999"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("Invalid"));

    assert_snapshot_with_settings(
        "serve_invalid_port",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_serve_default_host_port() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Spawn server in background
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--port", "18080"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start server");

    // Wait for server to start
    assert!(
        wait_for_port(18080, 5),
        "Server failed to start on port 18080"
    );

    // Kill the server
    child.kill().expect("Failed to kill server");

    assert_snapshot_with_settings(
        "serve_default_host_port",
        "Server started successfully",
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_serve_custom_port() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Spawn server in background
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--port", "18081"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start server");

    // Wait for server to start
    assert!(
        wait_for_port(18081, 5),
        "Server failed to start on port 18081"
    );

    // Kill the server
    child.kill().expect("Failed to kill server");

    assert_snapshot_with_settings(
        "serve_custom_port",
        "Server started successfully",
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_serve_custom_host() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Spawn server in background
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--host", "127.0.0.1", "--port", "18082"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start server");

    // Wait for server to start
    assert!(
        wait_for_port(18082, 5),
        "Server failed to start on port 18082"
    );

    // Kill the server
    child.kill().expect("Failed to kill server");

    assert_snapshot_with_settings(
        "serve_custom_host",
        "Server started successfully",
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_serve_with_enable_registry() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Spawn server in background
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--enable-registry", "--port", "18083"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start server");

    // Wait for server to start
    assert!(
        wait_for_port(18083, 5),
        "Server failed to start on port 18083"
    );

    // Kill the server
    child.kill().expect("Failed to kill server");

    assert_snapshot_with_settings(
        "serve_enable_registry",
        "Server started successfully",
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_serve_port_already_in_use_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Start first server
    let mut child1 = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--port", "18084"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start first server");

    // Wait for first server to start
    assert!(wait_for_port(18084, 5), "First server failed to start");

    // Try to start second server on same port
    let result = run_fastskill_command(&["serve", "--port", "18084"], Some(temp_dir.path()));

    // Kill first server
    child1.kill().expect("Failed to kill first server");

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("Address already in use"));

    assert_snapshot_with_settings(
        "serve_port_in_use",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

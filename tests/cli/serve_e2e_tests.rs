//! E2E tests for serve command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use std::io::ErrorKind;
use std::net::TcpListener;
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

fn can_bind_localhost_or_skip() -> bool {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            drop(listener);
            true
        }
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            eprintln!("Skipping test: unable to bind localhost socket ({err})");
            false
        }
        Err(err) => panic!("failed to bind localhost socket for test setup: {err}"),
    }
}

const PROJECT_TOML: &str = "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".skills\"\n";

#[test]
fn test_serve_invalid_port_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp_dir.path().join("skill-project.toml"), PROJECT_TOML).unwrap();

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
    if !can_bind_localhost_or_skip() {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp_dir.path().join("skill-project.toml"), PROJECT_TOML).unwrap();

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
    if !can_bind_localhost_or_skip() {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp_dir.path().join("skill-project.toml"), PROJECT_TOML).unwrap();

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
    if !can_bind_localhost_or_skip() {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp_dir.path().join("skill-project.toml"), PROJECT_TOML).unwrap();

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
fn test_serve_starts_without_registry_config() {
    if !can_bind_localhost_or_skip() {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp_dir.path().join("skill-project.toml"), PROJECT_TOML).unwrap();

    // serve no longer has --enable-registry; server starts and UI/API are always available
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--port", "18083"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start server");

    assert!(wait_for_port(18083, 5), "Server should start on port 18083");
    child.kill().expect("Failed to kill server");
}

#[test]
fn test_serve_health_endpoints() {
    if !can_bind_localhost_or_skip() {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp_dir.path().join("skill-project.toml"), PROJECT_TOML).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--port", "18085"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start server");

    assert!(
        wait_for_port(18085, 5),
        "Server failed to start on port 18085"
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = reqwest::Client::new();

    // Test /healthz liveness probe returns HTTP 200
    let health_status = rt.block_on(async {
        client
            .get("http://127.0.0.1:18085/healthz")
            .send()
            .await
            .expect("GET /healthz")
            .status()
    });
    assert_eq!(health_status, reqwest::StatusCode::OK, "/healthz should return 200");

    // Test /readyz readiness probe returns HTTP 200
    let ready_status = rt.block_on(async {
        client
            .get("http://127.0.0.1:18085/readyz")
            .send()
            .await
            .expect("GET /readyz")
            .status()
    });
    assert_eq!(ready_status, reqwest::StatusCode::OK, "/readyz should return 200");

    // Test /api/v1/skills returns HTTP 200
    let skills_status = rt.block_on(async {
        client
            .get("http://127.0.0.1:18085/api/v1/skills")
            .send()
            .await
            .expect("GET /api/v1/skills")
            .status()
    });
    assert_eq!(skills_status, reqwest::StatusCode::OK, "/api/v1/skills should return 200");

    // Test 308 redirect for unversioned /api/skills
    let no_redirect_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build no-redirect client");
    let redirect_status = rt.block_on(async {
        no_redirect_client
            .get("http://127.0.0.1:18085/api/skills")
            .send()
            .await
            .expect("GET /api/skills unversioned")
            .status()
    });
    assert_eq!(
        redirect_status.as_u16(),
        308,
        "Unversioned /api/skills should redirect 308 to /api/v1/skills"
    );

    child.kill().expect("Failed to kill server");

    assert_snapshot_with_settings(
        "serve_health_endpoints",
        "Health endpoints verified",
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_serve_starts_without_skill_project_toml() {
    if !can_bind_localhost_or_skip() {
        return;
    }

    // Create a temp dir with NO skill-project.toml
    let temp_dir = TempDir::new().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&["serve", "--port", "18086"])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("Failed to start server");

    assert!(
        wait_for_port(18086, 5),
        "Server failed to start on port 18086 without skill-project.toml"
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = reqwest::Client::new();

    let health_status = rt.block_on(async {
        client
            .get("http://127.0.0.1:18086/healthz")
            .send()
            .await
            .expect("GET /healthz")
            .status()
    });
    assert_eq!(health_status, reqwest::StatusCode::OK, "/healthz should return 200 without skill-project.toml");

    let ready_status = rt.block_on(async {
        client
            .get("http://127.0.0.1:18086/readyz")
            .send()
            .await
            .expect("GET /readyz")
            .status()
    });
    assert_eq!(ready_status, reqwest::StatusCode::OK, "/readyz should return 200 without skill-project.toml");

    child.kill().expect("Failed to kill server");
}

#[test]
fn test_serve_port_already_in_use_error() {
    if !can_bind_localhost_or_skip() {
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(temp_dir.path().join("skill-project.toml"), PROJECT_TOML).unwrap();

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

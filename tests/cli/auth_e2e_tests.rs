//! E2E tests for auth command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command_with_env,
};
use serde_json::json;
use std::fs;
use tempfile::TempDir;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn test_auth_login_success() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    // Mock server with /auth/token endpoint
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "token": "jwt-token-here"
            }
        })))
        .mount(&mock_server)
        .await;

    // Set config directory environment variable
    let env_vars = vec![
        ("OPENAI_API_KEY", "test-key"),
        ("FASTSKILL_CONFIG_DIR", config_dir.to_str().unwrap()),
    ];

    let result = run_fastskill_command_with_env(
        &["auth", "login", "--registry", &mock_server.uri()],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Successfully logged in") || result.stdout.contains("ok"));

    assert_snapshot_with_settings(
        "auth_login_success",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[tokio::test]
async fn test_auth_login_with_custom_role() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    // Mock server with /auth/token endpoint
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "token": "jwt-token-manager-role"
            }
        })))
        .mount(&mock_server)
        .await;

    // Set config directory environment variable
    let env_vars = vec![
        ("OPENAI_API_KEY", "test-key"),
        ("FASTSKILL_CONFIG_DIR", config_dir.to_str().unwrap()),
    ];

    let result = run_fastskill_command_with_env(
        &[
            "auth",
            "login",
            "--registry",
            &mock_server.uri(),
            "--role",
            "manager",
        ],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Successfully logged in") || result.stdout.contains("ok"));

    assert_snapshot_with_settings(
        "auth_login_custom_role",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[tokio::test]
async fn test_auth_login_unreachable_registry_error() {
    let temp_dir = TempDir::new().unwrap();

    // Try to login to unreachable registry
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];

    let result = run_fastskill_command_with_env(
        &["auth", "login", "--registry", "http://localhost:9999"],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("Failed to connect"));

    assert_snapshot_with_settings(
        "auth_login_unreachable",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[tokio::test]
async fn test_auth_login_invalid_credentials_error() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    // Mock server that returns 401 Unauthorized
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/token"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Invalid credentials"))
        .mount(&mock_server)
        .await;

    // Set config directory environment variable
    let env_vars = vec![
        ("OPENAI_API_KEY", "test-key"),
        ("FASTSKILL_CONFIG_DIR", config_dir.to_str().unwrap()),
    ];

    let result = run_fastskill_command_with_env(
        &["auth", "login", "--registry", &mock_server.uri()],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(
        result.stderr.contains("error")
            || result.stderr.contains("Unauthorized")
            || result.stderr.contains("Invalid")
    );

    assert_snapshot_with_settings(
        "auth_login_invalid_credentials",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_auth_logout_removes_token() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    // Create a config with token first
    let config_content = format!(
        r#"[registry."{}"]
token = "test-token-to-remove"
role = "manager"
"#,
        "http://test-registry"
    );
    fs::write(config_dir.join("config.toml"), config_content).unwrap();

    let env_vars = vec![("FASTSKILL_CONFIG_DIR", config_dir.to_str().unwrap())];

    let result = run_fastskill_command_with_env(
        &["auth", "logout", "--registry", "http://test-registry"],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(
        result.stdout.contains("logged out")
            || result.stdout.contains("Logout")
            || result.stdout.contains("removed")
            || result.stdout.contains("success")
    );

    assert_snapshot_with_settings(
        "auth_logout_success",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[tokio::test]
async fn test_auth_logout_nonexistent_registry() {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let env_vars = vec![("FASTSKILL_CONFIG_DIR", config_dir.to_str().unwrap())];

    let result = run_fastskill_command_with_env(
        &[
            "auth",
            "logout",
            "--registry",
            "http://nonexistent-registry",
        ],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    // Logout should be idempotent for non-existent registries

    assert_snapshot_with_settings(
        "auth_logout_nonexistent",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

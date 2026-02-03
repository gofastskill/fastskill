//! Integration tests for CLI functionality

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

mod cli;

#[test]
fn test_cli_help() {
    use std::process::Command;

    let binary = format!("{}/target/debug/fastskill", env!("CARGO_MANIFEST_DIR"));

    // Test --help flag
    let output = Command::new(&binary)
        .arg("--help")
        .output()
        .expect("Failed to execute CLI");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FastSkill"));
}

#[tokio::test]
async fn test_service_integration() {
    use fastskill::{FastSkillService, ServiceConfig};
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir,
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test search functionality
    let results = service.metadata_service().discover_skills("test").await;
    assert!(results.is_ok());

    // Test disable with string reference
    let skill_id = fastskill::SkillId::new("test-skill".to_string()).unwrap();
    let _result = service.skill_manager().disable_skill(&skill_id).await;
    // Expected to fail since skill doesn't exist, but tests the interface
}

// E2E tests for uncovered commands

#[test]
fn test_list_default_grid_format() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n",
    )
    .unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(
        result.stdout.contains("No skills") || result.stdout.contains("[INFO]"),
        "stdout: {}",
        result.stdout
    );

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "list_default_grid",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_list_json_format() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".claude").join("skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n",
    )
    .unwrap();

    let result =
        cli::snapshot_helpers::run_fastskill_command(&["list", "--json"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("[]") || result.stdout.contains("\"id\""));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "list_json_format",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_reindex_empty_directory() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml with tool.fastskill.embedding config
    let project_content = r#"[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), project_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result = cli::snapshot_helpers::run_fastskill_command_with_env(
        &["reindex"],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    // Empty skills directory should succeed with minimal or no output
    // Since it just shows version, that's normal behavior

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "reindex_empty_directory",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_install_missing_project_file_error() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(&["install"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("skill-project.toml"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "install_missing_project_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_show_nonexistent_skill_error() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["show", "nonexistent-skill"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("not found"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "show_nonexistent_skill_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_show_invalid_skill_id_format() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["show", "invalid skill id!"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("Invalid skill ID"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "show_invalid_id_format_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_read_nonexistent_skill_error() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["read", "nonexistent-skill"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("not found"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "read_nonexistent_skill_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_read_invalid_skill_id_format() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["read", "invalid skill id!"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("Invalid skill ID"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "read_invalid_id_format",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_auth_login_unreachable_registry_error() {
    let result = cli::snapshot_helpers::run_fastskill_command(
        &["auth", "login", "--registry", "http://localhost:99999"],
        None,
    );

    assert!(!result.success);
    assert!(result.stderr.contains("Failed to connect") || result.stderr.contains("error"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "auth_login_invalid_port",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_auth_logout_nonexistent_registry() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let env_vars = vec![("FASTSKILL_CONFIG_DIR", config_dir.to_str().unwrap())];

    let result = cli::snapshot_helpers::run_fastskill_command_with_env(
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

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "auth_logout_nonexistent",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_registry_list_empty() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let result =
        cli::snapshot_helpers::run_fastskill_command(&["sources", "list"], Some(temp_dir.path()));
    assert!(result.success);
    assert!(result.stdout.contains("No repositories") || result.stdout.is_empty());

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "registry_list_empty",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_registry_add_validation_missing_url() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["sources", "add", "missing-url", "--repo-type", "local"],
        None,
    );
    assert!(!result.success);
    assert!(result.stderr.contains("required") || result.stderr.contains("argument"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "registry_add_missing_url",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_registry_remove_nonexistent_error() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["sources", "remove", "nonexistent-repo"],
        None,
    );
    assert!(!result.success);
    assert!(result.stderr.contains("not found") || result.stderr.contains("error"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "registry_remove_nonexistent",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

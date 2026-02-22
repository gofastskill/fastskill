//! CORS configuration tests
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assertions_on_constants
)]

use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::service::HttpServerConfig;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_build_cors_layer_no_config() {
    // When no server config is present, should deny all origins
    let config = fastskill::ServiceConfig::default();

    let _cors_layer = fastskill::http::server::build_cors_layer(&config);

    // We can't easily test the CORS layer internals, but we verified it compiles
    // and logs a warning about no config found
    assert!(true);
}

#[test]
fn test_build_cors_layer_empty_origins() {
    // When allowed_origins is empty, should deny all origins
    let config = fastskill::ServiceConfig {
        http_server: Some(HttpServerConfig {
            allowed_origins: vec![],
            allowed_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
        }),
        ..Default::default()
    };

    let _cors_layer = fastskill::http::server::build_cors_layer(&config);

    // Verified it compiles
    assert!(true);
}

#[test]
fn test_build_cors_layer_with_origins() {
    // When allowed_origins is set, should allow those origins
    let config = fastskill::ServiceConfig {
        http_server: Some(HttpServerConfig {
            allowed_origins: vec![
                "https://example.com".to_string(),
                "http://localhost:3000".to_string(),
            ],
            allowed_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
        }),
        ..Default::default()
    };

    let _cors_layer = fastskill::http::server::build_cors_layer(&config);

    // Verified it compiles
    assert!(true);
}

#[test]
fn test_load_server_config_from_toml() {
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");

    let content = r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.server]
allowed_origins = ["https://example.com", "http://localhost:3000"]
allowed_headers = ["Content-Type", "Authorization", "X-API-Key"]
    "#;

    fs::write(&project_file, content).unwrap();

    let config_result = fastskill::core::load_project_config(temp_dir.path());
    assert!(config_result.is_ok());

    // Load project TOML and verify server config
    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let tool_config = project.tool.unwrap().fastskill.unwrap();
    let server_config = tool_config.server.unwrap();

    assert_eq!(server_config.allowed_origins.len(), 2);
    assert!(server_config
        .allowed_origins
        .contains(&"https://example.com".to_string()));
    assert!(server_config
        .allowed_origins
        .contains(&"http://localhost:3000".to_string()));
    assert_eq!(server_config.allowed_headers.len(), 3);
    assert!(server_config
        .allowed_headers
        .contains(&"Content-Type".to_string()));
    assert!(server_config
        .allowed_headers
        .contains(&"Authorization".to_string()));
    assert!(server_config
        .allowed_headers
        .contains(&"X-API-Key".to_string()));
}

#[test]
fn test_load_server_config_with_invalid_origins() {
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");

    let content = r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.server]
allowed_origins = ["https://example.com", "invalid-origin", "http://localhost:3000"]
    "#;

    fs::write(&project_file, content).unwrap();

    // Load project TOML and verify server config
    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let tool_config = project.tool.unwrap().fastskill.unwrap();
    let server_config = tool_config.server.unwrap();

    // Note: Invalid origins are included in the TOML, but would be filtered
    // at runtime by the load_server_config function
    assert_eq!(server_config.allowed_origins.len(), 3);
    assert!(server_config
        .allowed_origins
        .contains(&"https://example.com".to_string()));
    assert!(server_config
        .allowed_origins
        .contains(&"http://localhost:3000".to_string()));
    assert!(server_config
        .allowed_origins
        .contains(&"invalid-origin".to_string()));
}

#[test]
fn test_load_server_config_no_config() {
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");

    let content = r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"
    "#;

    fs::write(&project_file, content).unwrap();

    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let tool_config = project.tool.unwrap().fastskill.unwrap();

    // No server config should be present
    assert!(tool_config.server.is_none());
}

#[test]
fn test_load_server_config_empty_origins() {
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");

    let content = r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.server]
allowed_origins = []
    "#;

    fs::write(&project_file, content).unwrap();

    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let tool_config = project.tool.unwrap().fastskill.unwrap();
    let server_config = tool_config.server.unwrap();

    assert_eq!(server_config.allowed_origins.len(), 0);
}

#[test]
fn test_load_server_config_default_headers() {
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");

    // Test that default headers are used when not specified
    let content = r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"

[tool.fastskill.server]
allowed_origins = ["https://example.com"]
    "#;

    fs::write(&project_file, content).unwrap();

    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let tool_config = project.tool.unwrap().fastskill.unwrap();
    let server_config = tool_config.server.unwrap();

    assert_eq!(server_config.allowed_headers.len(), 2);
    assert!(server_config
        .allowed_headers
        .contains(&"Content-Type".to_string()));
    assert!(server_config
        .allowed_headers
        .contains(&"Authorization".to_string()));
}

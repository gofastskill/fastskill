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

//! Unit tests for the main service

use fastskill::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test_service_creation() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    assert!(!service.is_initialized());

    service.initialize().await.unwrap();
    assert!(service.is_initialized());
}

#[tokio::test]
async fn test_service_shutdown() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    service.shutdown().await.unwrap();
    assert!(!service.is_initialized());
}

#[tokio::test]
async fn test_service_configuration() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        execution: ExecutionConfig {
            default_timeout: std::time::Duration::from_secs(60),
            max_memory_mb: 1024,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut service = FastSkillService::new(config.clone()).await.unwrap();
    service.initialize().await.unwrap();

    assert_eq!(service.config().execution.default_timeout, std::time::Duration::from_secs(60));
    assert_eq!(service.config().execution.max_memory_mb, 1024);
}

#[tokio::test]
async fn test_skill_manager_access() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test that we can access the skill manager
    let skill_manager = service.skill_manager();
    assert!(skill_manager.list_skills(None).await.is_ok());
}

#[tokio::test]
async fn test_metadata_service_access() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test that we can access the metadata service
    let metadata_service = service.metadata_service();
    assert!(metadata_service.discover_skills("test query").await.is_ok());
}

#[tokio::test]
async fn test_loading_service_access() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test that we can access the loading service
    let loading_service = service.loading_service();
    assert!(loading_service.load_metadata(&[]).await.is_ok());
}

#[tokio::test]
async fn test_tool_calling_service_access() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test that we can access the tool calling service
    let tool_service = service.tool_service();
    assert!(tool_service.get_available_tools().await.is_ok());
}

#[tokio::test]
async fn test_routing_service_access() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test that we can access the routing service
    let routing_service = service.routing_service();
    assert!(routing_service.find_relevant_skills("test query", None).await.is_ok());
}

#[test]
fn test_version_constant() {
    assert!(!crate::VERSION.is_empty());
}

#[test]
fn test_init_logging() {
    // This should not panic
    crate::init_logging();
}

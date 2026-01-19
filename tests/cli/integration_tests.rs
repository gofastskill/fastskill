//! Integration tests for CLI commands

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::{FastSkillService, ServiceConfig};
use tempfile::TempDir;
use tokio;

#[tokio::test]
async fn test_add_command_from_folder() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // Create a test skill structure
    let skill_dir = skills_dir.join("test-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_md,
        r#"---
name: test-skill
description: A test skill
version: 1.0.0
---
# Test Skill
"#,
    )
    .unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test adding skill via CLI would be tested through actual CLI execution
    // For now, verify the skill structure is valid
    assert!(skill_dir.join("SKILL.md").exists());
    println!("Test skill created at: {:?}", skill_dir);
}

#[tokio::test]
async fn test_search_command() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test search through metadata service directly
    let results = service.metadata_service().discover_skills("test").await;
    assert!(results.is_ok());
    // Should return empty results when no skills exist
    assert_eq!(results.unwrap().len(), 0);
}

#[tokio::test]
async fn test_read_command_execution() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // Create a test skill
    let skill_dir = skills_dir.join("read-test");
    std::fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_md,
        r#"---
name: read-test
description: Test skill for read command
version: 1.0.0
---
# Read Test Skill
"#,
    )
    .unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test read through service
    let skill_id = fastskill::SkillId::new("read-test".to_string()).unwrap();
    let skill = service.skill_manager().get_skill(&skill_id).await.unwrap();
    assert!(skill.is_some());
}

#[tokio::test]
async fn test_disable_command() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test disable through service directly
    let skill_id = fastskill::SkillId::new("nonexistent".to_string()).unwrap();
    let result = service.skill_manager().disable_skill(&skill_id).await;
    // Should fail because skill doesn't exist
    assert!(result.is_err());
}

#[tokio::test]
async fn test_service_auto_indexing() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // Create multiple skill directories with SKILL.md files
    let skill_names = vec!["text-processor", "data-analyzer", "devops-tool"];

    for skill_name in &skill_names {
        let skill_dir = skills_dir.join(skill_name);
        std::fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        let content = format!(
            r#"---
name: {}
description: A test skill for {} functionality
version: 1.0.0
tags: [test, {}]
capabilities: [{}]
---
# {}

This is a test skill for auto-indexing.
"#,
            skill_name, skill_name, skill_name, skill_name, skill_name
        );

        std::fs::write(&skill_md, content).unwrap();
    }

    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Check that skills were auto-indexed
    let all_skills = service.skill_manager().list_skills(None).await.unwrap();
    assert_eq!(all_skills.len(), skill_names.len());

    // Check that skills can be found via search
    let search_results = service.metadata_service().discover_skills("test").await.unwrap();
    assert!(!search_results.is_empty());

    // Verify specific skills exist
    for skill_name in &skill_names {
        let skill_id = fastskill::SkillId::new(skill_name.to_string()).unwrap();
        let skill = service.skill_manager().get_skill(&skill_id).await.unwrap();
        assert!(skill.is_some());
        let skill = skill.unwrap();
        assert_eq!(skill.id, skill_id);
        // Additional assertions can be added here if needed for other SkillDefinition fields
    }
}

#[tokio::test]
async fn test_service_auto_indexing_nested_dirs() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // Create nested skill structure
    let nested_path = skills_dir.join("category").join("subcategory");
    std::fs::create_dir_all(&nested_path).unwrap();

    let skill_dir = nested_path.join("nested-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = skill_dir.join("SKILL.md");
    let content = r#"---
name: nested-skill
description: A skill in a nested directory
version: 2.0.0
tags: [nested, test]
capabilities: [nested_processing]
---
# Nested Skill

This skill is in a deeply nested directory.
"#;

    std::fs::write(&skill_md, content).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Check that the nested skill was found and indexed
    let all_skills = service.skill_manager().list_skills(None).await.unwrap();
    assert_eq!(all_skills.len(), 1);

    let skill_id = fastskill::SkillId::new("nested-skill".to_string()).unwrap();
    let skill = service.skill_manager().get_skill(&skill_id).await.unwrap();
    assert!(skill.is_some());
    let skill = skill.unwrap();
    assert_eq!(skill.id, skill_id);
    assert_eq!(skill.version, "2.0.0");
}

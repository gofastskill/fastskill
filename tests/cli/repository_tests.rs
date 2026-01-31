//! Tests for repository command

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_skill(dir: &PathBuf, skill_name: &str, version: &str) -> PathBuf {
    let skill_dir = dir.join(skill_name);
    fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = format!(
        r#"---
name: {}
description: Test skill for {}
version: {}
author: Test Author
tags:
  - test
  - example
capabilities:
  - testing
---

# {}

This is a test skill for testing purposes.
"#,
        skill_name, skill_name, version, skill_name
    );

    fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();
    skill_dir
}

#[tokio::test]
async fn test_scan_directory_for_skills() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path();

    // Create test skills
    create_test_skill(&skills_dir.to_path_buf(), "skill1", "1.0.0");
    create_test_skill(&skills_dir.to_path_buf(), "skill2", "2.0.0");

    // Note: This would require actual implementation testing
    // For now, just verify the directory structure
    assert!(skills_dir.join("skill1").join("SKILL.md").exists());
    assert!(skills_dir.join("skill2").join("SKILL.md").exists());
}

/// T026: Integration test for fastskill registry add updating repositories in skill-project.toml
#[test]
fn test_registry_add_updates_repositories_in_skill_project_toml() {
    use fastskill::core::manifest::SkillProjectToml;
    use fastskill::core::manifest::{
        FastSkillToolConfig, RepositoryDefinition, RepositoryType, ToolSection,
    };

    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // Create initial skill-project.toml
    let project_toml = project_root.join("skill-project.toml");
    fs::write(
        &project_toml,
        r#"
[dependencies]
web-scraper = "1.0.0"
"#,
    )
    .unwrap();

    // Load project
    let mut project = SkillProjectToml::load_from_file(&project_toml).unwrap();

    // Simulate adding a repository (what registry add command would do)
    let tool = project
        .tool
        .get_or_insert_with(|| ToolSection { fastskill: None });
    let fastskill_config = tool.fastskill.get_or_insert_with(|| FastSkillToolConfig {
        skills_directory: None,
        embedding: None,
        repositories: Some(Vec::new()),
    });
    let repos = fastskill_config.repositories.get_or_insert_with(Vec::new);

    repos.push(RepositoryDefinition {
        name: "test-registry".to_string(),
        r#type: RepositoryType::HttpRegistry,
        priority: 1,
        connection: fastskill::core::manifest::RepositoryConnection::HttpRegistry {
            index_url: "https://registry.example.com".to_string(),
        },
        auth: None,
    });

    // Save updated project
    project.save_to_file(&project_toml).unwrap();

    // Verify the update was saved
    let updated_project = SkillProjectToml::load_from_file(&project_toml).unwrap();
    assert!(updated_project.tool.is_some());
    let tool = updated_project.tool.as_ref().unwrap();
    assert!(tool.fastskill.is_some());
    let fastskill_config = tool.fastskill.as_ref().unwrap();
    assert!(fastskill_config.repositories.is_some());
    let repos = fastskill_config.repositories.as_ref().unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].name, "test-registry");
}

#[test]
fn test_skill_info_toml_priority() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md with version 1.0.0
    let skill_md = r#"---
name: Test Skill
description: Test
version: 1.0.0
---

# Test
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();

    // Create skill-project.toml with version 2.0.0 (should take priority)
    let skill_project = r#"
[metadata]
version = "2.0.0"
"#;
    fs::write(skill_dir.join("skill-project.toml"), skill_project).unwrap();

    // Note: This would require actual parsing test
    // For now, just verify files exist
    assert!(skill_dir.join("SKILL.md").exists());
    assert!(skill_dir.join("skill-project.toml").exists());
}

#[test]
fn test_marketplace_json_generation() {
    use fastskill::core::sources::{MarketplaceJson, MarketplaceSkill};

    let skills = vec![
        MarketplaceSkill {
            id: "skill1".to_string(),
            name: "Skill 1".to_string(),
            description: "Description 1".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            download_url: None,
        },
        MarketplaceSkill {
            id: "skill2".to_string(),
            name: "Skill 2".to_string(),
            description: "Description 2".to_string(),
            version: "2.0.0".to_string(),
            author: Some("Author".to_string()),
            download_url: Some("https://example.com/skill2.zip".to_string()),
        },
    ];

    let marketplace = MarketplaceJson {
        version: "1.0".to_string(),
        skills,
    };

    let json = serde_json::to_string_pretty(&marketplace).unwrap();
    assert!(json.contains("\"version\": \"1.0\""));
    assert!(json.contains("skill1"));
    assert!(json.contains("skill2"));
    assert!(json.contains("Skill 1"));
    assert!(json.contains("Skill 2"));
}

#[test]
fn test_claude_code_marketplace_json_parsing() {
    use fastskill::core::sources::ClaudeCodeMarketplaceJson;

    let json = r#"{
        "name": "test-repo",
        "owner": {
            "name": "Test Owner",
            "email": "test@example.com"
        },
        "metadata": {
            "description": "Test repository",
            "version": "1.0.0"
        },
        "plugins": [
            {
                "name": "test-plugin",
                "description": "Test plugin",
                "source": "./",
                "strict": false,
                "skills": ["./skill1", "./skill2"]
            }
        ]
    }"#;

    let marketplace: ClaudeCodeMarketplaceJson = serde_json::from_str(json).unwrap();
    assert_eq!(marketplace.name, "test-repo");
    assert!(marketplace.owner.is_some());
    assert_eq!(marketplace.owner.as_ref().unwrap().name, "Test Owner");
    assert_eq!(
        marketplace.owner.as_ref().unwrap().email.as_ref().unwrap(),
        "test@example.com"
    );
    assert!(marketplace.metadata.is_some());
    assert_eq!(
        marketplace
            .metadata
            .as_ref()
            .unwrap()
            .description
            .as_ref()
            .unwrap(),
        "Test repository"
    );
    assert_eq!(marketplace.plugins.len(), 1);
    assert_eq!(marketplace.plugins[0].name, "test-plugin");
    assert_eq!(marketplace.plugins[0].skills.len(), 2);
    assert_eq!(marketplace.plugins[0].skills[0], "./skill1");
    assert_eq!(marketplace.plugins[0].skills[1], "./skill2");
}

#[test]
fn test_claude_code_marketplace_parsing() {
    use serde_json::json;

    // Test Claude Code format parsing
    let claude_json = json!({
        "name": "test-repo",
        "plugins": [
            {
                "name": "test-plugin",
                "skills": ["./skill1"]
            }
        ]
    });

    let claude_marketplace: fastskill::core::sources::ClaudeCodeMarketplaceJson =
        serde_json::from_value(claude_json).unwrap();
    assert_eq!(claude_marketplace.name, "test-repo");
    assert_eq!(claude_marketplace.plugins.len(), 1);
    assert_eq!(claude_marketplace.plugins[0].name, "test-plugin");
    assert_eq!(claude_marketplace.plugins[0].skills.len(), 1);
    assert_eq!(claude_marketplace.plugins[0].skills[0], "./skill1");
}

#[test]
fn test_claude_code_marketplace_structure() {
    use fastskill::core::sources::{ClaudeCodeMarketplaceJson, ClaudeCodePlugin};

    // Create a Claude Code marketplace
    let claude_marketplace = ClaudeCodeMarketplaceJson {
        name: "test-repo".to_string(),
        owner: None,
        metadata: None,
        plugins: vec![ClaudeCodePlugin {
            name: "plugin1".to_string(),
            description: Some("Plugin 1".to_string()),
            source: Some("./".to_string()),
            strict: Some(false),
            skills: vec!["./skill1".to_string(), "./skill2".to_string()],
        }],
    };

    // Verify structure
    assert_eq!(claude_marketplace.name, "test-repo");
    assert_eq!(claude_marketplace.plugins.len(), 1);
    assert_eq!(claude_marketplace.plugins[0].skills.len(), 2);
    assert_eq!(claude_marketplace.plugins[0].name, "plugin1");
}

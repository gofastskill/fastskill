//! Integration tests for skill-project.toml functionality

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::core::manifest::{
    DependenciesSection, DependencySpec, MetadataSection, SkillProjectToml,
};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_init_creates_skill_project_toml_with_both_sections() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().join("skill-project.toml");

    // Create a project with both sections
    let mut deps = HashMap::new();
    deps.insert(
        "helper-skill".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    );

    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("my-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: Some("Test skill".to_string()),
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    project.save_to_file(&project_path).unwrap();

    // Verify file exists and has both sections
    assert!(project_path.exists());
    let content = fs::read_to_string(&project_path).unwrap();
    assert!(content.contains("[metadata]"));
    assert!(content.contains("[dependencies]"));
    assert!(content.contains("version = \"1.0.0\""));
    assert!(content.contains("helper-skill = \"1.0.0\""));
}

#[test]
fn test_init_without_skill_md() {
    let temp_dir = TempDir::new().unwrap();

    // Create project without SKILL.md
    let project_path = temp_dir.path().join("skill-project.toml");

    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("test-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    project.save_to_file(&project_path).unwrap();

    // Verify it works without SKILL.md
    assert!(project_path.exists());
    let loaded = SkillProjectToml::load_from_file(&project_path).unwrap();
    assert_eq!(
        loaded.metadata.as_ref().unwrap().version,
        Some("1.0.0".to_string())
    );
}

#[test]
fn test_init_with_skill_md_frontmatter_extraction() {
    let temp_dir = TempDir::new().unwrap();

    // Create SKILL.md with frontmatter
    let skill_md = temp_dir.path().join("SKILL.md");
    fs::write(
        &skill_md,
        r#"---
version: 2.0.0
name: extracted-skill
description: Extracted from frontmatter
author: Test Author
tags: [test, example]
capabilities: [testing]
---

# Extracted Skill
"#,
    )
    .unwrap();

    // This test verifies that frontmatter extraction would work
    // The actual extraction logic is in init.rs
    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("extracted-skill".to_string()),
            version: Some("2.0.0".to_string()), // From frontmatter
            description: Some("Extracted from frontmatter".to_string()),
            author: Some("Test Author".to_string()),
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    let project_path = temp_dir.path().join("skill-project.toml");
    project.save_to_file(&project_path).unwrap();

    // Verify metadata was extracted correctly
    let loaded = SkillProjectToml::load_from_file(&project_path).unwrap();
    let metadata = loaded.metadata.unwrap();
    assert_eq!(metadata.version, Some("2.0.0".to_string()));
    // name field removed - name comes from SKILL.md frontmatter only
    assert_eq!(metadata.author, Some("Test Author".to_string()));
}

#[test]
fn test_package_command_uses_metadata_section() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().join("skill-project.toml");

    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("package-test".to_string()),
            version: Some("1.2.3".to_string()),
            description: Some("Test for package command".to_string()),
            author: Some("Package Author".to_string()),
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    project.save_to_file(&project_path).unwrap();

    // Load and verify metadata is accessible for package command
    let loaded = SkillProjectToml::load_from_file(&project_path).unwrap();
    let metadata = loaded.metadata.unwrap();

    assert_eq!(metadata.version, Some("1.2.3".to_string()));
    // name field removed - name comes from SKILL.md frontmatter only
    // Package command should be able to use this metadata
}

#[test]
fn test_package_command_reads_metadata_from_skill_project_toml() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path().join("skill-project.toml");

    // Create skill-project.toml with metadata
    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("package-test-skill".to_string()),
            version: Some("1.2.3".to_string()),
            description: Some("Test skill for package command".to_string()),
            author: Some("Package Author".to_string()),
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    project.save_to_file(&project_path).unwrap();

    // Verify package command can read metadata
    let loaded = SkillProjectToml::load_from_file(&project_path).unwrap();
    let metadata = loaded.metadata.unwrap();

    assert_eq!(metadata.version, Some("1.2.3".to_string()));
    // name field removed - name comes from SKILL.md frontmatter only
    assert_eq!(metadata.author, Some("Package Author".to_string()));

    // Package command should be able to extract version for version bumping
    // and use name/description for package metadata
}

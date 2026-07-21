//! Unit tests for SkillProjectToml serialization and deserialization

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill_core::core::manifest::{
    DependenciesSection, DependencySpec, MetadataSection, SkillProjectToml,
};
use fastskill_core::core::origin::Origin;
use std::collections::HashMap;

#[test]
fn test_skill_project_toml_serialization_with_metadata_only() {
    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("my-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: Some("A test skill".to_string()),
            author: Some("Test Author".to_string()),
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    let toml_string = toml::to_string_pretty(&project).unwrap();

    assert!(toml_string.contains("version = \"1.0.0\""));
    assert!(toml_string.contains("id = \"my-skill\""));
    assert!(toml_string.contains("description = \"A test skill\""));
    assert!(toml_string.contains("[metadata]"));
}

#[test]
fn test_skill_project_toml_serialization_with_dependencies_only() {
    let mut deps = HashMap::new();
    deps.insert(
        "skill1".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    );
    deps.insert(
        "skill2".to_string(),
        DependencySpec::Inline {
            origin: Origin::Repository {
                repo: "enterprise".to_string(),
                skill: "skill2".to_string(),
                version: Some(
                    fastskill_core::core::version::VersionConstraint::parse("2.0.0").unwrap(),
                ),
            },
            groups: None,
        },
    );

    let project = SkillProjectToml {
        metadata: None,
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    let toml_string = toml::to_string_pretty(&project).unwrap();

    assert!(toml_string.contains("[dependencies]"));
    assert!(toml_string.contains("skill1 = \"1.0.0\""));
    // TOML serializer uses table format for inline tables: [dependencies.skill2.origin]
    assert!(toml_string.contains("[dependencies.skill2.origin]"));
    assert!(toml_string.contains("type = \"repository\""));
    assert!(toml_string.contains("repo = \"enterprise\""));
    assert!(toml_string.contains("version = \"=2.0.0\""));
}

#[test]
fn test_skill_project_toml_serialization_with_both_sections() {
    let mut deps = HashMap::new();
    deps.insert(
        "helper-skill".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    );

    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("my-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    let toml_string = toml::to_string_pretty(&project).unwrap();

    assert!(toml_string.contains("[metadata]"));
    assert!(toml_string.contains("[dependencies]"));
    assert!(toml_string.contains("version = \"1.0.0\""));
    assert!(toml_string.contains("helper-skill = \"1.0.0\""));
}

#[test]
fn test_skill_project_toml_deserialization() {
    let toml_content = r#"
[metadata]
id = "test-skill"
version = "1.0.0"
description = "Test description"

[dependencies]
skill1 = "1.0.0"
skill2 = { origin = { type = "repository", repo = "enterprise", skill = "skill2", version = "2.0.0" } }
"#;

    let project: SkillProjectToml = toml::from_str(toml_content).unwrap();

    assert!(project.metadata.is_some());
    let metadata = project.metadata.unwrap();
    assert_eq!(metadata.version, Some("1.0.0".to_string()));
    // name field removed - name comes from SKILL.md frontmatter only

    assert!(project.dependencies.is_some());
    let deps = project.dependencies.unwrap();
    assert_eq!(deps.dependencies.len(), 2);
    assert!(deps.dependencies.contains_key("skill1"));
    assert!(deps.dependencies.contains_key("skill2"));
}

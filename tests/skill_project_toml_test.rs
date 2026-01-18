//! Unit tests for SkillProjectToml serialization and deserialization

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::core::manifest::{
    DependenciesSection, DependencySource, DependencySpec, MetadataSection, SkillProjectToml,
    SourceSpecificFields,
};
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
            source: DependencySource::Source,
            source_specific: SourceSpecificFields {
                name: Some("enterprise".to_string()),
                version: Some("2.0.0".to_string()),
                url: None,
                branch: None,
                path: None,
                skill: None,
                zip_url: None,
            },
            groups: None,
            editable: None,
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
    // TOML serializer uses table format for inline tables: [dependencies.skill2]
    assert!(toml_string.contains("[dependencies.skill2]"));
    assert!(toml_string.contains("source = \"source\""));
    assert!(toml_string.contains("name = \"enterprise\""));
    assert!(toml_string.contains("version = \"2.0.0\""));
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
skill2 = { source = "source", name = "enterprise", version = "2.0.0" }
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

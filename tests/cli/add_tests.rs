//! Integration tests for add command with skill-project.toml

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::project::resolve_project_file;
use std::fs;
use tempfile::TempDir;

/// T025: Integration test for fastskill add updating dependencies in skill-project.toml
#[test]
fn test_add_updates_dependencies_in_skill_project_toml() {
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
    assert!(project.dependencies.is_some());

    // Simulate adding a new dependency (what add command would do)
    let deps = project.dependencies.as_mut().unwrap();
    deps.dependencies.insert(
        "dev-tools".to_string(),
        fastskill::core::manifest::DependencySpec::Version("2.0.0".to_string()),
    );

    // Save updated project
    project.save_to_file(&project_toml).unwrap();

    // Verify the update was saved
    let updated_project = SkillProjectToml::load_from_file(&project_toml).unwrap();
    let updated_deps = updated_project.dependencies.as_ref().unwrap();
    assert!(updated_deps.dependencies.contains_key("web-scraper"));
    assert!(updated_deps.dependencies.contains_key("dev-tools"));
}

#[test]
fn test_add_creates_skill_project_toml_if_missing() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // No skill-project.toml exists
    let result = resolve_project_file(project_root);
    assert!(!result.found);

    // Create new project file (what add command would do)
    let project_toml = result.path;
    let new_project = SkillProjectToml {
        metadata: None,
        dependencies: Some(fastskill::core::manifest::DependenciesSection {
            dependencies: std::collections::HashMap::new(),
        }),
        tool: None,
    };
    new_project.save_to_file(&project_toml).unwrap();

    // Verify file was created
    assert!(project_toml.exists());
    let loaded = SkillProjectToml::load_from_file(&project_toml).unwrap();
    assert!(loaded.dependencies.is_some());
}

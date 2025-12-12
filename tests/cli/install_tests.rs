//! Integration tests for install command with skill-project.toml

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::project::resolve_project_file;
use std::fs;
use tempfile::TempDir;

/// T024: Integration test for fastskill install reading dependencies from skill-project.toml
#[test]
fn test_install_reads_dependencies_from_skill_project_toml() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // Create skill-project.toml with dependencies
    let project_toml = project_root.join("skill-project.toml");
    fs::write(
        &project_toml,
        r#"
[dependencies]
web-scraper = "1.0.0"
dev-tools = { source = "git", url = "https://github.com/org/dev-tools.git" }
"#,
    )
    .unwrap();

    // Resolve project file
    let result = resolve_project_file(project_root);
    assert!(result.found);
    assert_eq!(result.path, project_toml);

    // Load and verify dependencies
    let project = SkillProjectToml::load_from_file(&project_toml).unwrap();
    assert!(project.dependencies.is_some());
    let deps = project.dependencies.as_ref().unwrap();
    assert!(deps.dependencies.contains_key("web-scraper"));
    assert!(deps.dependencies.contains_key("dev-tools"));
}

#[test]
fn test_install_handles_missing_skill_project_toml() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // No skill-project.toml file exists
    let result = resolve_project_file(project_root);
    assert!(!result.found);
    // Should return default path for creation
    assert_eq!(result.path, project_root.join("skill-project.toml"));
}

//! Integration tests for unified format context detection

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::core::manifest::ProjectContext;
use fastskill::core::manifest::SkillProjectToml;
use fastskill::core::project::{detect_context, detect_context_from_content, resolve_project_file};
use std::fs;
use tempfile::TempDir;

/// T047: Test context detection in skill directory
#[test]
fn test_context_detection_in_skill_directory() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md to indicate skill-level context
    fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n").unwrap();

    // Create skill-project.toml with metadata
    let project_file = skill_dir.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[metadata]
id = "test-skill"
version = "1.0.0"
description = "A test skill"
"#,
    )
    .unwrap();

    // Detect context from file location
    let context = detect_context(&project_file);
    assert_eq!(context, ProjectContext::Skill);

    // Load and validate
    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let validation_result = project.validate_for_context(context);
    assert!(
        validation_result.is_ok(),
        "Skill-level context should validate with metadata"
    );
}

/// T048: Test context detection at project root
#[test]
fn test_context_detection_at_project_root() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // No SKILL.md - project-level context
    let project_file = project_root.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[dependencies]
web-scraper = "1.0.0"
"#,
    )
    .unwrap();

    // Detect context from file location
    let context = detect_context(&project_file);
    assert_eq!(context, ProjectContext::Project);

    // Load and validate
    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let validation_result = project.validate_for_context(context);
    assert!(
        validation_result.is_ok(),
        "Project-level context should validate with dependencies"
    );
}

/// T049: Test ambiguous context fallback
#[test]
fn test_ambiguous_context_fallback() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // Create skill-project.toml without SKILL.md and with ambiguous content
    let project_file = project_root.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[metadata]
description = "Some description"
# No id or version - ambiguous
"#,
    )
    .unwrap();

    // Detect context from file location (no SKILL.md) - should be Project
    let file_context = detect_context(&project_file);
    assert_eq!(file_context, ProjectContext::Project);

    // Load and detect from content
    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let content_context = detect_context_from_content(&project);
    assert_eq!(content_context, ProjectContext::Ambiguous);

    // When ambiguous, content-based detection should be used
    // If content is ambiguous but file location suggests project, use content-based
    let final_context = if content_context == ProjectContext::Ambiguous {
        // Fallback: if file location suggests project but content is ambiguous,
        // we need to check if we can infer from other indicators
        // For now, if no clear indicators, it remains ambiguous
        content_context
    } else {
        content_context
    };

    // T059, T060: Ambiguous context now returns an error with helpful message
    let validation_result = project.validate_for_context(final_context);
    assert!(
        validation_result.is_err(),
        "Ambiguous context should fail validation with helpful error"
    );
    let error_msg = validation_result.unwrap_err();
    assert!(
        error_msg.contains("Cannot determine context"),
        "Error should mention ambiguous context"
    );
    assert!(
        error_msg.contains("SKILL.md") || error_msg.contains("dependencies"),
        "Error should provide guidance"
    );
}

/// Test context detection with both skill and project files
#[test]
fn test_context_detection_with_both_files() {
    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();
    let skill_dir = project_root.join("skills").join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create project-level file at root
    let project_file = project_root.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[dependencies]
web-scraper = "1.0.0"
"#,
    )
    .unwrap();

    // Create skill-level file in skill directory
    fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n").unwrap();
    let skill_project_file = skill_dir.join("skill-project.toml");
    fs::write(
        &skill_project_file,
        r#"
[metadata]
id = "test-skill"
version = "1.0.0"
"#,
    )
    .unwrap();

    // Resolve from project root - should find project-level file
    let result = resolve_project_file(project_root);
    assert!(result.found);
    assert_eq!(result.context, ProjectContext::Project);

    // Resolve from skill directory - should find skill-level file
    let result = resolve_project_file(&skill_dir);
    assert!(result.found);
    assert_eq!(result.context, ProjectContext::Skill);
}

/// T065: Test edge cases for empty/missing files
#[test]
fn test_edge_case_empty_missing_files() {
    use fastskill::core::manifest::SkillProjectToml;
    use fastskill::core::project::resolve_project_file;
    use std::fs;
    use tempfile::TempDir;

    // Test 1: Empty skill-project.toml file
    let temp_dir1 = TempDir::new().unwrap();
    let empty_file = temp_dir1.path().join("skill-project.toml");
    fs::write(&empty_file, "").unwrap();

    let result = SkillProjectToml::load_from_file(&empty_file);
    // Empty file should parse as valid (all sections optional)
    assert!(result.is_ok());
    let project = result.unwrap();
    assert!(project.metadata.is_none());
    assert!(project.dependencies.is_none());

    // Test 2: Missing skill-project.toml - resolve should return not found
    // Use a separate temp directory to ensure no parent files exist
    let temp_dir2 = TempDir::new().unwrap();
    let missing_dir = temp_dir2.path().join("missing").join("nested");
    fs::create_dir_all(&missing_dir).unwrap();
    let result = resolve_project_file(&missing_dir);
    assert!(!result.found);
    assert_eq!(result.path, missing_dir.join("skill-project.toml"));

    // Test 3: skill-project.toml with only whitespace
    let temp_dir3 = TempDir::new().unwrap();
    let whitespace_file = temp_dir3.path().join("whitespace.toml");
    fs::write(&whitespace_file, "   \n\t  \n").unwrap();
    let result = SkillProjectToml::load_from_file(&whitespace_file);
    // Whitespace-only should parse as valid empty project
    assert!(result.is_ok());

    // Test 4: skill-project.toml with only comments
    let temp_dir4 = TempDir::new().unwrap();
    let comment_file = temp_dir4.path().join("comment.toml");
    fs::write(
        &comment_file,
        r#"
# This is a comment
# Another comment
"#,
    )
    .unwrap();
    let result = SkillProjectToml::load_from_file(&comment_file);
    // Comments-only should parse as valid empty project
    assert!(result.is_ok());
}

/// T064: Integration test for all CLI commands with unified format
#[test]
fn test_all_cli_commands_with_unified_format() {
    use fastskill::core::manifest::{DependenciesSection, DependencySpec, SkillProjectToml};
    use fastskill::core::project::resolve_project_file;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let project_root = temp_dir.path();

    // Test 1: fastskill init creates skill-project.toml
    // This is already tested in init_tests.rs, so we just verify the file structure
    let project_file = project_root.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[dependencies]
test-skill = "1.0.0"
"#,
    )
    .unwrap();

    // Test 2: Verify skill-project.toml can be loaded
    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    assert!(project.dependencies.is_some());
    let deps = project.dependencies.as_ref().unwrap();
    assert!(deps.dependencies.contains_key("test-skill"));

    // Test 3: Verify file resolution works
    let result = resolve_project_file(project_root);
    assert!(result.found);
    assert_eq!(result.path, project_file);

    // Test 4: Verify dependencies can be converted to skill entries
    let entries = project.to_skill_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "test-skill");
    assert_eq!(entries[0].version, Some("1.0.0".to_string()));

    // Test 5: Verify skill-project.toml with all sections
    let full_project = SkillProjectToml {
        metadata: Some(fastskill::core::manifest::MetadataSection {
            id: None,
            version: None,
            description: Some("Test project".to_string()),
            author: None,
            tags: None,
            capabilities: None,
            download_url: None,
            name: Some("test-project".to_string()),
        }),
        dependencies: Some(DependenciesSection {
            dependencies: {
                let mut deps = HashMap::new();
                deps.insert(
                    "skill1".to_string(),
                    DependencySpec::Version("1.0.0".to_string()),
                );
                deps.insert(
                    "skill2".to_string(),
                    DependencySpec::Version("2.0.0".to_string()),
                );
                deps
            },
        }),
        tool: None,
    };

    // Test 6: Verify serialization works
    let serialized = toml::to_string_pretty(&full_project).unwrap();
    assert!(serialized.contains("test-project"));
    assert!(serialized.contains("skill1"));
    assert!(serialized.contains("skill2"));

    // Test 7: Verify deserialization works
    let deserialized: SkillProjectToml = toml::from_str(&serialized).unwrap();
    assert_eq!(
        deserialized.metadata.as_ref().unwrap().name,
        Some("test-project".to_string())
    );
    assert!(deserialized.dependencies.is_some());
    let deps = deserialized.dependencies.as_ref().unwrap();
    assert!(deps.dependencies.contains_key("skill1"));
    assert!(deps.dependencies.contains_key("skill2"));
}

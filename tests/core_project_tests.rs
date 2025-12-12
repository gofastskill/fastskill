//! Unit tests for file resolution priority

use fastskill::core::manifest::ProjectContext;
use fastskill::core::project::{detect_context, resolve_project_file};
use std::fs;
use tempfile::TempDir;

/// T013: Test file resolution priority
///
/// Resolution priority (FR-008):
/// 1. Project root: `./skill-project.toml`
/// 2. Walk up directory tree: Search parent directories
/// 3. Fallback: Empty/default configuration (file not found, will be created if needed)
#[test]
fn test_file_resolution_priority_at_root() {
    // Test priority 1: File at project root
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");
    fs::write(&project_file, "[dependencies]\n").unwrap();

    let result = resolve_project_file(temp_dir.path());

    assert!(result.found);
    assert_eq!(result.path, project_file);
    assert_eq!(result.context, ProjectContext::Project);
}

#[test]
fn test_file_resolution_priority_walk_up() {
    // Test priority 2: Walk up directory tree
    let temp_dir = TempDir::new().unwrap();
    let root_file = temp_dir.path().join("skill-project.toml");
    fs::write(&root_file, "[dependencies]\n").unwrap();

    // Create a subdirectory
    let subdir = temp_dir.path().join("subdir");
    fs::create_dir_all(&subdir).unwrap();

    // Resolve from subdirectory - should find file in parent
    let result = resolve_project_file(&subdir);

    assert!(result.found);
    assert_eq!(result.path, root_file);
    assert_eq!(result.context, ProjectContext::Project);
}

#[test]
fn test_file_resolution_priority_fallback() {
    // Test priority 3: Fallback when file not found
    let temp_dir = TempDir::new().unwrap();

    let result = resolve_project_file(temp_dir.path());

    assert!(!result.found);
    assert_eq!(result.path, temp_dir.path().join("skill-project.toml"));
    assert_eq!(result.context, ProjectContext::Project); // Default to project context
}

#[test]
fn test_file_resolution_priority_closest_wins() {
    // Test that closest file wins when multiple exist
    let temp_dir = TempDir::new().unwrap();

    // Create file at root
    let root_file = temp_dir.path().join("skill-project.toml");
    fs::write(&root_file, "[dependencies]\nroot = \"1.0.0\"\n").unwrap();

    // Create subdirectory with its own file
    let subdir = temp_dir.path().join("subdir");
    fs::create_dir_all(&subdir).unwrap();
    let subdir_file = subdir.join("skill-project.toml");
    fs::write(&subdir_file, "[dependencies]\nsubdir = \"2.0.0\"\n").unwrap();

    // Resolve from subdirectory - should find file in subdirectory (closest)
    let result = resolve_project_file(&subdir);

    assert!(result.found);
    assert_eq!(result.path, subdir_file);
}

#[test]
fn test_detect_context_with_skill_md() {
    // Test context detection when SKILL.md exists
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");
    fs::write(&project_file, "[metadata]\nid = \"test-skill\"\n").unwrap();
    fs::write(temp_dir.path().join("SKILL.md"), "# Skill\n").unwrap();

    let context = detect_context(&project_file);

    assert_eq!(context, ProjectContext::Skill);
}

#[test]
fn test_detect_context_without_skill_md() {
    // Test context detection when SKILL.md doesn't exist
    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");
    fs::write(&project_file, "[dependencies]\n").unwrap();
    // No SKILL.md

    let context = detect_context(&project_file);

    assert_eq!(context, ProjectContext::Project);
}

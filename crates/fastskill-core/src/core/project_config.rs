//! Project configuration loading and validation
//!
//! This module provides a single source of truth for project configuration:
//! - Project root directory
//! - Project file path (skill-project.toml)
//! - Skills directory location
//!
//! All CLI commands and HTTP handlers should use load_project_config() to get these paths.
//! No hardcoded ".claude/skills" or fallback logic - the project file must exist and be valid.

use super::manifest::ProjectContext;
use super::manifest::SkillProjectToml;
use super::project;
use std::path::{Path, PathBuf};

/// Validated project configuration
/// Contains all paths needed for project-level operations
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    /// Directory containing skill-project.toml (the project root)
    pub project_root: PathBuf,
    /// Full path to skill-project.toml file
    pub project_file_path: PathBuf,
    /// Resolved skills directory (absolute or relative to project root)
    pub skills_directory: PathBuf,
}

/// Load and validate project configuration from skill-project.toml
///
/// This function enforces the following requirements:
/// 1. skill-project.toml must exist (no fallback)
/// 2. The file must be project-level (not skill-level with SKILL.md)
/// 3. [tool.fastskill].skills_directory must be present
///
/// # Errors
///
/// Returns an error if:
/// - skill-project.toml is not found in this directory or any parent
/// - The file is skill-level (same directory as SKILL.md)
/// - [tool.fastskill].skills_directory is missing
/// - The file cannot be parsed
pub fn load_project_config(start_path: &Path) -> Result<ProjectConfig, String> {
    // Step 1: Resolve the project file
    let project_file_result = project::resolve_project_file(start_path);

    // Step 2: Check if file was found
    if !project_file_result.found {
        return Err(
            "skill-project.toml not found in this directory or any parent. \
            Create it at the top level of your workspace (e.g. run 'fastskill init' there), \
            then run this command again."
                .to_string(),
        );
    }

    let project_file_path = project_file_result.path;

    // Step 3: Check context - must be project-level, not skill-level
    if project_file_result.context != ProjectContext::Project {
        return Err(
            "skill-project.toml here is for a skill (same directory as SKILL.md). \
            Run install/add/list/update from the project root that has [dependencies] \
            and [tool.fastskill] with skills_directory."
                .to_string(),
        );
    }

    // Step 4: Load the project file
    let project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| format!("Failed to load skill-project.toml: {}", e))?;

    // Step 5: Validate that skills_directory is present
    let skills_directory_opt = project
        .tool
        .as_ref()
        .and_then(|t| t.fastskill.as_ref())
        .and_then(|f| f.skills_directory.as_ref());

    let skills_directory = match skills_directory_opt {
        Some(dir) => dir.clone(),
        None => {
            return Err(
                "project-level skill-project.toml requires [tool.fastskill] with skills_directory. \
                Run 'fastskill init --skills-dir <path>' at project root or add it manually.".to_string()
            );
        }
    };

    // Step 6: Resolve skills_directory (relative to project root if not absolute)
    let project_root = project_file_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let resolved_skills_directory = if skills_directory.is_absolute() {
        skills_directory
    } else {
        project_root.join(&skills_directory)
    };

    // Step 7: Return validated configuration
    Ok(ProjectConfig {
        project_root,
        project_file_path,
        skills_directory: resolved_skills_directory,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_project_config_not_found() {
        let temp_dir = TempDir::new().unwrap();

        let result = load_project_config(temp_dir.path());

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("skill-project.toml not found"));
    }

    #[test]
    fn test_load_project_config_skill_level() {
        let temp_dir = TempDir::new().unwrap();

        // Create SKILL.md to make it skill-level
        fs::write(temp_dir.path().join("SKILL.md"), "# Test Skill").unwrap();

        // Create skill-project.toml
        let content = r#"
[metadata]
id = "test-skill"
version = "1.0.0"
        "#;
        fs::write(temp_dir.path().join("skill-project.toml"), content).unwrap();

        let result = load_project_config(temp_dir.path());

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("is for a skill"));
    }

    #[test]
    fn test_load_project_config_missing_skills_directory() {
        let temp_dir = TempDir::new().unwrap();

        // Create project-level skill-project.toml without [tool.fastskill]
        let content = r#"
[dependencies]
test-skill = "1.0.0"
        "#;
        fs::write(temp_dir.path().join("skill-project.toml"), content).unwrap();

        let result = load_project_config(temp_dir.path());

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("requires [tool.fastskill] with skills_directory"));
    }

    #[test]
    fn test_load_project_config_success_absolute() {
        let temp_dir = TempDir::new().unwrap();
        let skills_path = temp_dir.path().join("skills");

        // Create project-level skill-project.toml with absolute skills_directory
        let content = format!(
            r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = "{}"
        "#,
            skills_path.display()
        );
        fs::write(temp_dir.path().join("skill-project.toml"), content).unwrap();

        let result = load_project_config(temp_dir.path());

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.project_root, temp_dir.path());
        assert_eq!(
            config.project_file_path,
            temp_dir.path().join("skill-project.toml")
        );
        assert_eq!(config.skills_directory, skills_path);
    }

    #[test]
    fn test_load_project_config_success_relative() {
        let temp_dir = TempDir::new().unwrap();

        // Create project-level skill-project.toml with relative skills_directory
        let content = r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"
        "#;
        fs::write(temp_dir.path().join("skill-project.toml"), content).unwrap();

        let result = load_project_config(temp_dir.path());

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.project_root, temp_dir.path());
        assert_eq!(
            config.project_file_path,
            temp_dir.path().join("skill-project.toml")
        );
        assert_eq!(
            config.skills_directory,
            temp_dir.path().join(".claude/skills")
        );
    }

    #[test]
    fn test_load_project_config_walks_up() {
        let temp_dir = TempDir::new().unwrap();

        // Create project-level skill-project.toml at root
        let content = r#"
[dependencies]
test-skill = "1.0.0"

[tool.fastskill]
skills_directory = ".claude/skills"
        "#;
        fs::write(temp_dir.path().join("skill-project.toml"), content).unwrap();

        // Create subdirectory
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir_all(&subdir).unwrap();

        // Load from subdirectory - should find parent file
        let result = load_project_config(&subdir);

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.project_root, temp_dir.path());
        assert_eq!(
            config.project_file_path,
            temp_dir.path().join("skill-project.toml")
        );
        assert_eq!(
            config.skills_directory,
            temp_dir.path().join(".claude/skills")
        );
    }
}

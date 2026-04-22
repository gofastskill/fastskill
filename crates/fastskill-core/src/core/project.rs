//! Project-level file resolution and context detection

use super::manifest::{FileResolutionResult, ProjectContext};
use std::path::Path;

/// Resolve skill-project.toml file from project root
///
/// Resolution priority (FR-008):
/// 1. Project root: `./skill-project.toml`
/// 2. Walk up directory tree: Search parent directories
/// 3. Fallback: Empty/default configuration (file not found, will be created if needed)
///
/// Context detection (T050, T051):
/// - First uses file location (SKILL.md presence)
/// - If ambiguous, falls back to content-based detection
pub fn resolve_project_file(start_path: &Path) -> FileResolutionResult {
    // Start from the given path and walk up
    let mut current = start_path.to_path_buf();

    // First, try to find the file starting from current directory
    loop {
        let project_file = current.join("skill-project.toml");

        if project_file.exists() {
            // First, detect context from file location
            let file_context = detect_context(&project_file);

            // File location is authoritative: SKILL.md in same dir = skill-level, else project-level.
            // Do not override to Skill from content when there is no SKILL.md, since project-level
            // manifests may also have [metadata].id (e.g. workspace id).
            let final_context = file_context;

            return FileResolutionResult {
                path: project_file,
                context: final_context,
                found: true,
            };
        }

        // Check if we've reached the filesystem root
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    // File not found - return default at project root (start_path)
    let default_path = start_path.join("skill-project.toml");
    FileResolutionResult {
        path: default_path,
        context: ProjectContext::Project, // Default to project context
        found: false,
    }
}

/// Detect context for a skill-project.toml file
///
/// Detection logic (FR-006):
/// 1. If skill-project.toml is in directory containing SKILL.md → Skill
/// 2. If skill-project.toml is at project root (no SKILL.md in same directory) → Project
/// 3. Otherwise → Ambiguous (check content: [metadata].id vs [dependencies])
pub fn detect_context(project_file: &Path) -> ProjectContext {
    let dir = project_file.parent().unwrap_or_else(|| Path::new("."));
    let skill_md = dir.join("SKILL.md");

    if skill_md.exists() {
        return ProjectContext::Skill;
    }

    // Check if we're at project root (no SKILL.md in same directory)
    // For now, if no SKILL.md, assume project context
    // Content-based detection will be handled separately
    ProjectContext::Project
}

/// Detect context from file content (for ambiguous cases)
///
/// Content-based detection (FR-006):
/// - If [metadata].id is present → Skill context
/// - If [dependencies] is present → Project context
/// - Otherwise → Ambiguous
pub fn detect_context_from_content(project: &super::manifest::SkillProjectToml) -> ProjectContext {
    // Check for skill-level indicators
    if let Some(ref metadata) = project.metadata {
        if metadata.id.is_some() {
            return ProjectContext::Skill;
        }
    }

    // Check for project-level indicators
    if let Some(ref deps) = project.dependencies {
        if !deps.dependencies.is_empty() {
            return ProjectContext::Project;
        }
    }

    // Ambiguous - cannot determine from content
    ProjectContext::Ambiguous
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_project_file_found() {
        let temp_dir = TempDir::new().unwrap();
        let project_file = temp_dir.path().join("skill-project.toml");
        fs::write(&project_file, "[dependencies]\n").unwrap();

        let result = resolve_project_file(temp_dir.path());

        assert!(result.found);
        assert_eq!(result.path, project_file);
    }

    #[test]
    fn test_resolve_project_file_not_found() {
        let temp_dir = TempDir::new().unwrap();

        let result = resolve_project_file(temp_dir.path());

        assert!(!result.found);
        assert_eq!(result.path, temp_dir.path().join("skill-project.toml"));
    }

    #[test]
    fn test_detect_context_skill() {
        let temp_dir = TempDir::new().unwrap();
        let project_file = temp_dir.path().join("skill-project.toml");
        fs::write(&project_file, "[metadata]\n").unwrap();
        fs::write(temp_dir.path().join("SKILL.md"), "# Skill\n").unwrap();

        let context = detect_context(&project_file);

        assert_eq!(context, ProjectContext::Skill);
    }

    #[test]
    fn test_detect_context_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_file = temp_dir.path().join("skill-project.toml");
        fs::write(&project_file, "[dependencies]\n").unwrap();
        // No SKILL.md

        let context = detect_context(&project_file);

        assert_eq!(context, ProjectContext::Project);
    }

    #[test]
    fn test_resolve_project_file_walk_up() {
        // T013: Test file resolution priority - walk up directory tree
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
    fn test_resolve_project_file_closest_wins() {
        // T013: Test that closest file wins when multiple exist
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
}

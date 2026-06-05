//! Local filesystem skill discovery.

use std::path::{Path, PathBuf};

use super::model::SkillInfo;
use super::SourcesError;

/// Scan a local directory for skills
pub(super) async fn scan_local_source(
    path: &PathBuf,
    source_name: &str,
) -> Result<Vec<SkillInfo>, SourcesError> {
    use walkdir::WalkDir;

    let resolved_path = if path.is_absolute() {
        path.clone()
    } else {
        // Resolve relative to current directory
        std::env::current_dir()
            .map_err(SourcesError::Io)?
            .join(path)
    };

    if !resolved_path.exists() {
        return Err(SourcesError::NotFound(resolved_path));
    }

    if !resolved_path.is_dir() {
        return Err(SourcesError::Io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("Path is not a directory: {}", resolved_path.display()),
        )));
    }

    let mut skills = Vec::new();

    // Walk directory recursively
    for entry in WalkDir::new(&resolved_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let entry_path = entry.path();
        if entry_path.is_file() && entry_path.file_name() == Some(std::ffi::OsStr::new("SKILL.md"))
        {
            // Found a skill directory
            if let Some(skill_dir) = entry_path.parent() {
                // Try to extract skill metadata
                if let Ok(skill_info) = extract_skill_info_from_path(skill_dir, source_name) {
                    skills.push(skill_info);
                }
            }
        }
    }

    Ok(skills)
}

/// Extract skill information from a local path
pub(super) fn extract_skill_info_from_path(
    skill_path: &Path,
    source_name: &str,
) -> Result<SkillInfo, SourcesError> {
    use std::fs;

    let skill_file = skill_path.join("SKILL.md");
    if !skill_file.exists() {
        return Err(SourcesError::NotFound(skill_file));
    }

    // Read SKILL.md to extract metadata
    let content = fs::read_to_string(&skill_file).map_err(SourcesError::Io)?;

    // Extract frontmatter (simple YAML frontmatter parser)
    let (id, name, description, version) = parse_skill_frontmatter(&content, skill_path)?;

    Ok(SkillInfo {
        id,
        name,
        description,
        version: Some(version),
        source_name: source_name.to_string(),
    })
}

/// Parse YAML frontmatter from SKILL.md using serde_yaml.
/// Handles values containing colons correctly (e.g. `description: Tool: does X`).
pub(super) fn parse_skill_frontmatter(
    content: &str,
    skill_path: &Path,
) -> Result<(String, String, String, String), SourcesError> {
    // Use the shared frontmatter parser that uses serde_yaml
    let fallback_id = skill_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    match crate::core::frontmatter::parse_skill_frontmatter(content) {
        Ok(meta) => {
            let id = meta.id.unwrap_or(fallback_id.clone());
            Ok((
                id.clone(),
                meta.name.unwrap_or(id),
                meta.description
                    .unwrap_or_else(|| "No description".to_string()),
                meta.version.unwrap_or_else(|| "1.0.0".to_string()),
            ))
        }
        Err(_) => {
            // No frontmatter or parse error — use directory name as fallback
            Ok((
                fallback_id.clone(),
                fallback_id,
                "No description".to_string(),
                "1.0.0".to_string(),
            ))
        }
    }
}

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
    let (_, name, description, _) = parse_skill_frontmatter(&content, skill_path)?;
    let (id, version) = crate::core::install::read_skill_identity(skill_path)
        .map_err(|error| SourcesError::Parse(error.to_string()))?;

    Ok(SkillInfo {
        id: id.into_string(),
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scan_rejects_missing_and_non_directory_sources() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            scan_local_source(&root.path().join("missing"), "local").await,
            Err(SourcesError::NotFound(_))
        ));
        let file = root.path().join("file");
        std::fs::write(&file, "content").unwrap();
        let error = scan_local_source(&file, "local").await.unwrap_err();
        assert!(error.to_string().contains("not a directory"));
    }

    #[test]
    fn extraction_uses_canonical_metadata_identity_and_reports_missing_file() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            extract_skill_info_from_path(root.path(), "local"),
            Err(SourcesError::NotFound(_))
        ));
        std::fs::write(
            root.path().join("SKILL.md"),
            "---\nname: Display Name\ndescription: fixture\nversion: 1.2.3\nmetadata:\n  id: team/demo\n---\n",
        )
        .unwrap();
        let skill = extract_skill_info_from_path(root.path(), "local").unwrap();
        assert_eq!(skill.id, "team/demo");
        assert_eq!(skill.name, "Display Name");
        assert_eq!(skill.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn frontmatter_falls_back_to_directory_identity() {
        let parsed = parse_skill_frontmatter("plain markdown", Path::new("fallback")).unwrap();
        assert_eq!(parsed.0, "fallback");
        assert_eq!(parsed.2, "No description");
        assert_eq!(parsed.3, "1.0.0");
    }
}

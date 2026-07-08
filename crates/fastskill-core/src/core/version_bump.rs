//! Semantic version bumping for skills

use crate::core::service::ServiceError;
use semver::Version;
use std::path::Path;

/// Version bump type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpType {
    Major,
    Minor,
    Patch,
}

/// Parse a version string into a SemVer version
pub fn parse_version(version: &str) -> Result<Version, ServiceError> {
    // Remove 'v' prefix if present
    let version_str = version.strip_prefix('v').unwrap_or(version);

    Version::parse(version_str)
        .map_err(|e| ServiceError::Custom(format!("Invalid version '{}': {}", version, e)))
}

/// Bump a version based on bump type
pub fn bump_version(version: &Version, bump_type: BumpType) -> Version {
    match bump_type {
        BumpType::Major => Version {
            major: version.major + 1,
            minor: 0,
            patch: 0,
            pre: semver::Prerelease::EMPTY,
            build: semver::BuildMetadata::EMPTY,
        },
        BumpType::Minor => Version {
            major: version.major,
            minor: version.minor + 1,
            patch: 0,
            pre: semver::Prerelease::EMPTY,
            build: semver::BuildMetadata::EMPTY,
        },
        BumpType::Patch => Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch + 1,
            pre: semver::Prerelease::EMPTY,
            build: semver::BuildMetadata::EMPTY,
        },
    }
}

/// Update skill-project.toml with new version.
///
/// This edits the file with `toml_edit` (format-preserving) rather than
/// deserializing into a partial struct and re-serializing. That matters because
/// only `[metadata].version` is touched — `[metadata].id`, any `[tool.*]`
/// sections, comments, formatting, and any other/unknown fields are preserved
/// verbatim. Deserializing into a narrow struct would silently drop everything
/// the struct doesn't enumerate (the BUG-1 data-loss defect).
pub fn update_skill_version(skill_path: &Path, new_version: &str) -> Result<(), ServiceError> {
    use toml_edit::{value, DocumentMut, Item, Table};

    let skill_project_path = skill_path.join("skill-project.toml");

    // Read the existing document, if any. A missing file starts from an empty
    // document; a malformed file recovers to an empty document rather than
    // propagating a hard error (mirrors the previous recovery behavior).
    let mut doc = match std::fs::read_to_string(&skill_project_path) {
        Ok(content) => content
            .parse::<DocumentMut>()
            .unwrap_or_else(|_| DocumentMut::new()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(e) => return Err(ServiceError::Io(e)),
    };

    // Ensure a [metadata] table exists, then set only `version`. Everything else
    // in the document (id, [tool], comments, unknown fields) is left untouched.
    if doc.get("metadata").and_then(Item::as_table).is_none() {
        doc["metadata"] = Item::Table(Table::new());
    }
    doc["metadata"]["version"] = value(new_version.to_string());

    std::fs::write(&skill_project_path, doc.to_string()).map_err(ServiceError::Io)?;
    Ok(())
}

/// Get current version from skill-project.toml
pub fn get_current_version(skill_path: &Path) -> Result<Option<String>, ServiceError> {
    let skill_project_path = skill_path.join("skill-project.toml");

    if !skill_project_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&skill_project_path).map_err(ServiceError::Io)?;

    #[derive(serde::Deserialize)]
    struct SkillProjectToml {
        metadata: Option<SkillProjectMetadata>,
    }

    #[derive(serde::Deserialize)]
    struct SkillProjectMetadata {
        version: Option<String>,
    }

    if let Ok(skill_project) = toml::from_str::<SkillProjectToml>(&content) {
        if let Some(metadata) = skill_project.metadata {
            if let Some(version) = metadata.version {
                if !version.is_empty() {
                    return Ok(Some(version));
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_bump_version_major_minor_patch() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(bump_version(&v, BumpType::Major).to_string(), "2.0.0");
        assert_eq!(bump_version(&v, BumpType::Minor).to_string(), "1.3.0");
        assert_eq!(bump_version(&v, BumpType::Patch).to_string(), "1.2.4");
    }

    #[test]
    fn test_update_version_preserves_id_and_tool_sections() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        let original = r#"# top-level comment
[metadata]
id = "org/my-skill"
version = "1.0.0"
name = "My Skill" # inline comment
description = "does things"
tags = ["a", "b"]

[tool.fastskill]
skills_directory = "skills"
eval_threshold = 0.8

[tool.other]
unknown_field = 42
"#;
        std::fs::write(path.join("skill-project.toml"), original).unwrap();

        update_skill_version(path, "2.5.0").unwrap();

        let updated = std::fs::read_to_string(path.join("skill-project.toml")).unwrap();

        // Version was bumped
        assert!(updated.contains("version = \"2.5.0\""));
        assert!(!updated.contains("version = \"1.0.0\""));

        // id is preserved (the BUG-1 regression)
        assert!(updated.contains("id = \"org/my-skill\""));

        // [tool] sections and their fields are preserved
        assert!(updated.contains("[tool.fastskill]"));
        assert!(updated.contains("skills_directory = \"skills\""));
        assert!(updated.contains("eval_threshold = 0.8"));
        assert!(updated.contains("[tool.other]"));
        assert!(updated.contains("unknown_field = 42"));

        // Other metadata fields preserved
        assert!(updated.contains("name = \"My Skill\""));
        assert!(updated.contains("description = \"does things\""));
        assert!(updated.contains("tags = [\"a\", \"b\"]"));

        // Comments preserved
        assert!(updated.contains("# top-level comment"));
        assert!(updated.contains("# inline comment"));

        // Re-parse and confirm structure is valid + id readable
        let doc = updated.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(doc["metadata"]["id"].as_str(), Some("org/my-skill"));
        assert_eq!(doc["metadata"]["version"].as_str(), Some("2.5.0"));
    }

    #[test]
    fn test_update_version_creates_file_when_absent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();

        update_skill_version(path, "0.1.0").unwrap();

        let content = std::fs::read_to_string(path.join("skill-project.toml")).unwrap();
        assert!(content.contains("version = \"0.1.0\""));
        assert_eq!(
            get_current_version(path).unwrap(),
            Some("0.1.0".to_string())
        );
    }

    #[test]
    fn test_update_version_adds_metadata_table_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        let original = "[tool.fastskill]\nskills_directory = \"skills\"\n";
        std::fs::write(path.join("skill-project.toml"), original).unwrap();

        update_skill_version(path, "3.0.0").unwrap();

        let updated = std::fs::read_to_string(path.join("skill-project.toml")).unwrap();
        assert!(updated.contains("[metadata]"));
        assert!(updated.contains("version = \"3.0.0\""));
        // Pre-existing [tool] table preserved
        assert!(updated.contains("[tool.fastskill]"));
        assert!(updated.contains("skills_directory = \"skills\""));
    }

    #[test]
    fn test_update_version_malformed_file_falls_back() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        std::fs::write(
            path.join("skill-project.toml"),
            "this is not = = valid toml [[[",
        )
        .unwrap();

        update_skill_version(path, "1.1.1").unwrap();

        let updated = std::fs::read_to_string(path.join("skill-project.toml")).unwrap();
        assert!(updated.contains("version = \"1.1.1\""));
        // Now a valid document
        assert!(updated.parse::<toml_edit::DocumentMut>().is_ok());
    }
}

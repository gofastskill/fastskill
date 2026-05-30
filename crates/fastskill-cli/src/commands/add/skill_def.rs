//! Skill definition helpers for the add command.

use crate::error::{CliError, CliResult};
use std::path::Path;

/// Parse skill-project.toml once and return both id and version.
/// Replaces the two separate read_skill_id_from_toml + read_version_from_toml calls.
#[allow(dead_code)]
pub fn read_project_metadata(path: &Path) -> CliResult<(Option<String>, Option<String>)> {
    let skill_project_path = path.join("skill-project.toml");

    if !skill_project_path.exists() {
        return Ok((None, None));
    }

    #[derive(serde::Deserialize)]
    struct SkillProjectToml {
        metadata: Option<fastskill_core::core::manifest::MetadataSection>,
    }

    let content = std::fs::read_to_string(&skill_project_path)
        .map_err(|e| CliError::Validation(format!("Failed to read skill-project.toml: {}", e)))?;

    let project: SkillProjectToml = toml::from_str(&content)
        .map_err(|e| CliError::Validation(format!("Failed to parse skill-project.toml: {}", e)))?;

    if let Some(metadata) = project.metadata {
        Ok((metadata.id, metadata.version))
    } else {
        Ok((None, None))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_read_project_metadata_missing_file() {
        let tmp = TempDir::new().unwrap();
        let (id, version) = read_project_metadata(tmp.path()).unwrap();
        assert!(id.is_none());
        assert!(version.is_none());
    }

    #[test]
    fn test_read_project_metadata_with_metadata() {
        let tmp = TempDir::new().unwrap();
        let content = r#"[metadata]
id = "my-skill"
version = "1.2.3"
"#;
        fs::write(tmp.path().join("skill-project.toml"), content).unwrap();

        let (id, version) = read_project_metadata(tmp.path()).unwrap();
        assert_eq!(id.as_deref(), Some("my-skill"));
        assert_eq!(version.as_deref(), Some("1.2.3"));
    }
}

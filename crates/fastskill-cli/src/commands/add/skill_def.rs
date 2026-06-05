//! Skill definition helpers for the add command.

use crate::error::{CliError, CliResult, CliWarning};
use fastskill_core::SkillDefinition;
use std::fs;
use std::path::Path;

/// Parse skill-project.toml once and return both id and version.
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

#[derive(serde::Deserialize)]
pub(super) struct SkillProjectTomlParsed {
    pub(super) metadata: Option<fastskill_core::core::manifest::MetadataSection>,
}

/// Parse skill-project.toml once. Returns `Ok(None)` when the file does not exist.
pub(super) fn parse_skill_project_toml(
    skill_path: &Path,
) -> CliResult<Option<SkillProjectTomlParsed>> {
    let path = skill_path.join("skill-project.toml");
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| CliError::Validation(format!("Failed to read skill-project.toml: {}", e)))?;
    toml::from_str::<SkillProjectTomlParsed>(&content)
        .map(Some)
        .map_err(|e| CliError::Validation(format!("Failed to parse skill-project.toml: {}", e)))
}

pub(super) fn read_skill_id_from_toml(skill_path: &Path) -> CliResult<Option<String>> {
    let Some(project) = parse_skill_project_toml(skill_path)? else {
        return Ok(None);
    };
    let metadata = project.metadata.ok_or_else(|| {
        CliError::Validation("skill-project.toml must have a [metadata] section".to_string())
    })?;
    Ok(metadata.id)
}

pub(super) fn read_version_from_toml(skill_path: &Path) -> CliResult<Option<String>> {
    Ok(parse_skill_project_toml(skill_path)?
        .and_then(|p| p.metadata)
        .and_then(|m| m.version))
}

fn generate_fallback_skill_id(skill_path: &Path) -> CliResult<String> {
    let skill_md_path = skill_path.join("SKILL.md");
    let content = std::fs::read_to_string(&skill_md_path)
        .map_err(|e| CliError::Validation(format!("Failed to read SKILL.md: {}", e)))?;
    let frontmatter =
        fastskill_core::core::metadata::parse_yaml_frontmatter(&content).map_err(|e| {
            CliError::Validation(format!("Failed to parse SKILL.md frontmatter: {}", e))
        })?;
    if let Some(metadata) = &frontmatter.metadata {
        if let Some(id) = metadata.get("id") {
            return Ok(id.clone());
        }
    }
    Ok(frontmatter.name.clone())
}

/// Create a skill definition from a path containing SKILL.md.
/// The skill ID is read from skill-project.toml if present, otherwise from SKILL.md frontmatter.
pub fn create_skill_from_path(
    skill_path: &Path,
    source_type: &str,
    editable: bool,
) -> CliResult<SkillDefinition> {
    let (skill_id_str, missing_toml) = match read_skill_id_from_toml(skill_path)? {
        Some(id) => (id, false),
        None => (generate_fallback_skill_id(skill_path)?, true),
    };
    if missing_toml {
        CliWarning::MissingSkillProjectToml {
            path: skill_path.to_path_buf(),
            fallback_id: skill_id_str.clone(),
        }
        .display(source_type, editable);
    }

    let skill_file = skill_path.join("SKILL.md");
    let content = fs::read_to_string(&skill_file).map_err(CliError::Io)?;

    let frontmatter = fastskill_core::core::metadata::parse_yaml_frontmatter(&content)
        .map_err(|e| CliError::Validation(format!("Failed to parse SKILL.md: {}", e)))?;

    let skill_id_str_clone = skill_id_str.clone();
    let skill_id = fastskill_core::SkillId::new(skill_id_str).map_err(|_| {
        CliError::Validation(format!("Invalid skill ID format: {}", skill_id_str_clone))
    })?;

    let version = read_version_from_toml(skill_path)?.or_else(|| {
        frontmatter
            .metadata
            .as_ref()
            .and_then(|m| m.get("version"))
            .or(frontmatter.version.as_ref())
            .cloned()
    });
    let version = if let Some(v) = version {
        v
    } else {
        eprintln!("⚠  Note: No version specified in SKILL.md frontmatter, defaulting to 1.0.0");
        "1.0.0".to_string()
    };

    let mut skill =
        SkillDefinition::new(skill_id, frontmatter.name, frontmatter.description, version);

    skill.skill_file = skill_file.clone();
    skill.author = frontmatter.author;

    Ok(skill)
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

    #[test]
    fn test_create_skill_from_path_without_toml() {
        let tmp = TempDir::new().unwrap();
        let skill_md = r#"---
name: test-skill-no-toml
description: Test skill without skill-project.toml
metadata:
  id: test-skill-no-toml
  version: "1.0.0"
---
Test skill content
"#;
        fs::write(tmp.path().join("SKILL.md"), skill_md).unwrap();

        let skill = create_skill_from_path(tmp.path(), "local", false).unwrap();
        assert_eq!(skill.id.as_str(), "test-skill-no-toml");
    }

    #[test]
    fn test_create_skill_from_path_toml_takes_precedence() {
        let tmp = TempDir::new().unwrap();
        let skill_md = r#"---
name: test-skill-both
description: Test skill
metadata:
  id: from-skill-md
  version: "2.0.0"
---
Test content
"#;
        fs::write(tmp.path().join("SKILL.md"), skill_md).unwrap();
        let project_toml = r#"[metadata]
id = "from-toml"
version = "1.5.0"
"#;
        fs::write(tmp.path().join("skill-project.toml"), project_toml).unwrap();

        let skill = create_skill_from_path(tmp.path(), "local", false).unwrap();
        assert_eq!(skill.id.as_str(), "from-toml");
        assert_eq!(skill.version, "1.5.0");
    }

    #[test]
    fn test_create_skill_from_path_fallback_to_name() {
        let tmp = TempDir::new().unwrap();
        let skill_md = r#"---
name: fallback-name-skill
description: Test skill
---
Test content
"#;
        fs::write(tmp.path().join("SKILL.md"), skill_md).unwrap();

        let skill = create_skill_from_path(tmp.path(), "local", false).unwrap();
        assert_eq!(skill.id.as_str(), "fallback-name-skill");
    }

    #[test]
    fn test_create_skill_from_path_invalid_name_fails() {
        let tmp = TempDir::new().unwrap();
        let skill_md = r#"---
name: invalid name with spaces
description: Test skill
---
Test content
"#;
        fs::write(tmp.path().join("SKILL.md"), skill_md).unwrap();

        let result = create_skill_from_path(tmp.path(), "local", false);
        assert!(result.is_err(), "Should fail due to spaces in name");
    }
}

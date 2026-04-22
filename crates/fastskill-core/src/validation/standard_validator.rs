//! StandardValidator implements AI Skill standard validation
//!
//! This module provides comprehensive validation for AI skills according to
//! AI Skill standard specification, covering name format, description length,
//! directory structure, file references, and metadata field constraints.

use crate::core::metadata::SkillFrontmatter;
use crate::core::service::ServiceError;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

// Compile regexes once at startup
static NAME_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*$").expect("Invalid name regex")
});
static FILE_REF_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"\\.?(?:/|\\\\)?(?:scripts|references|assets)(?:/|\\\\)[^/\\s)]+")
        .expect("Invalid file reference regex")
});

/// Validation result containing outcome and detailed error reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<String>,
    pub skill_path: PathBuf,
}

/// Specific validation error types for precise error reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationError {
    InvalidNameFormat(String),
    NameMismatch { expected: String, actual: String },
    InvalidDescriptionLength(usize),
    InvalidCompatibilityLength(usize),
    MissingRequiredField(String),
    InvalidFileReference(String),
    InvalidDirectoryStructure(String),
    YamlParseError(String),
}

/// Core validator implementing AI Skill standard requirements
pub struct StandardValidator;

impl StandardValidator {
    /// Validate skill name format according to standard
    pub fn validate_name(name: &str) -> Result<(), ValidationError> {
        // Name format: 1-64 chars, lowercase alphanumeric + hyphens, no leading/trailing/consecutive hyphens

        if name.is_empty() || name.len() > 64 {
            return Err(ValidationError::InvalidNameFormat(format!(
                "Skill name '{}' must be 1-64 characters long",
                name
            )));
        }

        if !NAME_REGEX.is_match(name) {
            return Err(ValidationError::InvalidNameFormat(
                format!(
                    "Skill name '{}' contains invalid characters. Use only lowercase alphanumeric and hyphens, no leading/trailing/consecutive hyphens",
                    name
                )
            ));
        }

        Ok(())
    }

    /// Validate skill description length
    pub fn validate_description(description: &str) -> Result<(), ValidationError> {
        if description.is_empty() || description.len() > 1024 {
            return Err(ValidationError::InvalidDescriptionLength(description.len()));
        }
        Ok(())
    }

    /// Main validation method for skill directory
    pub fn validate_skill_directory(skill_path: &Path) -> Result<ValidationResult, ServiceError> {
        let skill_md_path = skill_path.join("SKILL.md");

        if !skill_md_path.exists() {
            return Ok(ValidationResult {
                is_valid: false,
                errors: vec![ValidationError::InvalidDirectoryStructure(format!(
                    "SKILL.md not found in {}",
                    skill_path.display()
                ))],
                warnings: vec![],
                skill_path: skill_path.to_path_buf(),
            });
        }

        let frontmatter = Self::parse_frontmatter(&skill_md_path)?;

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate name
        if let Err(e) = Self::validate_name(&frontmatter.name) {
            errors.push(e);
        }

        // Validate description length
        if let Err(e) = Self::validate_description(&frontmatter.description) {
            errors.push(e);
        }

        // Validate compatibility
        if let Some(compatibility) = &frontmatter.compatibility {
            if compatibility.len() > 256 {
                errors.push(ValidationError::InvalidCompatibilityLength(
                    compatibility.len(),
                ));
            }
        } else {
            warnings.push("No compatibility field specified".to_string());
        }

        // Validate file references
        Self::validate_file_references(skill_path, &frontmatter, &mut errors);

        // Check directory structure
        Self::validate_directory_structure(skill_path, &frontmatter, &mut errors);

        Ok(ValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
            skill_path: skill_path.to_path_buf(),
        })
    }

    /// Parse YAML frontmatter from SKILL.md
    fn parse_frontmatter(skill_md_path: &Path) -> Result<SkillFrontmatter, ServiceError> {
        let content = std::fs::read_to_string(skill_md_path).map_err(ServiceError::Io)?;

        // Simple frontmatter extraction (between --- markers)
        let parts: Vec<&str> = content.split("---").collect();
        if parts.len() < 3 {
            return Err(ServiceError::Validation(
                "No YAML frontmatter found in SKILL.md".to_string(),
            ));
        }

        let yaml_content = parts[1];
        let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_content)
            .map_err(|e| ServiceError::Custom(format!("Failed to parse SKILL.md: {}", e)))?;

        Ok(frontmatter)
    }

    /// Validate that referenced files exist
    fn validate_file_references(
        skill_path: &Path,
        frontmatter: &SkillFrontmatter,
        errors: &mut Vec<ValidationError>,
    ) {
        // Check for file references in description and other text fields
        let text_fields = vec![("description", &frontmatter.description)];

        for (field_name, content) in text_fields {
            for capture in FILE_REF_REGEX.find_iter(content) {
                let file_ref = capture.as_str();

                // Convert to relative path from skill root
                let relative_path = if let Some(stripped) = file_ref.strip_prefix("./") {
                    stripped
                } else {
                    file_ref
                };

                let full_path = skill_path.join(relative_path);

                // Check if referenced file exists
                if !full_path.exists() {
                    errors.push(ValidationError::InvalidFileReference(format!(
                        "File reference '{}' in {} does not exist",
                        file_ref, field_name
                    )));
                }

                // Check if file is within allowed directories
                if let Ok(canonical_path) = full_path.canonicalize() {
                    if let Ok(canonical_skill_path) = skill_path.canonicalize() {
                        if !canonical_path.starts_with(&canonical_skill_path) {
                            errors.push(ValidationError::InvalidFileReference(format!(
                                "File reference '{}' in {} points outside skill directory",
                                file_ref, field_name
                            )));
                        }
                    }
                }
            }
        }
    }

    /// Validate directory structure meets requirements
    fn validate_directory_structure(
        skill_path: &Path,
        frontmatter: &SkillFrontmatter,
        errors: &mut Vec<ValidationError>,
    ) {
        // Check if scripts directory exists if referenced
        if frontmatter.description.contains("scripts") {
            let scripts_path = skill_path.join("scripts");
            if !scripts_path.exists() {
                errors.push(ValidationError::InvalidDirectoryStructure(
                    "scripts/ directory referenced but not found".to_string(),
                ));
            }
        }

        // Check if references directory exists if referenced
        if frontmatter.description.contains("references") {
            let references_path = skill_path.join("references");
            if !references_path.exists() {
                errors.push(ValidationError::InvalidDirectoryStructure(
                    "references/ directory referenced but not found".to_string(),
                ));
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_validate_name_valid() {
        assert!(StandardValidator::validate_name("my-skill").is_ok());
        assert!(StandardValidator::validate_name("skill123").is_ok());
        assert!(StandardValidator::validate_name("my-valid-skill-name").is_ok());
    }

    #[test]
    fn test_validate_name_invalid() {
        assert!(StandardValidator::validate_name("").is_err());
        assert!(StandardValidator::validate_name("MY-SKILL").is_err());
        assert!(StandardValidator::validate_name("-skill").is_err());
        assert!(StandardValidator::validate_name("skill-").is_err());
        assert!(StandardValidator::validate_name("skill--name").is_err());
    }

    #[test]
    fn test_validate_description_valid() {
        assert!(StandardValidator::validate_description("A valid description").is_ok());
        assert!(StandardValidator::validate_description("x".repeat(100).as_str()).is_ok());
    }

    #[test]
    fn test_validate_description_invalid() {
        assert!(StandardValidator::validate_description("").is_err());
        assert!(StandardValidator::validate_description("x".repeat(2000).as_str()).is_err());
    }

    #[test]
    fn test_validate_skill_directory_valid() {
        let temp_dir = TempDir::new().unwrap();
        let skill_path = temp_dir.path();

        let skill_md_content = r#"---
name: test-skill
version: "1.0.0"
description: A test skill
---
"#;
        std::fs::write(skill_path.join("SKILL.md"), skill_md_content).unwrap();

        let result = StandardValidator::validate_skill_directory(skill_path).unwrap();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_skill_directory_missing_required() {
        let temp_dir = TempDir::new().unwrap();
        let skill_path = temp_dir.path();

        // Test with empty name (name is still required)
        let skill_md_content = r#"---
name: ""
description: A test skill
---
"#;
        std::fs::write(skill_path.join("SKILL.md"), skill_md_content).unwrap();

        let result = StandardValidator::validate_skill_directory(skill_path).unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_parse_frontmatter_valid() {
        let temp_dir = TempDir::new().unwrap();
        let skill_path = temp_dir.path();

        let skill_md_content = r#"---
name: test-skill
version: "1.0.0"
description: A test skill
---
Content here
"#;
        std::fs::write(skill_path.join("SKILL.md"), skill_md_content).unwrap();

        let frontmatter =
            StandardValidator::parse_frontmatter(&skill_path.join("SKILL.md")).unwrap();
        assert_eq!(frontmatter.name, "test-skill");
        assert_eq!(frontmatter.version.as_deref(), Some("1.0.0"));
        assert_eq!(frontmatter.description, "A test skill");
    }

    #[test]
    fn test_validate_file_references_missing() {
        let temp_dir = TempDir::new().unwrap();
        let skill_path = temp_dir.path();

        let skill_md_content = r#"---
name: test-skill
version: "1.0.0"
description: See ./scripts/test.sh for details
---
"#;
        std::fs::write(skill_path.join("SKILL.md"), skill_md_content).unwrap();

        let result = StandardValidator::validate_skill_directory(skill_path).unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }
}

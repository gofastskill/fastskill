//! StandardValidator implements AI Skill standard validation
//!
//! This module provides comprehensive validation for AI skills according to
//! the AI Skill standard specification, covering name format, description length,
//! directory structure, file references, and metadata field constraints.

use crate::core::metadata::SkillFrontmatter;
use crate::core::service::ServiceError;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
        let name_regex = Regex::new(r"^[a-z0-9]([a-z0-9-]*[a-z0-9])?$").unwrap();

        if name.is_empty() || name.len() > 64 {
            return Err(ValidationError::InvalidNameFormat(format!(
                "Skill name '{}' must be 1-64 characters long",
                name
            )));
        }

        if !name_regex.is_match(name) {
            return Err(ValidationError::InvalidNameFormat(
                format!("Skill name '{}' contains invalid characters. Use only lowercase alphanumeric and hyphens, no leading/trailing/consecutive hyphens", name)
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
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check if SKILL.md exists
        let skill_md_path = skill_path.join("SKILL.md");
        if !skill_md_path.exists() {
            errors.push(ValidationError::InvalidDirectoryStructure(
                "SKILL.md not found at skill root".to_string(),
            ));
            return Ok(ValidationResult {
                is_valid: false,
                errors,
                warnings,
                skill_path: skill_path.to_path_buf(),
            });
        }

        // Parse frontmatter
        match Self::parse_frontmatter(&skill_md_path) {
            Ok(frontmatter) => {
                // Validate frontmatter
                Self::validate_frontmatter(&frontmatter, skill_path, &mut errors, &mut warnings);

                // Validate directory structure
                Self::validate_directory_structure(skill_path, &mut errors);

                // Validate file references
                Self::validate_file_references(skill_path, &frontmatter, &mut errors);
            }
            Err(e) => {
                errors.push(ValidationError::YamlParseError(e.to_string()));
            }
        }

        Ok(ValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
            skill_path: skill_path.to_path_buf(),
        })
    }

    fn parse_frontmatter(skill_md_path: &Path) -> Result<SkillFrontmatter, ServiceError> {
        let content = std::fs::read_to_string(skill_md_path).map_err(|e| ServiceError::Io(e))?;

        // Simple frontmatter extraction (between --- markers)
        let parts: Vec<&str> = content.split("---").collect();
        if parts.len() < 3 {
            return Err(ServiceError::Validation(
                "No YAML frontmatter found in SKILL.md".to_string(),
            ));
        }

        let yaml_content = parts[1];
        let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_content)
            .map_err(|e| ServiceError::Validation(format!("YAML parse error: {}", e)))?;

        Ok(frontmatter)
    }

    fn validate_frontmatter(
        frontmatter: &SkillFrontmatter,
        skill_path: &Path,
        errors: &mut Vec<ValidationError>,
        warnings: &mut Vec<String>,
    ) {
        // Validate name format
        if let Err(e) = Self::validate_name(&frontmatter.name) {
            errors.push(e);
        }

        // Validate name matches directory
        if let Some(dir_name) = skill_path.file_name().and_then(|n| n.to_str()) {
            if frontmatter.name != dir_name {
                errors.push(ValidationError::NameMismatch {
                    expected: frontmatter.name.clone(),
                    actual: dir_name.to_string(),
                });
            }
        }

        // Validate description
        if let Err(e) = Self::validate_description(&frontmatter.description) {
            errors.push(e);
        }

        // Validate optional fields
        if let Some(compatibility) = &frontmatter.compatibility {
            if compatibility.len() > 500 {
                errors.push(ValidationError::InvalidCompatibilityLength(
                    compatibility.len(),
                ));
            }
        }

        // Validate metadata format (should be key-value pairs)
        if let Some(metadata) = &frontmatter.metadata {
            for (key, value) in metadata {
                if key.is_empty() {
                    errors.push(ValidationError::InvalidFileReference(
                        "Metadata contains empty key".to_string(),
                    ));
                }
                if value.is_empty() {
                    errors.push(ValidationError::InvalidFileReference(format!(
                        "Metadata key '{}' has empty value",
                        key
                    )));
                }
            }
        }

        // Check for SKILL.md length warning
        if let Ok(content) = std::fs::read_to_string(skill_path.join("SKILL.md")) {
            let lines = content.lines().count();
            if lines > 500 {
                warnings.push(format!(
                    "SKILL.md exceeds 500 lines ({}) - consider organizing content",
                    lines
                ));
            }
        }
    }

    fn validate_directory_structure(skill_path: &Path, errors: &mut Vec<ValidationError>) {
        // Check for required SKILL.md file
        let skill_md_path = skill_path.join("SKILL.md");
        if !skill_md_path.exists() {
            errors.push(ValidationError::InvalidDirectoryStructure(
                "SKILL.md not found at skill root".to_string(),
            ));
            return; // Can't validate further without SKILL.md
        }

        // Check directory structure
        if let Ok(entries) = std::fs::read_dir(skill_path) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    // Skip SKILL.md (already validated)
                    if name == "SKILL.md" {
                        continue;
                    }

                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_file() {
                            // Allow specific files at root level
                            if matches!(name, "skill-project.toml") {
                                continue; // This file is allowed at root
                            }
                            // Files should be in subdirectories, not at root (except SKILL.md and skill-project.toml)
                            errors.push(ValidationError::InvalidDirectoryStructure(
                                format!("File '{}' found at skill root - files should be in subdirectories (scripts/, references/, assets/)", name)
                            ));
                        } else if file_type.is_dir() {
                            // Only allow specific subdirectories
                            if !matches!(name, "scripts" | "references" | "assets") {
                                errors.push(ValidationError::InvalidDirectoryStructure(
                                    format!("Invalid directory '{}' - only 'scripts', 'references', and 'assets' directories are allowed", name)
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    fn validate_file_references(
        skill_path: &Path,
        frontmatter: &SkillFrontmatter,
        errors: &mut Vec<ValidationError>,
    ) {
        // Check for file references in description and other text fields
        let text_fields = vec![("description", &frontmatter.description)];

        for (field_name, content) in text_fields {
            // Simple pattern to find relative file references like "./scripts/file.sh" or "references/doc.md"
            let file_ref_regex =
                regex::Regex::new(r"\.?(?:/|\\)?(?:scripts|references|assets)(?:/|\\)[^/\s)]+")
                    .unwrap();

            for capture in file_ref_regex.find_iter(content) {
                let file_ref = capture.as_str();

                // Convert to relative path from skill root
                let relative_path = if file_ref.starts_with("./") {
                    &file_ref[2..]
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

                        // Additional check: ensure path is within allowed subdirectories
                        let skill_path_str = canonical_skill_path.to_string_lossy();
                        let file_path_str = canonical_path.to_string_lossy();

                        let relative_to_skill = file_path_str
                            .strip_prefix(&format!(
                                "{}{}",
                                skill_path_str,
                                std::path::MAIN_SEPARATOR
                            ))
                            .unwrap_or(&file_path_str);

                        if !relative_to_skill.starts_with("scripts/")
                            && !relative_to_skill.starts_with("references/")
                            && !relative_to_skill.starts_with("assets/")
                        {
                            errors.push(ValidationError::InvalidFileReference(
                                format!("File reference '{}' in {} must be within scripts/, references/, or assets/ directories", file_ref, field_name)
                            ));
                        }
                    }
                }
            }
        }
    }
}

impl Default for StandardValidator {
    fn default() -> Self {
        Self
    }
}

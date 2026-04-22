//! Skill field validation (required fields, formats, lengths).

use crate::core::skill_manager::SkillDefinition;
use crate::validation::result::{ErrorSeverity, ValidationResult};

pub(crate) fn is_valid_skill_name(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

pub(crate) fn is_valid_semver(s: &str) -> bool {
    semver::Version::parse(s).is_ok()
}

pub(crate) fn validate_required_fields(
    skill: &SkillDefinition,
    mut result: ValidationResult,
    required_fields: &[String],
    max_description_length: usize,
) -> ValidationResult {
    for field in required_fields {
        match field.as_str() {
            "name" => {
                if skill.name.trim().is_empty() {
                    result = result.with_error(
                        "name",
                        "Skill name cannot be empty",
                        ErrorSeverity::Critical,
                    );
                }
            }
            "description" => {
                if skill.description.trim().is_empty() {
                    result = result.with_error(
                        "description",
                        "Skill description cannot be empty",
                        ErrorSeverity::Critical,
                    );
                } else if skill.description.len() > max_description_length {
                    result = result.with_warning(
                        "description",
                        &format!(
                            "Description is very long ({} chars, max: {})",
                            skill.description.len(),
                            max_description_length
                        ),
                    );
                }
            }
            "version" => {
                if skill.version.trim().is_empty() {
                    result = result.with_error(
                        "version",
                        "Skill version cannot be empty",
                        ErrorSeverity::Critical,
                    );
                }
            }
            _ => {}
        }
    }
    result
}

pub(crate) fn validate_field_formats(
    skill: &SkillDefinition,
    mut result: ValidationResult,
) -> ValidationResult {
    if !is_valid_skill_name(&skill.name) {
        result = result.with_warning(
            "name",
            "Skill name should only contain alphanumeric characters, hyphens, and underscores",
        );
    }
    if !is_valid_semver(&skill.version) {
        result = result.with_warning(
            "version",
            "Version should follow semantic versioning (e.g., 1.0.0)",
        );
    }
    result
}

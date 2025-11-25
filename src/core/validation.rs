//! Validation functions for skill-project.toml and related structures

use regex::Regex;
use std::collections::HashSet;

/// Validate semantic version format (MAJOR.MINOR.PATCH)
pub fn validate_semver(version: &str) -> Result<(), ValidationError> {
    let semver_regex = Regex::new(r"^\d+\.\d+\.\d+$")
        .map_err(|e| ValidationError::Internal(format!("Failed to compile semver regex: {}", e)))?;

    if !semver_regex.is_match(version) {
        return Err(ValidationError::InvalidSemver(version.to_string()));
    }

    Ok(())
}

/// Validate identifier format (alphanumeric, hyphens, underscores)
pub fn validate_identifier(identifier: &str) -> Result<(), ValidationError> {
    if identifier.is_empty() {
        return Err(ValidationError::EmptyIdentifier);
    }

    let identifier_regex = Regex::new(r"^[a-zA-Z0-9_-]+$").map_err(|e| {
        ValidationError::Internal(format!("Failed to compile identifier regex: {}", e))
    })?;

    if !identifier_regex.is_match(identifier) {
        return Err(ValidationError::InvalidIdentifier(identifier.to_string()));
    }

    Ok(())
}

/// Validate that skill-project.toml has at least one section
pub fn validate_project_structure(
    has_metadata: bool,
    has_dependencies: bool,
) -> Result<(), ValidationError> {
    if !has_metadata && !has_dependencies {
        return Err(ValidationError::MissingSections);
    }

    Ok(())
}

/// Validate name + version uniqueness
/// Checks if a skill with the same name and version already exists
pub fn validate_uniqueness(
    name: &str,
    version: &str,
    existing_skills: &HashSet<(String, String)>,
) -> Result<(), ValidationError> {
    // First validate the inputs
    validate_identifier(name)?;
    validate_semver(version)?;

    let key = (name.to_string(), version.to_string());
    if existing_skills.contains(&key) {
        return Err(ValidationError::DuplicateSkill {
            name: name.to_string(),
            version: version.to_string(),
        });
    }

    Ok(())
}

/// Validation errors
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Invalid semantic version format: {0}. Expected MAJOR.MINOR.PATCH")]
    InvalidSemver(String),

    #[error("Invalid identifier: {0}. Must contain only alphanumeric characters, hyphens, and underscores")]
    InvalidIdentifier(String),

    #[error("Empty identifier not allowed")]
    EmptyIdentifier,

    #[error("skill-project.toml must have at least one section ([metadata] or [dependencies])")]
    MissingSections,

    #[error("Duplicate skill: {name}@{version} already exists")]
    DuplicateSkill { name: String, version: String },

    #[error("Internal validation error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for validate_semver
    #[test]
    fn test_validate_semver_valid() {
        assert!(validate_semver("1.0.0").is_ok());
        assert!(validate_semver("0.1.0").is_ok());
        assert!(validate_semver("10.20.30").is_ok());
        assert!(validate_semver("999.999.999").is_ok());
        assert!(validate_semver("0.0.1").is_ok());
    }

    #[test]
    fn test_validate_semver_invalid() {
        assert!(validate_semver("1.0").is_err());
        assert!(validate_semver("1").is_err());
        assert!(validate_semver("v1.0.0").is_err());
        assert!(validate_semver("1.0.0-beta").is_err());
        assert!(validate_semver("1.0.0+metadata").is_err());
        assert!(validate_semver("invalid").is_err());
        assert!(validate_semver("1.0.0.0").is_err());
        assert!(validate_semver("1.0").is_err());
    }

    #[test]
    fn test_validate_semver_edge_cases() {
        // Empty string
        assert!(validate_semver("").is_err());

        // Very long version string (actually valid format, just large numbers)
        let long_version = "1".repeat(100) + ".0.0";
        // This is technically valid semver format, so we test it passes
        assert!(validate_semver(&long_version).is_ok());

        // Special characters (invalid)
        assert!(validate_semver("1.0.0-alpha").is_err());
        assert!(validate_semver("1.0.0_beta").is_err());
        assert!(validate_semver("1.0.0.beta").is_err());

        // Assert error type
        if let Err(ValidationError::InvalidSemver(_)) = validate_semver("1.0") {
            // Correct error type
        } else {
            panic!("Expected InvalidSemver error");
        }
    }

    // Tests for validate_identifier
    #[test]
    fn test_validate_identifier_valid() {
        assert!(validate_identifier("my-skill").is_ok());
        assert!(validate_identifier("my_skill").is_ok());
        assert!(validate_identifier("skill123").is_ok());
        assert!(validate_identifier("skill-123_test").is_ok());
        assert!(validate_identifier("a").is_ok());
        assert!(validate_identifier("A").is_ok());
        assert!(validate_identifier("123").is_ok());
        assert!(validate_identifier(&"a".repeat(100)).is_ok());
    }

    #[test]
    fn test_validate_identifier_invalid() {
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier("my skill").is_err()); // space
        assert!(validate_identifier("my.skill").is_err()); // dot
        assert!(validate_identifier("my@skill").is_err()); // @ symbol
        assert!(validate_identifier("my#skill").is_err()); // # symbol
        assert!(validate_identifier("my!skill").is_err()); // ! symbol
    }

    #[test]
    fn test_validate_identifier_edge_cases() {
        // Empty string
        assert!(validate_identifier("").is_err());
        if let Err(ValidationError::EmptyIdentifier) = validate_identifier("") {
            // Correct error type
        } else {
            panic!("Expected EmptyIdentifier error");
        }

        // Single character (valid)
        assert!(validate_identifier("a").is_ok());
        assert!(validate_identifier("_").is_ok());
        assert!(validate_identifier("-").is_ok());

        // Very long identifier
        let long_id = "a".repeat(1000);
        assert!(validate_identifier(&long_id).is_ok());

        // Mixed case
        assert!(validate_identifier("MySkill_123-Test").is_ok());
    }

    // Tests for validate_project_structure
    #[test]
    fn test_validate_project_structure_valid() {
        // Has metadata only
        assert!(validate_project_structure(true, false).is_ok());

        // Has dependencies only
        assert!(validate_project_structure(false, true).is_ok());

        // Has both
        assert!(validate_project_structure(true, true).is_ok());
    }

    #[test]
    fn test_validate_project_structure_invalid() {
        // Has neither
        assert!(validate_project_structure(false, false).is_err());

        if let Err(ValidationError::MissingSections) = validate_project_structure(false, false) {
            // Correct error type
        } else {
            panic!("Expected MissingSections error");
        }
    }

    // Tests for validate_uniqueness
    #[test]
    fn test_validate_uniqueness_unique() {
        let mut existing_skills = HashSet::new();
        existing_skills.insert(("other-skill".to_string(), "1.0.0".to_string()));

        // Unique skill name and version
        assert!(validate_uniqueness("my-skill", "1.0.0", &existing_skills).is_ok());

        // Same name, different version
        assert!(validate_uniqueness("other-skill", "2.0.0", &existing_skills).is_ok());

        // Different name, same version
        assert!(validate_uniqueness("new-skill", "1.0.0", &existing_skills).is_ok());
    }

    #[test]
    fn test_validate_uniqueness_duplicate() {
        let mut existing_skills = HashSet::new();
        existing_skills.insert(("my-skill".to_string(), "1.0.0".to_string()));

        // Duplicate name and version
        assert!(validate_uniqueness("my-skill", "1.0.0", &existing_skills).is_err());

        if let Err(ValidationError::DuplicateSkill { name, version }) =
            validate_uniqueness("my-skill", "1.0.0", &existing_skills)
        {
            assert_eq!(name, "my-skill");
            assert_eq!(version, "1.0.0");
        } else {
            panic!("Expected DuplicateSkill error");
        }
    }

    #[test]
    fn test_validate_uniqueness_invalid_inputs() {
        let existing_skills = HashSet::new();

        // Invalid identifier
        assert!(validate_uniqueness("", "1.0.0", &existing_skills).is_err());
        assert!(validate_uniqueness("my skill", "1.0.0", &existing_skills).is_err());

        // Invalid version
        assert!(validate_uniqueness("my-skill", "1.0", &existing_skills).is_err());
        assert!(validate_uniqueness("my-skill", "invalid", &existing_skills).is_err());
    }

    #[test]
    fn test_validate_uniqueness_edge_cases() {
        let mut existing_skills = HashSet::new();
        existing_skills.insert(("skill".to_string(), "0.0.1".to_string()));

        // Empty existing skills set
        let empty_set = HashSet::new();
        assert!(validate_uniqueness("new-skill", "1.0.0", &empty_set).is_ok());

        // Multiple existing skills
        existing_skills.insert(("skill2".to_string(), "2.0.0".to_string()));
        existing_skills.insert(("skill3".to_string(), "3.0.0".to_string()));
        assert!(validate_uniqueness("skill4", "4.0.0", &existing_skills).is_ok());
        assert!(validate_uniqueness("skill", "0.0.1", &existing_skills).is_err());
    }
}

//! Unit tests for MetadataSection validation

use fastskill::core::validation::{validate_identifier, validate_semver};

#[test]
fn test_validate_semver_valid() {
    assert!(validate_semver("1.0.0").is_ok());
    assert!(validate_semver("0.1.0").is_ok());
    assert!(validate_semver("10.20.30").is_ok());
}

#[test]
fn test_validate_semver_invalid() {
    assert!(validate_semver("1.0").is_err());
    assert!(validate_semver("1").is_err());
    assert!(validate_semver("v1.0.0").is_err());
    assert!(validate_semver("1.0.0-beta").is_err());
    assert!(validate_semver("invalid").is_err());
}

#[test]
fn test_validate_identifier_valid() {
    assert!(validate_identifier("my-skill").is_ok());
    assert!(validate_identifier("my_skill").is_ok());
    assert!(validate_identifier("skill123").is_ok());
    assert!(validate_identifier("skill-123_test").is_ok());
}

#[test]
fn test_validate_identifier_invalid() {
    assert!(validate_identifier("").is_err());
    assert!(validate_identifier("my skill").is_err()); // space
    assert!(validate_identifier("my.skill").is_err()); // dot
    assert!(validate_identifier("my@skill").is_err()); // @ symbol
}

#[test]
fn test_validate_project_structure() {
    use fastskill::core::validation::validate_project_structure;

    // Valid: has metadata
    assert!(validate_project_structure(true, false).is_ok());

    // Valid: has dependencies
    assert!(validate_project_structure(false, true).is_ok());

    // Valid: has both
    assert!(validate_project_structure(true, true).is_ok());

    // Invalid: has neither
    assert!(validate_project_structure(false, false).is_err());
}

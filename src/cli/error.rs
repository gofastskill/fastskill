//! CLI-specific error types

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid skill source: {0}")]
    InvalidSource(String),

    #[error("Git clone failed: {0}")]
    GitCloneFailed(String),

    #[error("Skill validation failed: {0}")]
    SkillValidationFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Service error: {0}")]
    Service(#[from] fastskill::ServiceError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Sources error: {0}")]
    #[allow(dead_code)] // May be used in future
    Sources(String),

    #[error("skill-project.toml validation error: {0}")]
    ProjectTomlValidation(String),

    #[error("Invalid semantic version: {0}\n  Expected format: MAJOR.MINOR.PATCH (e.g., 1.0.0)\n  Example valid versions: 1.0.0, 0.1.0, 2.3.4")]
    InvalidSemver(String),

    #[error("Invalid identifier: {0}\n  Identifiers must contain only alphanumeric characters, hyphens (-), and underscores (_)\n  Example valid identifiers: my-skill, skill_123, testSkill")]
    InvalidIdentifier(String),

    #[error("Duplicate skill: {0}@{1} already exists\n  Each skill must have a unique combination of name and version\n  Consider bumping the version or using a different name")]
    #[allow(dead_code)]
    DuplicateSkill(String, String),
}

pub type CliResult<T> = Result<T, CliError>;

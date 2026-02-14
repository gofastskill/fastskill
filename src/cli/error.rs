//! CLI-specific error types
//!
//! This module provides comprehensive error handling for the FastSkill CLI.
//! All errors are designed to provide clear, actionable messages to users.
//!
//! ## Error Message Guidelines
//!
//! ### TOML Parsing Errors (T066)
//! - Include line numbers when available from TOML parser
//! - Format: "TOML syntax error at line {line}: {message}"
//! - Example: "TOML syntax error at line 5: expected `]` but found `}`"
//!
//! ### Validation Errors (T067)
//! - Include context information (project-level vs skill-level)
//! - Specify which section is missing or invalid
//! - Provide guidance on what's required
//! - Format: "Missing required section: {section} (context: {context:?})"
//! - Example: "Missing required section: dependencies (context: Project)"
//!
//! ### Missing File Errors
//! - Suggest how to create the file or what command to run
//! - Format: "{file} not found. {suggestion}"
//! - Example: "skill-project.toml not found. Create it or use 'fastskill add' to add skills."
//!
//! ### Context-Specific Errors
//! - Skill-level context: Require [metadata] with id and version
//! - Project-level context: Require [dependencies] section
//! - Ambiguous context: Use content-based detection (metadata.id vs dependencies)
//!
//! ## Error Types
//!
//! - `CliError`: Main error type for CLI operations
//! - `ValidationError`: TOML validation and context-specific errors
//! - All errors implement `std::error::Error` via `thiserror`

use std::path::PathBuf;

use thiserror::Error;

// Import ProjectContext from manifest module
use fastskill::core::manifest::ProjectContext;

/// Rich "skill not found" message with searched locations and Try suggestions.
/// Rendered as a multi-line block by Display.
#[derive(Debug)]
pub struct SkillNotFoundMessage {
    pub skill_id: String,
    pub searched_paths: Vec<(PathBuf, String)>,
    pub suggestions: Vec<String>,
}

impl std::fmt::Display for SkillNotFoundMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Skill '{}' not found", self.skill_id)?;
        writeln!(f)?;
        writeln!(f, "Searched locations:")?;
        for (path, label) in &self.searched_paths {
            writeln!(f, "  \u{2713} {} ({})", path.display(), label)?;
        }
        writeln!(f)?;
        writeln!(f, "Try:")?;
        for line in &self.suggestions {
            writeln!(f, "  {}", line)?;
        }
        Ok(())
    }
}

impl std::error::Error for SkillNotFoundMessage {}

impl SkillNotFoundMessage {
    /// Build a skill-not-found message with searched paths and standard Try suggestions.
    pub fn new(skill_id: impl Into<String>, searched_paths: Vec<(PathBuf, String)>) -> Self {
        Self {
            skill_id: skill_id.into(),
            searched_paths,
            suggestions: skill_not_found_try_suggestions(),
        }
    }
}

/// Standard "Try" lines shown when a skill is not found.
pub fn skill_not_found_try_suggestions() -> Vec<String> {
    vec![
        "fastskill install owner/repo".to_string(),
        "fastskill list                    # See available skills".to_string(),
    ]
}

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

    #[error(transparent)]
    SkillNotFound(#[from] SkillNotFoundMessage),

    #[error("Symlink creation failed on Windows: {0}\n  Administrator privileges or Developer Mode is required to create symlinks\n  To install without editable mode (as a copy), run the same command without -e")]
    #[allow(dead_code)] // Only used on Windows
    WindowsSymlinkPermission(String),

    #[error("Editable install (-e) is not supported on this platform")]
    #[allow(dead_code)] // Only used on non-Unix, non-Windows platforms
    EditableNotSupported,
}

/// Validation error for TOML validation failures
#[derive(Debug, Error)]
#[allow(dead_code)] // Will be used in Phase 3+ implementation
pub enum ValidationError {
    #[error("TOML syntax error at line {line}: {message}")]
    SyntaxError { line: usize, message: String },

    #[error("Missing required section: {section} (context: {context:?})")]
    MissingSection {
        section: String,
        context: ProjectContext,
    },

    #[error("Invalid field {field} in section {section}: {message}")]
    InvalidField {
        section: String,
        field: String,
        message: String,
    },

    #[error("Context detection failed: {message}")]
    ContextDetectionFailed { message: String },
}

impl ValidationError {
    /// Format TOML parse error with line number extraction
    /// T020: Implement validation error formatting with line numbers
    #[allow(dead_code)] // Will be used in Phase 3+ implementation
    pub fn from_toml_error(error: &toml::de::Error) -> Self {
        // Extract line number from TOML error message
        // TOML errors typically include line information in the format: "line X, column Y"
        let error_msg = error.to_string();
        let line = extract_line_number(&error_msg).unwrap_or(0);

        ValidationError::SyntaxError {
            line,
            message: error_msg,
        }
    }
}

/// Extract line number from TOML error message
/// TOML errors typically include "line X" or "at line X" in the message
#[allow(dead_code)] // Will be used in Phase 3+ implementation
fn extract_line_number(error_msg: &str) -> Option<usize> {
    // Try to find "line X" pattern
    if let Some(line_start) = error_msg.find("line ") {
        let after_line = &error_msg[line_start + 5..];
        let line_end = after_line
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_line.len());
        if let Ok(line) = after_line[..line_end].parse::<usize>() {
            return Some(line);
        }
    }

    // Try to find "at line X" pattern
    if let Some(line_start) = error_msg.find("at line ") {
        let after_line = &error_msg[line_start + 8..];
        let line_end = after_line
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_line.len());
        if let Ok(line) = after_line[..line_end].parse::<usize>() {
            return Some(line);
        }
    }

    None
}

pub type CliResult<T> = Result<T, CliError>;

/// Canonical error message when skill-project.toml is not found in the hierarchy.
/// Used by install, list, update, and add when `!project_file_result.found`.
pub fn manifest_required_message() -> &'static str {
    "skill-project.toml not found in this directory or any parent. \
     Create it at the top level of your workspace (e.g. run 'fastskill init' there), \
     then run this command again."
}

/// Message for add command when manifest is missing (includes add-specific hint).
pub fn manifest_required_for_add_message() -> &'static str {
    "skill-project.toml not found in this directory or any parent. \
     Create it at the top level of your workspace (e.g. run 'fastskill init' there), \
     then run 'fastskill add <skill>' from your project."
}

#[derive(Debug, Clone)]
pub enum CliWarning {
    MissingSkillProjectToml { path: PathBuf, fallback_id: String },
}

impl CliWarning {
    pub fn display(&self, source_type: &str, editable: bool) {
        match self {
            CliWarning::MissingSkillProjectToml { path, fallback_id } => {
                eprintln!(
                    "⚠  Warning: skill-project.toml not found at {}",
                    path.display()
                );
                eprintln!(
                    "   Using skill ID '{}' from SKILL.md frontmatter",
                    fallback_id
                );

                if source_type == "local" && editable {
                    eprintln!("   Consider running 'fastskill init' in the skill directory to add skill-project.toml");
                } else {
                    eprintln!(
                        "   This is normal for standard-compliant skills from external sources"
                    );
                }
            }
        }
    }
}

impl CliError {
    /// Get the exit code for this error
    /// Returns: 0 = success, 1 = not found/invalid, 2 = system error
    pub fn exit_code(&self) -> i32 {
        match self {
            // Validation errors (not found, invalid format) -> exit code 1
            CliError::Validation(_) => 1,
            // Skill not found -> exit code 1
            CliError::SkillNotFound(_) => 1,
            // System errors (IO, config, service) -> exit code 2
            CliError::Io(_) | CliError::Config(_) | CliError::Service(_) => 2,
            // Windows symlink permission error -> exit code 2
            CliError::WindowsSymlinkPermission(_) => 2,
            // Editable not supported -> exit code 2
            CliError::EditableNotSupported => 2,
            // Other errors default to 1
            _ => 1,
        }
    }
}

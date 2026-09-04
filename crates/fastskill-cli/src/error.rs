//! CLI-specific error types
//!
//! This module provides comprehensive error handling for the FastSkill CLI.
//! All errors are designed to provide clear, actionable messages to users.
//!
//! ## Error Message Guidelines
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
//! - All errors implement `std::error::Error` via `thiserror`

use std::path::PathBuf;

use thiserror::Error;

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
    Service(#[from] fastskill_core::ServiceError),

    #[error("Search error: {0}")]
    Search(#[from] fastskill_core::SearchError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("skill-project.toml validation error: {0}")]
    ProjectTomlValidation(String),

    #[error("Invalid depth value: {0}")]
    InvalidDepth(String),

    #[error("{0}")]
    SkillNotFound(SkillNotFoundMessage),

    #[error("Invalid semantic version: {0}")]
    InvalidSemver(String),

    #[error("Invalid identifier: {0}")]
    InvalidIdentifier(String),

    /// A command whose entire output is derived from the vector index was run
    /// with no Embedding provider configured.
    ///
    /// CONTEXT.md ("Vector index"): *"every consumer (`search --local`,
    /// `analyze`) inherits the same provider precondition"*. Inheriting a
    /// precondition means an **unmet** one is an error. `reindex` is the single
    /// command CONTEXT.md sanctions *skipping* — it has nothing to report when
    /// there is nothing to index — and `search --local` degrades to a real
    /// keyword path. A command with no non-semantic path to fall back to can
    /// either grow one or refuse; printing nothing and exiting 0 is neither,
    /// because a caller reads that as "analysed, found nothing".
    ///
    /// The field is the command name, so the message names what the user ran.
    #[error("{0} requires an embedding provider. Run 'fastskill doctor' for setup guidance.")]
    MissingEmbeddingProvider(&'static str),
}

pub type CliResult<T> = Result<T, CliError>;

/// Canonical error message when skill-project.toml is not found in the hierarchy.
/// Used by install, list, update, and add when `!project_file_result.found`.
pub fn manifest_required_message() -> &'static str {
    "skill-project.toml not found in this directory or any parent. \
     Create it at the top level of your workspace (e.g. run 'fastskill init' there), \
     then run this command again."
}

/// The opening words every "there is no project here" message shares.
const MANIFEST_MISSING_PREFIX: &str = "skill-project.toml not found";

/// Whether this error is "there is no project manifest here".
///
/// Several places raise it — [`manifest_required_message`] and
/// `fastskill_core::core::load_project_config` among them — so the test below
/// pins the known producers rather than trusting one call site. Callers use
/// this to add context a generic handler cannot: `main` turns a bare-word
/// shorthand that died here into a message naming the word and `fastskill init`.
pub fn is_manifest_missing(error: &CliError) -> bool {
    matches!(error, CliError::Config(message) if message.starts_with(MANIFEST_MISSING_PREFIX))
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
    #[allow(dead_code)]
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
            // Search errors: map underlying service failures to system error code
            CliError::Search(fastskill_core::SearchError::Service(_)) => 2,
            CliError::Search(_) => 1,
            // Other errors default to 1
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_manifest_missing, manifest_required_message, CliError};

    #[test]
    fn search_service_error_uses_system_exit_code() {
        let error = CliError::Search(fastskill_core::SearchError::Service(
            fastskill_core::ServiceError::Custom("boom".to_string()),
        ));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn search_validation_error_uses_user_error_exit_code() {
        let error = CliError::Search(fastskill_core::SearchError::Validation(
            "invalid".to_string(),
        ));
        assert_eq!(error.exit_code(), 1);
    }

    #[test]
    fn manifest_missing_is_recognised_from_the_canonical_message() {
        assert!(is_manifest_missing(&CliError::Config(
            manifest_required_message().to_string()
        )));
    }

    #[test]
    fn manifest_missing_is_recognised_from_the_core_loader() {
        // The loader `resolve_skills_storage_directory` calls owns its own copy
        // of the wording. If that copy drifts out of the prefix, the shorthand
        // hint in `main` goes silently missing -- fail here instead.
        let empty = tempfile::TempDir::new().unwrap();
        let error = fastskill_core::core::load_project_config(empty.path())
            .map(|_| ())
            .expect_err("a directory with no skill-project.toml must not resolve");
        assert!(
            is_manifest_missing(&CliError::Config(error.clone())),
            "core's loader no longer produces a recognisable manifest-missing error: {}",
            error
        );
    }

    #[test]
    fn other_config_errors_are_not_manifest_missing() {
        assert!(!is_manifest_missing(&CliError::Config(
            "Skills directory does not exist: /nope".to_string()
        )));
    }
}

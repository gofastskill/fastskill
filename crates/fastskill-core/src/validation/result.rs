//! Shared validation result types for skill validation.

use serde::{Deserialize, Serialize};

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether validation passed
    pub is_valid: bool,

    /// List of validation errors
    pub errors: Vec<ValidationError>,

    /// List of validation warnings
    pub warnings: Vec<ValidationWarning>,

    /// Validation score (0.0 to 1.0)
    pub score: f64,
}

impl ValidationResult {
    /// Create a valid result
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            score: 1.0,
        }
    }

    /// Create an invalid result
    pub fn invalid(error: &str) -> Self {
        Self {
            is_valid: false,
            errors: vec![ValidationError {
                field: "general".to_string(),
                message: error.to_string(),
                severity: ErrorSeverity::Error,
            }],
            warnings: Vec::new(),
            score: 0.0,
        }
    }

    /// Add an error
    pub fn with_error(mut self, field: &str, message: &str, severity: ErrorSeverity) -> Self {
        self.errors.push(ValidationError {
            field: field.to_string(),
            message: message.to_string(),
            severity,
        });
        self.is_valid = false;
        self.score = 0.0;
        self
    }

    /// Add a warning
    pub fn with_warning(mut self, field: &str, message: &str) -> Self {
        self.warnings.push(ValidationWarning {
            field: field.to_string(),
            message: message.to_string(),
        });
        self
    }

    /// Calculate quality score based on errors and warnings
    pub(crate) fn calculate_score(&mut self) {
        let error_penalty = self.errors.len() as f64 * 0.3;
        let warning_penalty = self.warnings.len() as f64 * 0.1;

        self.score = (1.0 - error_penalty - warning_penalty).max(0.0);
    }
}

/// Validation error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Field or component that failed validation
    pub field: String,

    /// Error message
    pub message: String,

    /// Error severity level
    pub severity: ErrorSeverity,
}

/// Error severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Warning - not critical but should be addressed
    Warning,
    /// Error - validation failed but skill might still work
    Error,
    /// Critical - validation failed and skill cannot be used
    Critical,
}

/// Validation warning details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    /// Field or component with warning
    pub field: String,

    /// Warning message
    pub message: String,
}

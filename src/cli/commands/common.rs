//! Common utilities for CLI commands

use crate::cli::error::{CliError, CliResult};
use fastskill::OutputFormat;

/// Emit a standardized deprecation warning with migration guidance.
pub fn emit_deprecation_warning(
    legacy_command: &str,
    new_command: &str,
    operation_type: &str,
    mappings: &[(&str, &str)],
) {
    eprintln!(
        "⚠️  Warning: The '{}' command is deprecated and will be removed in a future version.",
        legacy_command
    );
    eprintln!(
        "   Please use 'fastskill {}' for {} instead:",
        new_command, operation_type
    );
    for (legacy_sub, new_path) in mappings {
        eprintln!("   - '{}' → '{}'", legacy_sub, new_path);
    }
    eprintln!();
}

/// Validate format arguments and return resolved format
pub fn validate_format_args(format: &Option<OutputFormat>, json: bool) -> CliResult<OutputFormat> {
    match (format, json) {
        (Some(_), true) => Err(CliError::Config(
            "Error: --json and --format cannot be used together. Use one output selector."
                .to_string(),
        )),
        (Some(f), false) => Ok(f.clone()),
        (None, true) => Ok(OutputFormat::Json),
        (None, false) => Ok(OutputFormat::Table), // Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_deprecation_warning_no_panic() {
        let mappings = vec![("list", "repos list")];
        emit_deprecation_warning("sources", "repos", "repository management", &mappings);
    }
}

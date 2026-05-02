//! Common utilities for CLI commands

use crate::error::{CliError, CliResult};
use fastskill_agent_runtime::RuntimeSelectionError;
use fastskill_core::OutputFormat;

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

/// Map a `RuntimeSelectionError` to a `CliError` with canonical `RUNTIME_*` prefix codes.
pub fn runtime_selection_error_to_cli(e: RuntimeSelectionError) -> CliError {
    match e {
        RuntimeSelectionError::ConflictingFlags => CliError::Config(
            "RUNTIME_CONFLICTING_FLAGS: --agent and --all are mutually exclusive; \
             use one or the other"
                .to_string(),
        ),
        RuntimeSelectionError::UnknownRuntimeIds { unknown, available } => {
            CliError::Config(format!(
                "RUNTIME_UNKNOWN_ID: Unknown runtime ID(s): {}. Available: {}",
                unknown.join(", "),
                available.join(", ")
            ))
        }
        RuntimeSelectionError::EmptyRuntimeSet { hint } => CliError::Config(format!(
            "RUNTIME_EMPTY_SET: --all resolved to no runtimes. {}",
            hint
        )),
    }
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

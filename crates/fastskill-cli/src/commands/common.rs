//! Common utilities for CLI commands

use crate::error::{CliError, CliResult};
use crate::runtime_selector::RuntimeSelectionError;
use fastskill_core::OutputFormat;

/// Emit a standardized deprecation warning with migration guidance.
#[allow(dead_code)]
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

/// Validate format arguments for the eval commands.
///
/// Eval output is only implemented for `table` and `json`. The shared
/// `grid`/`xml` formatters in `output/mod.rs` take domain-specific row types
/// and do not accept eval summaries, so those choices are rejected here with a
/// clear error rather than silently collapsing to `table`.
pub fn validate_eval_format_args(
    format: &Option<OutputFormat>,
    json: bool,
) -> CliResult<OutputFormat> {
    let resolved = validate_format_args(format, json)?;
    match resolved {
        OutputFormat::Grid | OutputFormat::Xml => Err(CliError::Config(
            "Error: eval commands support only --format table or json (grid/xml are not \
             implemented for eval output). Use --format json for machine-readable output."
                .to_string(),
        )),
        other => Ok(other),
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

    #[test]
    fn test_validate_eval_format_accepts_table_and_json() {
        assert_eq!(
            validate_eval_format_args(&Some(OutputFormat::Table), false).unwrap(),
            OutputFormat::Table
        );
        assert_eq!(
            validate_eval_format_args(&Some(OutputFormat::Json), false).unwrap(),
            OutputFormat::Json
        );
        // --json shorthand
        assert_eq!(
            validate_eval_format_args(&None, true).unwrap(),
            OutputFormat::Json
        );
        // default
        assert_eq!(
            validate_eval_format_args(&None, false).unwrap(),
            OutputFormat::Table
        );
    }

    #[test]
    fn test_validate_eval_format_rejects_grid() {
        let err = validate_eval_format_args(&Some(OutputFormat::Grid), false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("table or json"), "unexpected message: {msg}");
    }

    #[test]
    fn test_validate_eval_format_rejects_xml() {
        let err = validate_eval_format_args(&Some(OutputFormat::Xml), false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("table or json"), "unexpected message: {msg}");
    }

    #[test]
    fn test_validate_eval_format_still_rejects_conflicting_selectors() {
        // --json + --format together is an error regardless of eval gating.
        assert!(validate_eval_format_args(&Some(OutputFormat::Table), true).is_err());
    }
}

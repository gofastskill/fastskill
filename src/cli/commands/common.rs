//! Common utilities for CLI commands

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_deprecation_warning_no_panic() {
        let mappings = vec![("list", "repos list")];
        emit_deprecation_warning("sources", "repos", "repository management", &mappings);
    }
}

//! Process-level JSON output handling for command errors.

/// Whether an invocation requests JSON through a shorthand or format option.
pub(crate) fn requests_json_output(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--format" && pair[1] == "json")
}

/// Lifecycle JSON has a shared result envelope, including errors raised before
/// a command-specific renderer runs.
pub(crate) fn is_json_lifecycle(args: &[String]) -> bool {
    if !args.iter().any(|arg| arg == "--json") {
        return false;
    }
    let mut positionals = Vec::new();
    let mut skip_value = false;
    for arg in args.iter().skip(1) {
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg == "--skills-dir" {
            skip_value = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        positionals.push(arg.as_str());
    }
    matches!(
        positionals.as_slice(),
        ["skill", "add", ..]
            | ["skill", "update", ..]
            | ["skill", "remove", ..]
            | ["project", "install", ..]
            | ["bundle", "add", ..]
            | ["bundle", "update", ..]
            | ["bundle", "remove", ..]
            | ["bundle", "override", ..]
    )
}

pub(crate) fn emit_lifecycle_json_error(error: &anyhow::Error, global: bool, dry_run: bool) {
    crate::output::emit(
        &serde_json::json!({
            "scope": if global { "global" } else { "project" },
            "outcome": "failed",
            "dry_run": dry_run,
            "targets": [],
            "changes": [],
            "retained": [],
            "diagnostics": [error.to_string()]
        })
        .to_string(),
    );
}

pub(crate) fn emit_json_error(error: &anyhow::Error) {
    crate::output::emit(
        &serde_json::json!({
            "success": false,
            "error": { "message": error.to_string() }
        })
        .to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn lifecycle_detection_handles_scopes_nested_paths_and_other_commands() {
        assert!(is_json_lifecycle(&args(&[
            "fastskill",
            "--global",
            "skill",
            "update",
            "--json",
        ])));
        assert!(is_json_lifecycle(&args(&[
            "fastskill",
            "bundle",
            "override",
            "member",
            "--json",
        ])));
        assert!(is_json_lifecycle(&args(&[
            "fastskill",
            "--skills-dir",
            "/tmp/skills",
            "skill",
            "add",
            "--json",
        ])));
        assert!(!is_json_lifecycle(&args(&[
            "fastskill",
            "skill",
            "list",
            "--json",
        ])));
        assert!(!is_json_lifecycle(&args(&["fastskill", "--json"])));
    }

    #[test]
    fn output_detection_accepts_flag_and_format_forms() {
        assert!(requests_json_output(&args(&[
            "fastskill",
            "skill",
            "list",
            "--json",
        ])));
        assert!(requests_json_output(&args(&[
            "fastskill",
            "skill",
            "list",
            "--format",
            "json",
        ])));
        assert!(requests_json_output(&args(&[
            "fastskill",
            "skill",
            "list",
            "--format=json",
        ])));
        assert!(!requests_json_output(&args(&[
            "fastskill",
            "skill",
            "list",
            "--format",
            "table",
        ])));
    }

    #[tokio::test]
    async fn both_error_envelopes_are_single_parseable_values() {
        let (_, generic) = crate::output::capture(async {
            emit_json_error(&anyhow::anyhow!("broken input"));
        })
        .await;
        let value: serde_json::Value = serde_json::from_str(generic.trim()).unwrap();
        assert_eq!(value["success"], false);
        assert_eq!(value["error"]["message"], "broken input");

        let (_, lifecycle) = crate::output::capture(async {
            emit_lifecycle_json_error(&anyhow::anyhow!("blocked"), true, true);
        })
        .await;
        let value: serde_json::Value = serde_json::from_str(lifecycle.trim()).unwrap();
        assert_eq!(value["scope"], "global");
        assert_eq!(value["outcome"], "failed");
        assert_eq!(value["dry_run"], true);
    }
}

//! Guards the check-behaviour claims made in `webdocs/evals-quality/setup.mdx`
//! and `webdocs/optimize/setup.mdx`.
//!
//! These assertions are about the *pinned* `aikit-evals` rev as re-exported by
//! this shim. If a pin bump changes them, the docs are wrong and must be
//! updated in the same PR — that is the point of this test.

use fastskill_evals::{run_checks, ChecksToml};
use std::path::Path;

/// `max_command_count` is a legacy alias; results report the canonical
/// `max_tool_calls`, and the count covers `tool_use` + `raw_json` events while
/// excluding assistant text.
#[test]
fn max_tool_calls_alias_name_and_counting_match_the_docs() {
    // 1. `max_command_count` parses (legacy alias) ...
    let toml = r#"
[[check]]
name = "max_command_count"
limit = 1
"#;
    let parsed: ChecksToml = toml::from_str(toml).expect("alias must parse");

    // ... and a trace of ONE tool_use + ONE raw_json = 2 counted, exceeding limit 1.
    let trace = [
        r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"Bash","input":{}}}"#,
        r#"{"seq":1,"payload":{"type":"raw_json","data":{"x":1}}}"#,
        r#"{"seq":2,"payload":{"type":"message","role":"assistant","text":"hello"}}"#,
    ]
    .join("\n");

    let results = run_checks(&parsed.checks, "", &trace, Path::new("/tmp"));
    // 2. results report the CANONICAL name, not the alias
    assert_eq!(results[0].check_name, "max_tool_calls");
    // 3. tool_use AND raw_json both counted (2 > 1) => fails; message states the count
    assert!(!results[0].passed, "2 counted events must exceed limit 1");
    assert!(results[0].message.as_deref().unwrap().contains("2"));
}

//! Guards the check-behaviour claims made in `webdocs/evals-quality/setup.mdx`
//! and `webdocs/optimize/setup.mdx`.
//!
//! These assertions are about the *pinned* `aikit-evals` rev as re-exported by
//! this shim. If a pin bump changes them, the docs are wrong and must be
//! updated in the same PR — that is the point of this test.

use fastskill_evals::{run_checks, suite_passes, ChecksToml};
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

/// The headline claim of the aikit#153 bump: pattern checks read the canonical
/// trace and **never** raw stdout. This is what makes a bare skill-name
/// `trigger_expectation` stop passing vacuously — with the `claude` agent, raw
/// stdout opens with a `system`/`init` event naming every installed skill.
#[test]
fn pattern_checks_ignore_stdout_and_read_only_the_trace() {
    let toml = r#"
[[check]]
name = "trigger_expectation"
pattern = "my-skill"
expected = true

[[check]]
name = "command_contains"
pattern = "my-skill"
"#;
    let parsed: ChecksToml = toml::from_str(toml).expect("checks must parse");

    // Exactly the shape that used to produce a false pass: the skill name is
    // present in the agent's init listing on stdout, and absent from the trace
    // because the skill never ran.
    let stdout =
        r#"{"type":"system","subtype":"init","skills":["my-skill"],"slash_commands":["my-skill"]}"#;
    let trace = r#"{"seq":0,"payload":{"type":"message","role":"assistant","text":"4"}}"#;

    let results = run_checks(&parsed.checks, stdout, trace, Path::new("/tmp"));
    assert!(
        !results[0].passed,
        "trigger_expectation must NOT be satisfied by a stdout-only match"
    );
    assert!(
        !results[1].passed,
        "command_contains must NOT be satisfied by a stdout-only match"
    );

    // ... and the mirror: "must not trigger" now passes when the skill is absent.
    let negative: ChecksToml = toml::from_str(
        r#"
[[check]]
name = "trigger_expectation"
pattern = "my-skill"
expected = false
"#,
    )
    .unwrap();
    let results = run_checks(&negative.checks, stdout, trace, Path::new("/tmp"));
    assert!(
        results[0].passed,
        "expected=false must pass when the skill is absent from the trace"
    );
}

/// `skill_invoked` matches a *structured* `Skill` tool invocation, not a
/// substring, and `skill` is optional.
#[test]
fn skill_invoked_matches_structured_skill_tool_use() {
    let toml = r#"
[[check]]
name = "skill_invoked"
skill = "my-skill"
"#;
    let parsed: ChecksToml = toml::from_str(toml).expect("skill_invoked must parse");

    // The named form exact-matches the invocation's skill-identifying field
    // (`skill`, `name`, or `skillName` — the shapes claude's Skill tool_use
    // events carry), not a substring of the serialized input.
    let fired = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"Skill","input":{"skill":"my-skill"}}}"#;
    assert!(run_checks(&parsed.checks, "", fired, Path::new("/tmp"))[0].passed);

    // A different tool is not a skill invocation, even though the trace text
    // contains the skill name — this is what separates it from a substring match.
    let other_tool = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"Bash","input":{"command":"echo my-skill"}}}"#;
    assert!(
        !run_checks(&parsed.checks, "", other_tool, Path::new("/tmp"))[0].passed,
        "a Bash call mentioning the skill name must not count as an invocation"
    );

    // A Skill invocation whose input carries the name only in a non-identifying
    // field must not satisfy the NAMED form: exact-field matching is what stops
    // "foo" from matching "foo-bar" or argument text.
    let non_identifying = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"Skill","input":{"args":"use my-skill please"}}}"#;
    assert!(
        !run_checks(&parsed.checks, "", non_identifying, Path::new("/tmp"))[0].passed,
        "the named form must read identifying fields only, not the whole input"
    );

    // `skill` omitted => any Skill invocation matches, whatever its input shape.
    let any: ChecksToml = toml::from_str("[[check]]\nname = \"skill_invoked\"\n").unwrap();
    let someone_else = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"Skill","input":{"command":"unrelated"}}}"#;
    assert!(run_checks(&any.checks, "", someone_else, Path::new("/tmp"))[0].passed);
}

/// `required = false` is advisory: the check still runs and is still reported,
/// but its failure does not fail the case.
#[test]
fn required_false_is_advisory_but_still_reported() {
    let toml = r#"
[[check]]
name = "command_contains"
pattern = "definitely-absent"
required = false

[[check]]
name = "command_contains"
pattern = "present"
required = true
"#;
    let parsed: ChecksToml = toml::from_str(toml).expect("checks must parse");
    let trace = r#"{"seq":0,"payload":{"type":"message","role":"assistant","text":"present"}}"#;

    let results = run_checks(&parsed.checks, "", trace, Path::new("/tmp"));
    assert!(!results[0].passed, "the optional check genuinely failed");
    assert!(!results[0].required, "and is reported as not required");
    assert!(results[1].passed);
    assert!(
        suite_passes(&results),
        "a failing OPTIONAL check must not fail the suite"
    );

    // Negative direction: flip it to required and the same failure now fails the suite.
    let strict: ChecksToml = toml::from_str(
        r#"
[[check]]
name = "command_contains"
pattern = "definitely-absent"
required = true
"#,
    )
    .unwrap();
    assert!(!suite_passes(&run_checks(
        &strict.checks,
        "",
        trace,
        Path::new("/tmp")
    )));
}

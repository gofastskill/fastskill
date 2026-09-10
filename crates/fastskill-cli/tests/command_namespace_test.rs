#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::process::{Command, Output};

const CANONICAL_PATHS: &[&str] = &[
    "analysis/cluster",
    "analysis/duplicates",
    "analysis/matrix",
    "bundle/add",
    "bundle/build",
    "bundle/list",
    "bundle/override",
    "bundle/remove",
    "bundle/update",
    "cache/clean",
    "cache/info",
    "cli/completion",
    "cli/doctor",
    "cli/spec",
    "eval/judge",
    "eval/report",
    "eval/run",
    "eval/score",
    "eval/scorecard",
    "eval/validate",
    "index/rebuild",
    "marketplace/create",
    "mcp/install",
    "mcp/list",
    "mcp/serve",
    "optimization/export",
    "optimization/inspect",
    "optimization/resume",
    "optimization/run",
    "optimization/status",
    "project/init",
    "project/install",
    "repo/add",
    "repo/info",
    "repo/list",
    "repo/refresh",
    "repo/remove",
    "repo/show",
    "repo/skills",
    "repo/test",
    "repo/update",
    "repo/versions",
    "server/serve",
    "skill/add",
    "skill/list",
    "skill/read",
    "skill/remove",
    "skill/search",
    "skill/update",
];

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run fastskill {}: {error}", args.join(" ")))
}

fn stdout(args: &[&str]) -> String {
    let output = run(args);
    assert!(
        output.status.success(),
        "fastskill {} failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn assert_in_order(text: &str, needles: &[&str]) {
    let mut cursor = 0;
    for needle in needles {
        let offset = text[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?} after byte {cursor}:\n{text}"));
        cursor += offset + needle.len();
    }
}

#[test]
fn spec_exports_the_exact_canonical_inventory() {
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&["cli", "spec", "--format", "json"])).unwrap();
    let actual = document["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|command| command["path"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    let expected = CANONICAL_PATHS.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 49);
}

#[test]
fn root_and_bundle_help_follow_the_accepted_order() {
    let root = stdout(&["--help"]);
    assert_eq!(root.matches("Usage:").count(), 1, "{root}");
    assert!(!root.contains("Other:"), "{root}");
    assert_in_order(
        &root,
        &[
            "Skills and projects:",
            "  skill",
            "  bundle",
            "  project",
            "Sources and distribution:",
            "  repo",
            "  marketplace",
            "Quality:",
            "  analysis",
            "  eval",
            "  optimization",
            "Operations:",
            "  index",
            "  cache",
            "  server",
            "  mcp",
            "  cli",
        ],
    );

    let bundle = stdout(&["bundle", "--help"]);
    assert_in_order(
        &bundle,
        &[
            "  build",
            "  add",
            "  list",
            "  update",
            "  remove",
            "  override",
        ],
    );
}

#[test]
fn retired_roots_plural_repo_and_skill_shorthand_fail_without_mutation() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("sentinel"), "unchanged").unwrap();
    let removed = [
        vec!["init"],
        vec!["install"],
        vec!["add", "source"],
        vec!["remove", "demo"],
        vec!["update"],
        vec!["list"],
        vec!["read", "demo"],
        vec!["search", "demo"],
        vec!["repos", "list"],
        vec!["analyze", "matrix"],
        vec!["optimize", "status", "run"],
        vec!["reindex"],
        vec!["serve"],
        vec!["doctor"],
        vec!["completion", "bash"],
        vec!["spec", "--format", "json"],
        vec!["mcp", "register"],
        vec!["demo"],
    ];

    for args in removed {
        let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
            .args(&args)
            .current_dir(project.path())
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "retired invocation unexpectedly succeeded: fastskill {}",
            args.join(" ")
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unknown")
                || stderr.contains("unrecognized")
                || stderr.contains("not found"),
            "retired invocation did not fail as unknown: fastskill {}\n{stderr}",
            args.join(" ")
        );
        assert_eq!(
            std::fs::read_to_string(project.path().join("sentinel")).unwrap(),
            "unchanged"
        );
        assert!(!project.path().join("skill-project.toml").exists());
    }
}

#[test]
fn skill_commands_reject_removed_bundle_selectors() {
    for args in [
        &["skill", "list", "--bundles"][..],
        &["skill", "update", "--bundle", "team", "--from", "team.zip"][..],
        &["skill", "remove", "--bundle", "team"][..],
    ] {
        let output = run(args);
        assert!(!output.status.success(), "fastskill {}", args.join(" "));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unknown argument"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

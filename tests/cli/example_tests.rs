//! Tests that each command's --help shows at least one runnable example.
//! Validates the examples added via clap after_help for all command forms.
//!
//! `disable_help_shows_examples` and `show_help_shows_examples` were removed:
//! `disable`/`show` are retired commands (main.rs explicitly excludes them
//! from the `read` shorthand as issue-#183 "cli-command-surface-redesign"
//! removals), so `fastskill disable --help` / `fastskill show --help` now
//! exit non-zero with "unrecognized subcommand" — there is no help text left
//! to assert on.
//!
//! The `sources`/`registry` top-level commands from the same redesign are
//! gone too (`fastskill sources ...` / `fastskill registry ...` now fail
//! with "unknown argument"); their subcommands were folded into `repos`
//! (`sources list/add/remove/show/update/test/refresh` -> `repos
//! list/add/remove/info/update/test/refresh`, `registry
//! list-skills/show-skill/versions` -> `repos skills/show/versions`), so the
//! per-subcommand example checks below were ported onto the `repos`
//! equivalents. `sources create` (marketplace-catalog generation) has no
//! `repos` equivalent — that functionality now lives under the unrelated
//! `marketplace create` command tree, which is out of scope for `repos`
//! coverage, so that test was dropped rather than ported.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::snapshot_helpers::run_fastskill_command;

fn help_succeeds_and_contains_examples(args: &[&str]) {
    let result = run_fastskill_command(args, None);
    assert!(
        result.success,
        "fastskill {} --help should succeed",
        args.join(" ")
    );

    // Require a real `Examples:` block, not merely the string "fastskill"
    // somewhere in the output. The previous form of this assertion was
    // `contains("Examples:") || contains("fastskill ")`, and the fallback
    // always matched the `Usage: fastskill ...` line -- so every test in this
    // file passed unconditionally, including for commands that had no
    // examples at all.
    let examples_body = result.stdout.split_once("Examples:").map(|(_, rest)| rest);
    let Some(examples_body) = examples_body else {
        panic!(
            "fastskill {} --help should contain an `Examples:` section; got:\n{}",
            args.join(" "),
            result.stdout
        );
    };

    // ...and at least one listed example must actually be an invocation.
    assert!(
        examples_body
            .lines()
            .any(|line| line.trim_start().starts_with("fastskill ")),
        "fastskill {} --help has an `Examples:` header but no `fastskill ...` \
         invocation under it; got:\n{}",
        args.join(" "),
        result.stdout
    );
}

/// Root `--help` is rendered by cli-framework's own `HelpRenderer`, not by
/// `build_typed_clap_command`, so it has no `Examples:` epilogue to assert on
/// (unlike every subcommand below). Assert what the root help is actually for:
/// that it succeeds and lists the command groups.
///
/// Rendering examples for the root command too would require a cli-framework
/// change to `HelpRenderer`, mirroring aroff/cli-framework#110 which added the
/// epilogue for subcommands.
#[test]
fn root_help_lists_command_groups() {
    let result = run_fastskill_command(&["--help"], None);
    assert!(result.success, "fastskill --help should succeed");
    for group in ["Discovery:", "Packages:", "Options:"] {
        assert!(
            result.stdout.contains(group),
            "root help should list the `{}` section; got:\n{}",
            group,
            result.stdout
        );
    }
}

#[test]
fn add_help_shows_examples() {
    help_succeeds_and_contains_examples(&["add", "--help"]);
}

#[test]
fn init_help_shows_examples() {
    help_succeeds_and_contains_examples(&["init", "--help"]);
}

#[test]
fn install_help_shows_examples() {
    help_succeeds_and_contains_examples(&["install", "--help"]);
}

#[test]
fn list_help_shows_examples() {
    help_succeeds_and_contains_examples(&["list", "--help"]);
}

#[test]
fn read_help_shows_examples() {
    help_succeeds_and_contains_examples(&["read", "--help"]);
}

/// `repos` is a command *group*, registered via `register_group(_, GroupMetadata)`.
/// `GroupMetadata` carries only `summary` and `hidden` -- it has no `examples`
/// field -- so group help cannot show an `Examples:` block the way leaf commands
/// do. Assert what group help is for: listing its subcommands. (The individual
/// `repos <sub> --help` tests below do assert on examples.)
#[test]
fn repos_help_lists_subcommands() {
    let result = run_fastskill_command(&["repos", "--help"], None);
    assert!(result.success, "fastskill repos --help should succeed");
    for sub in ["list", "add", "remove", "refresh"] {
        assert!(
            result.stdout.contains(sub),
            "repos help should list the `{}` subcommand; got:\n{}",
            sub,
            result.stdout
        );
    }
}

#[test]
fn repos_skills_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "skills", "--help"]);
}

#[test]
fn repos_show_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "show", "--help"]);
}

#[test]
fn repos_versions_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "versions", "--help"]);
}

#[test]
fn reindex_help_shows_examples() {
    help_succeeds_and_contains_examples(&["reindex", "--help"]);
}

#[test]
fn remove_help_shows_examples() {
    help_succeeds_and_contains_examples(&["remove", "--help"]);
}

#[test]
fn search_help_shows_examples() {
    help_succeeds_and_contains_examples(&["search", "--help"]);
}

#[test]
fn serve_help_shows_examples() {
    help_succeeds_and_contains_examples(&["serve", "--help"]);
}

#[test]
fn repos_list_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "list", "--help"]);
}

#[test]
fn repos_add_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "add", "--help"]);
}

#[test]
fn repos_remove_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "remove", "--help"]);
}

#[test]
fn repos_info_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "info", "--help"]);
}

#[test]
fn repos_update_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "update", "--help"]);
}

#[test]
fn repos_test_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "test", "--help"]);
}

#[test]
fn repos_refresh_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "refresh", "--help"]);
}

#[test]
fn update_help_shows_examples() {
    help_succeeds_and_contains_examples(&["update", "--help"]);
}

// `version_help_shows_examples` was removed: `version` is not a command.
// It is absent from `fastskill spec`, and `fastskill version --help` exits 0
// only because the unknown token falls through to the `read <skill-id>`
// shorthand -- so the test was asserting on `read`'s help while claiming to
// cover `version`, and passed for entirely the wrong reason. Printing the
// version is `--version`/`-V`, already covered by
// `help_tests::test_version_flag`. Same rationale as the `disable`/`show`
// removals noted at the top of this file.

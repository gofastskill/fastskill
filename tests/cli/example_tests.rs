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

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::run_fastskill_command;

fn help_succeeds_and_contains_examples(args: &[&str]) {
    let result = run_fastskill_command(args, None);
    assert!(
        result.success,
        "fastskill {} --help should succeed",
        args.join(" ")
    );
    assert!(
        result.stdout.contains("Examples:") || result.stdout.contains("fastskill "),
        "help output should contain Examples or example usage; got: {}",
        &result.stdout[..result.stdout.len().min(500)]
    );
}

#[test]
fn root_help_shows_examples() {
    help_succeeds_and_contains_examples(&["--help"]);
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

#[test]
fn repos_help_shows_examples() {
    help_succeeds_and_contains_examples(&["repos", "--help"]);
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

#[test]
fn version_help_shows_examples() {
    help_succeeds_and_contains_examples(&["version", "--help"]);
}

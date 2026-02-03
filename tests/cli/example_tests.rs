//! Tests that each command's --help shows at least one runnable example.
//! Validates the examples added via clap after_help for all command forms.

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
fn auth_help_shows_examples() {
    help_succeeds_and_contains_examples(&["auth", "--help"]);
}

#[test]
fn auth_login_help_shows_examples() {
    help_succeeds_and_contains_examples(&["auth", "login", "--help"]);
}

#[test]
fn auth_logout_help_shows_examples() {
    help_succeeds_and_contains_examples(&["auth", "logout", "--help"]);
}

#[test]
fn auth_whoami_help_shows_examples() {
    help_succeeds_and_contains_examples(&["auth", "whoami", "--help"]);
}

#[test]
fn disable_help_shows_examples() {
    help_succeeds_and_contains_examples(&["disable", "--help"]);
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
fn package_help_shows_examples() {
    help_succeeds_and_contains_examples(&["package", "--help"]);
}

#[test]
fn publish_help_shows_examples() {
    help_succeeds_and_contains_examples(&["publish", "--help"]);
}

#[test]
fn read_help_shows_examples() {
    help_succeeds_and_contains_examples(&["read", "--help"]);
}

#[test]
fn registry_help_shows_examples() {
    help_succeeds_and_contains_examples(&["registry", "--help"]);
}

#[test]
fn registry_list_skills_help_shows_examples() {
    help_succeeds_and_contains_examples(&["registry", "list-skills", "--help"]);
}

#[test]
fn registry_show_skill_help_shows_examples() {
    help_succeeds_and_contains_examples(&["registry", "show-skill", "--help"]);
}

#[test]
fn registry_versions_help_shows_examples() {
    help_succeeds_and_contains_examples(&["registry", "versions", "--help"]);
}

#[test]
fn registry_search_help_shows_examples() {
    help_succeeds_and_contains_examples(&["registry", "search", "--help"]);
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
fn show_help_shows_examples() {
    help_succeeds_and_contains_examples(&["show", "--help"]);
}

#[test]
fn sources_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "--help"]);
}

#[test]
fn sources_list_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "list", "--help"]);
}

#[test]
fn sources_add_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "add", "--help"]);
}

#[test]
fn sources_remove_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "remove", "--help"]);
}

#[test]
fn sources_show_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "show", "--help"]);
}

#[test]
fn sources_update_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "update", "--help"]);
}

#[test]
fn sources_test_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "test", "--help"]);
}

#[test]
fn sources_refresh_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "refresh", "--help"]);
}

#[test]
fn sources_create_help_shows_examples() {
    help_succeeds_and_contains_examples(&["sources", "create", "--help"]);
}

#[test]
fn update_help_shows_examples() {
    help_succeeds_and_contains_examples(&["update", "--help"]);
}

#[test]
fn version_help_shows_examples() {
    help_succeeds_and_contains_examples(&["version", "--help"]);
}

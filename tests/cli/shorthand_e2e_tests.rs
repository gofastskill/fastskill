//! `fastskill <skill-id>` shorthand: what the CLI says when it cannot proceed.
//!
//! `fastskill <word>` with no subcommand is shorthand for `fastskill read
//! <word>` (README "Reading a skill"). Reading needs an initialised project, so
//! before `fastskill init` the shorthand fails during config resolution — and
//! used to fail with the *generic* manifest error, identical to the one
//! `fastskill list` prints. A user who simply mistyped a command was told their
//! workspace was misconfigured and never told that the word was not a command
//! at all.
//!
//! These tests pin the three things that must stay true:
//!   1. outside a project, the shorthand's failure names the unknown word, says
//!      it was read as a skill ID, and points at `fastskill init`;
//!   2. a *known* command outside a project still gets the plain manifest error
//!      (the shorthand note must not leak onto commands that never used it);
//!   3. inside a project, a near-miss command name still gets the typo hint.

use super::snapshot_helpers::run_fastskill_command;
use std::fs;
use tempfile::TempDir;

fn init_project(dir: &std::path::Path) {
    fs::write(
        dir.join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join(".claude/skills")).unwrap();
}

#[test]
fn bare_word_outside_a_project_names_the_unknown_command_and_init() {
    let project = TempDir::new().unwrap();

    let result = run_fastskill_command(&["totallybogus"], Some(project.path()));

    assert!(
        !result.success,
        "an unknown bare word must not succeed: {}{}",
        result.stdout, result.stderr
    );

    let stderr = &result.stderr;
    assert!(
        stderr.contains("'totallybogus' is not a fastskill command"),
        "the failure must name the word that was not a command; got: {}",
        stderr
    );
    assert!(
        stderr.contains("skill ID"),
        "the failure must say the word was treated as a skill ID; got: {}",
        stderr
    );
    assert!(
        stderr.contains("fastskill init"),
        "the failure must point at `fastskill init`; got: {}",
        stderr
    );
}

#[test]
fn known_command_outside_a_project_keeps_the_plain_manifest_error() {
    let project = TempDir::new().unwrap();

    let result = run_fastskill_command(&["list"], Some(project.path()));

    assert!(
        !result.success,
        "`list` outside a project must fail: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result
            .stderr
            .contains("skill-project.toml not found in this directory or any parent"),
        "`list` must keep the plain manifest error; got: {}",
        result.stderr
    );
    assert!(
        !result.stderr.contains("is not a fastskill command"),
        "`list` is a command -- the shorthand note must not appear; got: {}",
        result.stderr
    );
}

#[test]
fn command_typo_inside_a_project_still_suggests_the_command() {
    let project = TempDir::new().unwrap();
    init_project(project.path());

    let result = run_fastskill_command(&["instal"], Some(project.path()));

    assert!(
        !result.success,
        "a mistyped command must fail: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result
            .stderr
            .contains("error: unrecognized subcommand 'instal'"),
        "the typo path must keep its own message; got: {}",
        result.stderr
    );
    assert!(
        result.stderr.contains("Did you mean 'install'?"),
        "the typo path must keep its suggestion; got: {}",
        result.stderr
    );
}

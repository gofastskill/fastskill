//! The retired `fastskill <skill-id>` shorthand must never dispatch a read.
//!
//! Skill reads are explicit through `fastskill skill read <skill-id>`. A bare
//! token is always an unknown command, regardless of project state or whether
//! a matching skill is installed.

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
fn bare_word_outside_a_project_is_an_unknown_command() {
    let project = TempDir::new().unwrap();

    let result = run_fastskill_command(&["totallybogus"], Some(project.path()));

    assert!(
        !result.success,
        "an unknown bare word must not succeed: {}{}",
        result.stdout, result.stderr
    );

    assert!(result.stderr.contains("unrecognized"), "{}", result.stderr);
    assert!(!result.stderr.contains("skill ID"), "{}", result.stderr);
    assert!(!project.path().join("skill-project.toml").exists());
}

#[test]
fn known_command_outside_a_project_keeps_the_plain_manifest_error() {
    let project = TempDir::new().unwrap();

    let result = run_fastskill_command(&["skill", "list"], Some(project.path()));

    assert!(
        !result.success,
        "`skill list` outside a project must fail: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result
            .stderr
            .contains("skill-project.toml not found in this directory or any parent"),
        "`skill list` must keep the plain manifest error; got: {}",
        result.stderr
    );
    assert!(
        !result.stderr.contains("is not a fastskill command"),
        "`skill list` is a command -- no unknown-command note should appear; got: {}",
        result.stderr
    );
}

#[test]
fn installed_skill_still_requires_the_explicit_read_path() {
    let project = TempDir::new().unwrap();
    init_project(project.path());
    let installed = project.path().join(".claude/skills/demo");
    fs::create_dir_all(&installed).unwrap();
    fs::write(
        installed.join("SKILL.md"),
        "---\nname: demo\ndescription: fixture\n---\n# Demo\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["demo"], Some(project.path()));

    assert!(
        !result.success,
        "a bare installed skill ID must fail: {}{}",
        result.stdout, result.stderr
    );
    assert!(result.stderr.contains("unrecognized"), "{}", result.stderr);

    let explicit = run_fastskill_command(&["skill", "read", "demo"], Some(project.path()));
    assert!(explicit.success, "{}{}", explicit.stdout, explicit.stderr);
    assert!(explicit.stdout.contains("# Demo"));
}

//! End-to-end update tests using a deterministic local managed skill.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::run_fastskill_command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct LocalUpdateFixture {
    root: TempDir,
    source: PathBuf,
    installed: PathBuf,
    lock: PathBuf,
}

fn write_skill(path: &Path, version: &str, body: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!(
            "---\nname: local-update\ndescription: deterministic update fixture\nversion: {version}\n---\n{body}\n"
        ),
    )
    .unwrap();
}

fn fixture() -> LocalUpdateFixture {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".skills\"\n",
    )
    .unwrap();
    let source = root.path().join("source/local-update");
    write_skill(&source, "1.0.0", "version one");
    let added = run_fastskill_command(
        &["add", source.to_str().unwrap(), "--no-reindex"],
        Some(root.path()),
    );
    assert!(
        added.success,
        "fixture add failed: {}{}",
        added.stdout, added.stderr
    );
    LocalUpdateFixture {
        installed: root.path().join(".skills/local-update"),
        lock: root.path().join("skills.lock"),
        source,
        root,
    }
}

fn advance_source(fixture: &LocalUpdateFixture) {
    write_skill(&fixture.source, "1.1.0", "version two");
}

#[test]
fn update_all_applies_verified_local_content() {
    let fixture = fixture();
    advance_source(&fixture);

    let result = run_fastskill_command(&["update", "--no-reindex"], Some(fixture.root.path()));

    assert!(
        result.success,
        "update failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(result.stdout.contains("Updated 1 skill"));
    let installed = fs::read_to_string(fixture.installed.join("SKILL.md")).unwrap();
    assert!(installed.contains("version: 1.1.0"));
    assert!(installed.contains("version two"));
}

#[test]
fn update_named_skill_reports_and_applies_target() {
    let fixture = fixture();
    advance_source(&fixture);

    let result = run_fastskill_command(
        &["update", "local-update", "--no-reindex"],
        Some(fixture.root.path()),
    );

    assert!(
        result.success,
        "update failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(result.stdout.contains("local-update"));
    assert!(fs::read_to_string(fixture.installed.join("SKILL.md"))
        .unwrap()
        .contains("version two"));
}

#[test]
fn update_honors_project_skills_directory_override() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".skills\"\n",
    )
    .unwrap();
    let source = root.path().join("source/local-update");
    write_skill(&source, "1.0.0", "version one");
    let override_dir = root.path().join("override-skills");
    fs::create_dir_all(&override_dir).unwrap();
    let override_arg = override_dir.to_str().unwrap();
    let source_arg = source.to_str().unwrap();

    let added = run_fastskill_command(
        &[
            "--skills-dir",
            override_arg,
            "add",
            source_arg,
            "--no-reindex",
        ],
        Some(root.path()),
    );
    assert!(
        added.success,
        "fixture add failed: {}{}",
        added.stdout, added.stderr
    );
    assert!(override_dir.join("local-update/SKILL.md").exists());
    assert!(!root.path().join(".skills/local-update").exists());

    write_skill(&source, "1.1.0", "version two");
    let updated = run_fastskill_command(
        &[
            "--skills-dir",
            override_arg,
            "update",
            "local-update",
            "--no-reindex",
        ],
        Some(root.path()),
    );

    assert!(
        updated.success,
        "update failed: {}{}",
        updated.stdout, updated.stderr
    );
    let installed = fs::read_to_string(override_dir.join("local-update/SKILL.md")).unwrap();
    assert!(installed.contains("version: 1.1.0"));
    assert!(installed.contains("version two"));
    assert!(!root.path().join(".skills/local-update").exists());
}

#[test]
fn update_check_is_a_truthful_non_mutating_plan() {
    let fixture = fixture();
    advance_source(&fixture);
    let before_lock = fs::read(&fixture.lock).unwrap();
    let before_installed = fs::read(fixture.installed.join("SKILL.md")).unwrap();

    let result = run_fastskill_command(
        &["update", "local-update", "--check", "--json"],
        Some(fixture.root.path()),
    );

    assert!(
        result.success,
        "check failed: {}{}",
        result.stdout, result.stderr
    );
    let json: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["outcome"], "changed");
    assert_eq!(json["targets"][0]["id"], "local-update");
    assert_eq!(fs::read(&fixture.lock).unwrap(), before_lock);
    assert_eq!(
        fs::read(fixture.installed.join("SKILL.md")).unwrap(),
        before_installed
    );
}

#[test]
fn update_dry_run_is_a_truthful_non_mutating_plan() {
    let fixture = fixture();
    advance_source(&fixture);
    let before_lock = fs::read(&fixture.lock).unwrap();
    let before_installed = fs::read(fixture.installed.join("SKILL.md")).unwrap();

    let result = run_fastskill_command(
        &["update", "--dry-run", "--json"],
        Some(fixture.root.path()),
    );

    assert!(
        result.success,
        "dry-run failed: {}{}",
        result.stdout, result.stderr
    );
    let json: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
    assert_eq!(json["outcome"], "changed");
    assert_eq!(json["dry_run"], true);
    assert_eq!(fs::read(&fixture.lock).unwrap(), before_lock);
    assert_eq!(
        fs::read(fixture.installed.join("SKILL.md")).unwrap(),
        before_installed
    );
}

#[test]
fn update_preview_rejects_local_edits_before_reporting_a_change() {
    let fixture = fixture();
    advance_source(&fixture);
    write_skill(&fixture.installed, "1.0.0", "locally edited content");
    let before_lock = fs::read(&fixture.lock).unwrap();
    let before_installed = fs::read(fixture.installed.join("SKILL.md")).unwrap();

    let result = run_fastskill_command(
        &["update", "local-update", "--dry-run", "--json"],
        Some(fixture.root.path()),
    );

    assert!(!result.success, "preview unexpectedly succeeded");
    let json: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
    assert!(matches!(
        json["outcome"].as_str(),
        Some("blocked" | "failed")
    ));
    assert!(json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic
            .as_str()
            .is_some_and(|message| { message.contains("local") || message.contains("modified") })));
    assert_eq!(fs::read(&fixture.lock).unwrap(), before_lock);
    assert_eq!(
        fs::read(fixture.installed.join("SKILL.md")).unwrap(),
        before_installed
    );
}

#[test]
fn repository_strategies_reject_local_origins_before_mutation() {
    for strategy in ["patch", "minor"] {
        let fixture = fixture();
        advance_source(&fixture);
        let before_lock = fs::read(&fixture.lock).unwrap();
        let before_installed = fs::read(fixture.installed.join("SKILL.md")).unwrap();

        let result = run_fastskill_command(
            &["update", "local-update", "--strategy", strategy],
            Some(fixture.root.path()),
        );

        assert!(!result.success, "{strategy} unexpectedly succeeded");
        assert!(
            result.stderr.contains("repository-origin"),
            "{}",
            result.stderr
        );
        assert_eq!(fs::read(&fixture.lock).unwrap(), before_lock);
        assert_eq!(
            fs::read(fixture.installed.join("SKILL.md")).unwrap(),
            before_installed
        );
    }
}

#[test]
fn update_missing_lock_file_is_an_error() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\nlocal-update = { origin = { type = \"local\", path = \"source/local-update\" } }\n\n[tool.fastskill]\nskills_directory = \".skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["update"], Some(root.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("skills.lock") && result.stderr.contains("not found"));
}

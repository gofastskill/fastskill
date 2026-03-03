//! Integration tests for legacy command deprecation and forwarding behavior.
//!
//! These tests execute the CLI binary and verify migration warnings are emitted
//! while legacy commands continue to function.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::run_fastskill_command;
use std::fs;
use tempfile::TempDir;

fn write_project_manifest(project_dir: &std::path::Path) {
    fs::write(
        project_dir.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();
}

fn write_local_skill_repo(
    base: &std::path::Path,
    skill_id: &str,
    version: &str,
) -> std::path::PathBuf {
    let repo_dir = base.join("legacy-local-repo");
    let skill_dir = repo_dir.join(skill_id);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {skill_id}\ndescription: legacy skill\nversion: {version}\n---\n# {skill_id}\n"
        ),
    )
    .unwrap();
    repo_dir
}

fn assert_deprecation_warning(stderr: &str, legacy_command: &str, new_command: &str) {
    assert!(stderr.contains(&format!(
        "Warning: The '{legacy_command}' command is deprecated"
    )));
    assert!(stderr.contains(&format!("Please use 'fastskill {new_command}'")));
    assert!(stderr.contains("→"));
}

#[test]
fn test_sources_deprecation_warnings_and_forwarding() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let local_repo_path = write_local_skill_repo(temp_dir.path(), "legacy-skill", "1.2.0");

    let add = run_fastskill_command(
        &[
            "sources",
            "add",
            "legacy-local",
            "--repo-type",
            "local",
            local_repo_path.to_str().unwrap(),
        ],
        Some(temp_dir.path()),
    );
    assert!(
        add.success,
        "sources add failed: {}{}",
        add.stdout, add.stderr
    );
    assert_deprecation_warning(&add.stderr, "sources", "repos");
    assert!(add.stderr.contains("'sources add' → 'repos add'"));
    assert!(add.stdout.contains("Added repository: legacy-local"));

    let list = run_fastskill_command(&["sources", "list", "--json"], Some(temp_dir.path()));
    assert!(
        list.success,
        "sources list failed: {}{}",
        list.stdout, list.stderr
    );
    assert_deprecation_warning(&list.stderr, "sources", "repos");
    assert!(list.stdout.contains("legacy-local"));

    let remove = run_fastskill_command(
        &["sources", "remove", "legacy-local"],
        Some(temp_dir.path()),
    );
    assert!(
        remove.success,
        "sources remove failed: {}{}",
        remove.stdout, remove.stderr
    );
    assert_deprecation_warning(&remove.stderr, "sources", "repos");
    assert!(remove.stdout.contains("Removed repository: legacy-local"));
}

#[test]
fn test_registry_deprecation_warnings_and_forwarding() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let local_repo_path = write_local_skill_repo(temp_dir.path(), "catalog-skill", "2.0.0");

    let add = run_fastskill_command(
        &[
            "repos",
            "add",
            "catalog-local",
            "--repo-type",
            "local",
            local_repo_path.to_str().unwrap(),
        ],
        Some(temp_dir.path()),
    );
    assert!(
        add.success,
        "repos add failed: {}{}",
        add.stdout, add.stderr
    );

    let show = run_fastskill_command(
        &[
            "registry",
            "show-skill",
            "catalog-skill",
            "--repository",
            "catalog-local",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        show.success,
        "registry show-skill failed: {}{}",
        show.stdout, show.stderr
    );
    assert_deprecation_warning(&show.stderr, "registry", "repos");
    assert!(show.stderr.contains("'registry show-skill' → 'repos show'"));
    assert!(show.stdout.contains("Skill: catalog-skill"));

    let versions = run_fastskill_command(
        &[
            "registry",
            "versions",
            "catalog-skill",
            "--repository",
            "catalog-local",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        versions.success,
        "registry versions failed: {}{}",
        versions.stdout, versions.stderr
    );
    assert_deprecation_warning(&versions.stderr, "registry", "repos");
    assert!(versions
        .stderr
        .contains("'registry versions' → 'repos versions'"));
    assert!(versions.stdout.contains("Available versions:"));
    assert!(versions.stdout.contains("2.0.0"));
}

#[test]
fn test_legacy_commands_hidden_from_top_level_help() {
    let result = run_fastskill_command(&["--help"], None);
    assert!(result.success);

    assert!(result.stdout.contains("repos"));
    assert!(result.stdout.contains("search"));
    assert!(!result.stdout.contains("sources"));
    assert!(!result.stdout.contains("registry"));
}

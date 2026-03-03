//! Integration tests for consolidated `repos` command workflows.
//!
//! These tests execute the CLI binary end-to-end and validate the repos
//! workflow matrix required by spec 026a.

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
    let repo_dir = base.join("local-repo");
    let skill_dir = repo_dir.join(skill_id);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {skill_id}\ndescription: test skill\nversion: {version}\n---\n# {skill_id}\n"
        ),
    )
    .unwrap();
    repo_dir
}

#[test]
fn test_repos_complete_workflow_matrix() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let local_repo_path = write_local_skill_repo(temp_dir.path(), "matrix-skill", "1.0.0");

    let add = run_fastskill_command(
        &[
            "repos",
            "add",
            "matrix-local",
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
    assert!(add.stdout.contains("Added repository: matrix-local"));

    let list = run_fastskill_command(&["repos", "list", "--json"], Some(temp_dir.path()));
    assert!(
        list.success,
        "repos list failed: {}{}",
        list.stdout, list.stderr
    );
    assert!(list.stdout.contains("matrix-local"));

    let info = run_fastskill_command(
        &["repos", "info", "matrix-local", "--json"],
        Some(temp_dir.path()),
    );
    assert!(
        info.success,
        "repos info failed: {}{}",
        info.stdout, info.stderr
    );
    assert!(info.stdout.contains("\"name\": \"matrix-local\""));

    let update = run_fastskill_command(
        &["repos", "update", "matrix-local", "--priority", "3"],
        Some(temp_dir.path()),
    );
    assert!(
        update.success,
        "repos update failed: {}{}",
        update.stdout, update.stderr
    );
    assert!(update.stdout.contains("Updated repository: matrix-local"));

    let test = run_fastskill_command(&["repos", "test", "matrix-local"], Some(temp_dir.path()));
    assert!(
        test.success,
        "repos test failed: {}{}",
        test.stdout, test.stderr
    );
    assert!(test.stdout.contains("Testing repository: matrix-local"));

    let refresh_one =
        run_fastskill_command(&["repos", "refresh", "matrix-local"], Some(temp_dir.path()));
    assert!(
        refresh_one.success,
        "repos refresh <name> failed: {}{}",
        refresh_one.stdout, refresh_one.stderr
    );
    assert!(refresh_one
        .stdout
        .contains("Refreshed cache for repository: matrix-local"));

    let refresh_all = run_fastskill_command(&["repos", "refresh"], Some(temp_dir.path()));
    assert!(
        refresh_all.success,
        "repos refresh failed: {}{}",
        refresh_all.stdout, refresh_all.stderr
    );
    assert!(refresh_all
        .stdout
        .contains("Refreshed cache for all repositories"));

    let skills = run_fastskill_command(
        &["repos", "skills", "--repository", "matrix-local"],
        Some(temp_dir.path()),
    );
    assert!(
        !skills.success,
        "repos skills should fail for local repo: {}{}",
        skills.stdout, skills.stderr
    );
    assert!(skills.stderr.contains("is not an HTTP registry"));

    let show = run_fastskill_command(
        &[
            "repos",
            "show",
            "matrix-skill",
            "--repository",
            "matrix-local",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        show.success,
        "repos show failed: {}{}",
        show.stdout, show.stderr
    );
    assert!(show.stdout.contains("Skill: matrix-skill"));

    let versions = run_fastskill_command(
        &[
            "repos",
            "versions",
            "matrix-skill",
            "--repository",
            "matrix-local",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        versions.success,
        "repos versions failed: {}{}",
        versions.stdout, versions.stderr
    );
    assert!(versions.stdout.contains("Available versions:"));
    assert!(versions.stdout.contains("1.0.0"));

    let remove = run_fastskill_command(&["repos", "remove", "matrix-local"], Some(temp_dir.path()));
    assert!(
        remove.success,
        "repos remove failed: {}{}",
        remove.stdout, remove.stderr
    );
    assert!(remove.stdout.contains("Removed repository: matrix-local"));
}

#[test]
fn test_repos_command_excludes_search_subcommand() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let repos_search = run_fastskill_command(&["repos", "search", "test"], Some(temp_dir.path()));
    assert!(!repos_search.success);
    assert!(repos_search
        .stderr
        .contains("unrecognized subcommand 'search'"));

    let search_help = run_fastskill_command(&["search", "--help"], Some(temp_dir.path()));
    assert!(search_help.success);
    assert!(search_help.stdout.contains("Search skills by query"));
}

#[test]
fn test_repos_help_does_not_advertise_search() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let repos_help = run_fastskill_command(&["repos", "--help"], Some(temp_dir.path()));
    assert!(repos_help.success);
    assert!(!repos_help.stdout.contains(" repos search "));
    assert!(repos_help.stdout.contains("skills"));
    assert!(repos_help.stdout.contains("show"));
    assert!(repos_help.stdout.contains("versions"));
}

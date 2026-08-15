//! E2E tests for `repos` (repository management) commands.
//!
//! These tests execute the CLI binary and verify actual behavior. They
//! target scenarios that `tests/cli/repos_integration_tests.rs` does not
//! already cover (its `test_repos_complete_workflow_matrix` walks a single
//! happy-path sequence through add/list/info/update/test/refresh/skills/
//! show/versions/remove for one `local`-type repository). The tests here
//! were ported from the retired `sources`/`registry` top-level commands
//! (see issue-#183 "cli-command-surface-redesign") onto their `repos`
//! equivalents, keeping only cases that add coverage beyond that matrix:
//! empty-state output, manifest-defined repositories (not added via the
//! CLI), the `--priority`/`--branch` flags on `add`, the `git-marketplace`
//! repo type, validation/duplicate/not-found error paths, and both the
//! success and failure paths of `repos test` connectivity checks.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use std::io::ErrorKind;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn start_mock_server_or_skip() -> Option<MockServer> {
    match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => Some(MockServer::builder().listener(listener).start().await),
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            eprintln!("Skipping test: unable to bind local mock server socket ({err})");
            None
        }
        Err(err) => panic!("failed to bind local mock server socket: {err}"),
    }
}

#[test]
fn test_repos_list_empty() {
    let temp_dir = TempDir::new().unwrap();

    let result = run_fastskill_command(&["repos", "list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("No repositories") || result.stdout.is_empty());

    assert_snapshot_with_settings("repos_list_empty", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_repos_list_with_manifest_defined_repositories() {
    let temp_dir = TempDir::new().unwrap();

    // Repositories declared directly in skill-project.toml (not added via
    // `repos add`) must still be parsed and listed correctly.
    let project_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".cursor/skills"

[[tool.fastskill.repositories]]
name = "local-skills"
type = "local"
path = "/path/to/local/skills"
priority = 10

[[tool.fastskill.repositories]]
name = "git-repo"
type = "git-marketplace"
url = "https://github.com/org/repo.git"
branch = "main"
priority = 5
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), project_content).unwrap();

    let result = run_fastskill_command(&["repos", "list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("local-skills") && result.stdout.contains("git-repo"));

    assert_snapshot_with_settings(
        "repos_list_with_manifest_repos",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_add_with_priority() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // `repos update --priority` is covered by the workflow matrix, but
    // setting priority at `add` time is a separate code path.
    let result = run_fastskill_command(
        &[
            "repos",
            "add",
            "priority-repo",
            "--repo-type",
            "local",
            "/tmp/test",
            "--priority",
            "10",
        ],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Added repository: priority-repo"));

    let info = run_fastskill_command(
        &["repos", "info", "priority-repo", "--json"],
        Some(temp_dir.path()),
    );
    assert!(info.success);
    assert!(info.stdout.contains("\"priority\": 10"));

    assert_snapshot_with_settings(
        "repos_add_with_priority",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_add_git_marketplace() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // The workflow matrix only exercises the `local` repo type; verify the
    // `git-marketplace` type (with `--branch`) is also wired up via `add`.
    let result = run_fastskill_command(
        &[
            "repos",
            "add",
            "git-repo",
            "--repo-type",
            "git-marketplace",
            "https://github.com/org/repo.git",
            "--branch",
            "main",
        ],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Added repository: git-repo"));

    let info = run_fastskill_command(&["repos", "info", "git-repo"], Some(temp_dir.path()));
    assert!(info.success);
    assert!(info.stdout.contains("Type: git-marketplace"));
    assert!(info.stdout.contains("Branch: main"));

    assert_snapshot_with_settings(
        "repos_add_git_marketplace",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_add_validation_missing_url() {
    let temp_dir = TempDir::new().unwrap();

    let result = run_fastskill_command(
        &["repos", "add", "missing-url", "--repo-type", "local"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("required"));

    assert_snapshot_with_settings(
        "repos_add_missing_url",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_add_duplicate_name_error() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let result1 = run_fastskill_command(
        &[
            "repos",
            "add",
            "duplicate-repo",
            "--repo-type",
            "local",
            "/tmp/test1",
        ],
        Some(temp_dir.path()),
    );
    assert!(result1.success);

    let result2 = run_fastskill_command(
        &[
            "repos",
            "add",
            "duplicate-repo",
            "--repo-type",
            "local",
            "/tmp/test2",
        ],
        Some(temp_dir.path()),
    );

    assert!(!result2.success);
    assert!(result2.stderr.contains("already exists"));

    assert_snapshot_with_settings(
        "repos_add_duplicate",
        &format!("{}{}", result2.stdout, result2.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_remove_nonexistent_error() {
    let temp_dir = TempDir::new().unwrap();

    let result = run_fastskill_command(
        &["repos", "remove", "nonexistent-repo"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("not found"));

    assert_snapshot_with_settings(
        "repos_remove_nonexistent",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_info_repository_details() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let add_result = run_fastskill_command(
        &[
            "repos",
            "add",
            "showable-repo",
            "--repo-type",
            "git-marketplace",
            "https://github.com/org/repo.git",
            "--priority",
            "5",
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result.success);

    // Text-mode `repos info` (the matrix only exercises `--json` mode).
    let result = run_fastskill_command(&["repos", "info", "showable-repo"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("showable-repo") && result.stdout.contains("git-marketplace"));

    assert_snapshot_with_settings(
        "repos_info_details",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_update_branch() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let add_result = run_fastskill_command(
        &[
            "repos",
            "add",
            "updateable-repo",
            "--repo-type",
            "git-marketplace",
            "https://github.com/org/repo.git",
            "--branch",
            "main",
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result.success);

    // The matrix only exercises `repos update --priority`; verify
    // `--branch` updates independently.
    let result = run_fastskill_command(
        &["repos", "update", "updateable-repo", "--branch", "develop"],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result
        .stdout
        .contains("Updated repository: updateable-repo"));

    let info = run_fastskill_command(&["repos", "info", "updateable-repo"], Some(temp_dir.path()));
    assert!(info.success);
    assert!(info.stdout.contains("Branch: develop"));

    assert_snapshot_with_settings(
        "repos_update_branch",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[tokio::test]
async fn test_repos_test_connectivity_reachable() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // The matrix's `repos test` only covers a `local`-type repository,
    // which never makes a network call. Exercise the real HTTP-registry
    // reachable path here.
    let Some(mock_server) = start_mock_server_or_skip().await else {
        return;
    };
    Mock::given(method("GET"))
        .and(path("/api/v1/registry/index/skills"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&mock_server)
        .await;

    let add_result = run_fastskill_command(
        &[
            "repos",
            "add",
            "testable-repo",
            "--repo-type",
            "http-registry",
            &mock_server.uri(),
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result.success);

    let result = run_fastskill_command(&["repos", "test", "testable-repo"], Some(temp_dir.path()));

    assert!(
        result.success,
        "repos test failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(result.stdout.contains("accessible"));

    assert_snapshot_with_settings(
        "repos_test_connectivity",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_repos_test_unreachable_error() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "repos",
            "add",
            "unreachable-repo",
            "--repo-type",
            "http-registry",
            "http://localhost:9999",
        ],
        Some(temp_dir.path()),
    );
    assert!(result.success);

    let test_result = run_fastskill_command(
        &["repos", "test", "unreachable-repo"],
        Some(temp_dir.path()),
    );

    assert!(!test_result.success);
    assert!(test_result.stderr.contains("error") || test_result.stderr.contains("test failed"));

    assert_snapshot_with_settings(
        "repos_test_unreachable",
        &format!("{}{}", test_result.stdout, test_result.stderr),
        &cli_snapshot_settings(),
    );
}

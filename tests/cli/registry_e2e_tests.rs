//! E2E tests for sources (registry) commands
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_sources_list_empty() {
    let temp_dir = TempDir::new().unwrap();

    let result = run_fastskill_command(&["sources", "list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("No repositories") || result.stdout.is_empty());

    assert_snapshot_with_settings(
        "sources_list_empty",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_list_with_repositories() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml with repositories
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

    let result = run_fastskill_command(&["sources", "list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("local-skills") && result.stdout.contains("git-repo"));

    assert_snapshot_with_settings(
        "sources_list_with_repos",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_add_local_repository() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("test-repo");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "sources",
            "add",
            "test-repo",
            "--repo-type",
            "local",
            skills_dir.to_str().unwrap(),
        ],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Added") || result.stdout.contains("test-repo"));

    assert_snapshot_with_settings(
        "sources_add_local",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_add_with_priority() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "sources",
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
    assert!(result.stdout.contains("Added") || result.stdout.contains("priority"));

    assert_snapshot_with_settings(
        "sources_add_with_priority",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_add_git_marketplace() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "sources",
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
    assert!(result.stdout.contains("Added") || result.stdout.contains("git-repo"));

    assert_snapshot_with_settings("sources_add_git", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_sources_add_validation_missing_url() {
    let temp_dir = TempDir::new().unwrap();

    let result = run_fastskill_command(
        &["sources", "add", "missing-url", "--repo-type", "local"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("required"));

    assert_snapshot_with_settings(
        "sources_add_missing_url",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_add_duplicate_name_error() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Add first repository
    let result1 = run_fastskill_command(
        &[
            "sources",
            "add",
            "duplicate-repo",
            "--repo-type",
            "local",
            "/tmp/test1",
        ],
        Some(temp_dir.path()),
    );
    assert!(result1.success);

    // Try to add second repository with same name
    let result2 = run_fastskill_command(
        &[
            "sources",
            "add",
            "duplicate-repo",
            "--repo-type",
            "local",
            "/tmp/test2",
        ],
        Some(temp_dir.path()),
    );

    assert!(!result2.success);
    assert!(result2.stderr.contains("error") || result2.stderr.contains("already exists"));

    assert_snapshot_with_settings(
        "sources_add_duplicate",
        &format!("{}{}", result2.stdout, result2.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_remove_existing_repo() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Add a repository first
    let add_result = run_fastskill_command(
        &[
            "sources",
            "add",
            "removable-repo",
            "--repo-type",
            "local",
            "/tmp/test",
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result.success);

    // Remove the repository
    let result = run_fastskill_command(
        &["sources", "remove", "removable-repo"],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Removed") || result.stdout.contains("removable-repo"));

    assert_snapshot_with_settings(
        "sources_remove_success",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_remove_nonexistent_error() {
    let temp_dir = TempDir::new().unwrap();

    let result = run_fastskill_command(
        &["sources", "remove", "nonexistent-repo"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("not found"));

    assert_snapshot_with_settings(
        "sources_remove_nonexistent",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_show_repository_details() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Add a repository first
    let add_result = run_fastskill_command(
        &[
            "sources",
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

    // Show repository details
    let result =
        run_fastskill_command(&["sources", "show", "showable-repo"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("showable-repo") && result.stdout.contains("git-marketplace"));

    assert_snapshot_with_settings(
        "sources_show_details",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_update_branch() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Add a repository first
    let add_result = run_fastskill_command(
        &[
            "sources",
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

    // Update repository branch
    let result = run_fastskill_command(
        &[
            "sources",
            "update",
            "updateable-repo",
            "--branch",
            "develop",
        ],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Updated") || result.stdout.contains("develop"));

    assert_snapshot_with_settings(
        "sources_update_branch",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_update_priority() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Add a repository first
    let add_result = run_fastskill_command(
        &[
            "sources",
            "add",
            "priority-update-repo",
            "--repo-type",
            "local",
            "/tmp/test",
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result.success);

    // Update repository priority
    let result = run_fastskill_command(
        &[
            "sources",
            "update",
            "priority-update-repo",
            "--priority",
            "15",
        ],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Updated") || result.stdout.contains("15"));

    assert_snapshot_with_settings(
        "sources_update_priority",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[tokio::test]
async fn test_sources_test_connectivity() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Mock server for testing
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&mock_server)
        .await;

    // Add a mock repository that references the mock server
    let add_result = run_fastskill_command(
        &[
            "sources",
            "add",
            "testable-repo",
            "--repo-type",
            "http-registry",
            &mock_server.uri(),
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result.success);

    // Test repository connectivity
    let result =
        run_fastskill_command(&["sources", "test", "testable-repo"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("reachable") || result.stdout.contains("OK"));

    assert_snapshot_with_settings(
        "sources_test_connectivity",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_test_unreachable_error() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "sources",
            "add",
            "unreachable-repo",
            "--repo-type",
            "http-registry",
            "http://localhost:9999",
        ],
        Some(temp_dir.path()),
    );
    assert!(result.success);

    // Test unreachable repository
    let test_result = run_fastskill_command(
        &["sources", "test", "unreachable-repo"],
        Some(temp_dir.path()),
    );

    assert!(!test_result.success);
    assert!(
        test_result.stderr.contains("error") || test_result.stderr.contains("Failed to connect")
    );

    assert_snapshot_with_settings(
        "sources_test_unreachable",
        &format!("{}{}", test_result.stdout, test_result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_refresh_single_repo() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Add a repository first
    let add_result = run_fastskill_command(
        &[
            "sources",
            "add",
            "refreshable-repo",
            "--repo-type",
            "local",
            "/tmp/test",
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result.success);

    // Refresh single repository
    let result = run_fastskill_command(
        &["sources", "refresh", "refreshable-repo"],
        Some(temp_dir.path()),
    );

    assert!(result.success);
    assert!(result.stdout.contains("Refreshed") || result.stdout.contains("refreshable-repo"));

    assert_snapshot_with_settings(
        "sources_refresh_single",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_sources_refresh_all_repos() {
    let temp_dir = TempDir::new().unwrap();

    // Create skill-project.toml to establish project structure
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".cursor/skills\"\n",
    )
    .unwrap();

    // Add multiple repositories
    let add_result1 = run_fastskill_command(
        &[
            "sources",
            "add",
            "repo1",
            "--repo-type",
            "local",
            "/tmp/test1",
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result1.success);

    let add_result2 = run_fastskill_command(
        &[
            "sources",
            "add",
            "repo2",
            "--repo-type",
            "local",
            "/tmp/test2",
        ],
        Some(temp_dir.path()),
    );
    assert!(add_result2.success);

    // Refresh all repositories
    let result = run_fastskill_command(&["sources", "refresh"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("Refreshed") || result.stdout.contains("repositories"));

    assert_snapshot_with_settings(
        "sources_refresh_all",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

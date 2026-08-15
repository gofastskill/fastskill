//! Integration tests for CLI functionality

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

mod cli;

#[test]
fn test_cli_help() {
    use std::process::Command;

    let binary = cli::snapshot_helpers::get_binary_path();
    let output = if binary == "cargo" {
        Command::new("cargo")
            .args(["run", "--bin", "fastskill", "--", "--help"])
            .output()
            .expect("Failed to execute CLI")
    } else {
        Command::new(&binary)
            .arg("--help")
            .output()
            .expect("Failed to execute CLI")
    };

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FastSkill"));
}

#[tokio::test]
async fn test_service_integration() {
    use fastskill_core::{FastSkillService, ServiceConfig};
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir,
        ..Default::default()
    };

    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test search functionality
    let results = service.metadata_service().discover_skills("test").await;
    assert!(results.is_ok());
}

// E2E tests for uncovered commands

#[test]
fn test_list_default_grid_format() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(&["list"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(
        result.stdout.contains("No skills") || result.stdout.contains("[INFO]"),
        "stdout: {}",
        result.stdout
    );

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "list_default_grid",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_list_json_format() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".claude").join("skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result =
        cli::snapshot_helpers::run_fastskill_command(&["list", "--json"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("[]") || result.stdout.contains("\"id\""));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "list_json_format",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_reindex_empty_directory() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml with tool.fastskill.embedding config
    let project_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), project_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result = cli::snapshot_helpers::run_fastskill_command_with_env(
        &["reindex"],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    // Empty skills directory should succeed with minimal or no output
    // Since it just shows version, that's normal behavior

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "reindex_empty_directory",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_install_missing_project_file_error() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(&["install"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("skill-project.toml"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "install_missing_project_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

// `show` was retired (main.rs explicitly keeps it out of the `read` shorthand
// as one of the issue-#183 "cli-command-surface-redesign" removals, alongside
// `resolve`/`sync`/`disable`). Its job — show metadata for a skill — is now
// `read --meta`, so these two tests are adapted to that rather than deleted:
// the underlying behavior (a clear error for an unknown/invalid skill id)
// still exists and is still worth covering.

#[test]
fn test_show_nonexistent_skill_error() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    // Needs a real skill-project.toml, otherwise the command fails on
    // "skill-project.toml not found" before it ever looks for the skill —
    // a "not found" message, but the wrong one for what this test means to
    // cover (an unknown skill id, not a missing project file).
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["read", "--meta", "nonexistent-skill"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("not found"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "show_nonexistent_skill_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_show_invalid_skill_id_format() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["read", "--meta", "invalid skill id!"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("Invalid skill ID"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "show_invalid_id_format_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_read_nonexistent_skill_error() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["read", "nonexistent-skill"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("not found"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "read_nonexistent_skill_cli",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_read_invalid_skill_id_format() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = cli::snapshot_helpers::run_fastskill_command(
        &["read", "invalid skill id!"],
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("Invalid skill ID"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "read_invalid_id_format",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

// NOTE: `auth login` / `auth logout` tested a registry-authentication
// subcommand that does not exist in the current CLI surface (verified via
// `fastskill --help`: no `auth` command group at all — only
// `repos {add,remove,list,...}` for repository management, which has no
// login/logout concept). This isn't a rename target like `sources`/`registry`
// below; there is no current equivalent to adapt to, so the two `auth_*`
// tests are deleted rather than kept as dead weight against a feature that
// was never shipped in this command-layer generation.

#[test]
fn test_registry_list_empty() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    // `sources` was renamed to `repos` (see `fastskill --help`).
    let result =
        cli::snapshot_helpers::run_fastskill_command(&["repos", "list"], Some(temp_dir.path()));
    assert!(result.success);
    assert!(result.stdout.contains("No repositories") || result.stdout.is_empty());

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "registry_list_empty",
        &result.stdout,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_registry_add_validation_missing_url() {
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).unwrap();

    // `sources add` was renamed to `repos add`; it still requires both
    // `<name>` and `<url-or-path>` positionals (see `fastskill repos add --help`).
    let result = cli::snapshot_helpers::run_fastskill_command(
        &["repos", "add", "missing-url", "--repo-type", "local"],
        None,
    );
    assert!(!result.success);
    assert!(result.stderr.contains("required") || result.stderr.contains("argument"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "registry_add_missing_url",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

#[test]
fn test_registry_remove_nonexistent_error() {
    use tempfile::TempDir;

    // `registry remove` was renamed to `repos remove`, which now exists as a
    // real subcommand (the old assertion special-cased "unrecognized
    // subcommand 'remove'" because at the time neither `registry` nor
    // `remove` resolved at all). Assert the actual not-found error instead of
    // the generic "error" fallback, since `repos remove` is real now.
    //
    // Run in a fresh temp dir rather than `None` (inherited cwd): unlike
    // `repos add`'s missing-arg check (rejected during arg parsing, before
    // any config is touched), `repos remove` loads skill-project.toml to
    // resolve the configured repositories, so with `None` this test picks up
    // whatever manifest (if any) happens to sit above the process's actual
    // working directory and asserts on the wrong error entirely.
    let temp_dir = TempDir::new().unwrap();
    let result = cli::snapshot_helpers::run_fastskill_command(
        &["repos", "remove", "nonexistent-repo"],
        Some(temp_dir.path()),
    );
    assert!(!result.success);
    assert!(result.stderr.contains("not found"));

    cli::snapshot_helpers::assert_snapshot_with_settings(
        "registry_remove_nonexistent",
        &result.stderr,
        &cli::snapshot_helpers::cli_snapshot_settings(),
    );
}

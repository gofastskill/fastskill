//! E2E tests for reindex command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command_with_env,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_reindex_default_directory() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create config for reindex
    let config_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), config_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result = run_fastskill_command_with_env(&["reindex"], &env_vars, Some(temp_dir.path()));

    assert!(result.success);
    // Should succeed even with empty skills directory

    assert_snapshot_with_settings(
        "reindex_default_directory",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_reindex_custom_directory() {
    let temp_dir = TempDir::new().unwrap();
    let custom_skills_dir = temp_dir.path().join("custom-skills");
    fs::create_dir_all(&custom_skills_dir).unwrap();

    // Create config for reindex
    let config_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), config_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result = run_fastskill_command_with_env(
        &[
            "reindex",
            "--skills-dir",
            custom_skills_dir.to_str().unwrap(),
        ],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    // Should succeed with custom directory

    assert_snapshot_with_settings(
        "reindex_custom_directory",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

/// Requires valid OPENAI_API_KEY; run with `cargo test -- --ignored` when key is set.
#[test]
#[ignore]
fn test_reindex_with_force_flag() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a skill to force reindex
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: test-skill
description: Test skill for force reindex
version: 1.0.0
---
# Test Skill
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create config for reindex
    let config_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), config_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result =
        run_fastskill_command_with_env(&["reindex", "--force"], &env_vars, Some(temp_dir.path()));

    assert!(result.success);
    // Should succeed with force flag

    assert_snapshot_with_settings("reindex_force", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_reindex_max_concurrent() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create config for reindex
    let config_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), config_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result = run_fastskill_command_with_env(
        &["reindex", "--max-concurrent", "2"],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(result.success);
    // Should succeed with concurrency limit

    assert_snapshot_with_settings(
        "reindex_max_concurrent",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_reindex_empty_directory() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create config for reindex
    let config_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), config_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result = run_fastskill_command_with_env(&["reindex"], &env_vars, Some(temp_dir.path()));

    assert!(result.success);
    // Should succeed with empty directory

    assert_snapshot_with_settings(
        "reindex_empty_directory",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_reindex_invalid_directory_error() {
    let temp_dir = TempDir::new().unwrap();

    // Create config for reindex
    let config_content = r#"[dependencies]

[tool.fastskill]
skills_directory = ".skills"

[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), config_content).unwrap();

    // Set OPENAI_API_KEY to avoid config requirement
    let env_vars = vec![("OPENAI_API_KEY", "test-key")];
    let result = run_fastskill_command_with_env(
        &["reindex", "--skills-dir", "/nonexistent/path"],
        &env_vars,
        Some(temp_dir.path()),
    );

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("not found"));

    assert_snapshot_with_settings(
        "reindex_invalid_directory",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

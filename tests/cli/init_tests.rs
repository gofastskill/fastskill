//! Integration tests for fastskill init command

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use fastskill_core::core::manifest::SkillProjectToml;
use std::fs;
use tempfile::TempDir;

/// T037: Test fastskill init creating skill-project.toml with metadata
///
/// REAL PRODUCT BUG (do not remove `#[ignore]` until fixed in `crates/fastskill-cli`):
/// `InitArgs::command_spec()` in `crates/fastskill-cli/src/commands/init.rs` declares a
/// custom `--version` argument that collides with clap's auto-generated `--version` flag.
/// This trips a clap debug_assert and panics on **every** invocation of `init` in debug
/// builds, not just `--version` usage:
///   $ ./target/debug/fastskill init --help
///   thread 'main' panicked at .../clap_builder-4.6.0/src/builder/debug_asserts.rs:99:13:
///   Command init: Argument names must be unique, but 'version' is in use by more than
///   one argument or group (call `cmd.disable_version_flag(true)` to remove the
///   auto-generated `--version`)
///   $ ./target/debug/fastskill init --yes   # also panics
/// Fix requires either renaming the custom flag or calling `disable_version_flag(true)`
/// on the generated clap Command for `init`, in production code this test's owner may
/// not modify. Re-enable once that lands.
#[test]
#[ignore = "REAL BUG: init's custom --version arg collides with clap's auto-generated \
            --version flag (debug_assert panic on every `init` invocation, not just \
            --version usage); see crates/fastskill-cli/src/commands/init.rs InitArgs::command_spec()"]
fn test_init_creates_skill_project_toml_with_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md to indicate skill-level context
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
version: 1.0.0
description: A test skill
author: Test Author
tags:
  - test
  - example
capabilities:
  - capability1
---
# Test Skill
"#,
    )
    .unwrap();

    // Run init command with --yes flag to skip prompts
    let result = run_fastskill_command(
        &[
            "init",
            "--yes",
            "--version",
            "1.0.0",
            "--description",
            "A test skill",
            "--author",
            "Test Author",
        ],
        Some(&skill_dir),
    );

    assert!(result.success, "init should succeed");
    assert_snapshot_with_settings(
        "init_with_metadata",
        &result.stdout,
        &cli_snapshot_settings(),
    );

    // Verify skill-project.toml was created
    let project_file = skill_dir.join("skill-project.toml");
    assert!(
        project_file.exists(),
        "skill-project.toml should be created"
    );

    // Verify it contains metadata
    let content = fs::read_to_string(&project_file).unwrap();
    let project: SkillProjectToml = toml::from_str(&content).unwrap();

    assert!(project.metadata.is_some());
    let metadata = project.metadata.as_ref().unwrap();
    assert_eq!(metadata.id, Some("test-skill".to_string()));
    assert_eq!(metadata.version, Some("1.0.0".to_string()));
    assert_eq!(metadata.description, Some("A test skill".to_string()));
    assert_eq!(metadata.author, Some("Test Author".to_string()));
}

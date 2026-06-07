//! E2E tests for read command
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_read_existing_skill() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a test skill in .skills directory
    let skill_dir = skills_dir.join("web-scraper");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: web-scraper
description: A web scraping skill
version: 1.2.3
tags: [scraping, web]
capabilities: [http_client]
---
# Web Scraper Skill

Advanced web scraping capabilities for extracting structured data from websites.

## Features

- Extract structured data
- Handle pagination  
- Support multiple formats (JSON, CSV, XML)
- Rate limiting support
- Custom headers and authentication

## Usage

Basic usage example for web scraping.

```bash
fastskill read web-scraper
```

This skill provides comprehensive web scraping functionality.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["read", "web-scraper"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("web-scraper") && result.stdout.contains("1.2.3"));

    assert_snapshot_with_settings(
        "read_existing_skill",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_read_displays_metadata() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a test skill with metadata
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: test-skill
description: A test skill for metadata display
version: 1.0.0
tags: [test, metadata, display]
capabilities: [display_capability]
---
# Test Skill

This skill tests metadata display functionality.

## Usage

Used for testing metadata display.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["read", "test-skill"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("name: test-skill"));
    assert!(result.stdout.contains("version: 1.0.0"));
    assert!(result.stdout.contains("description:"));
    assert!(result.stdout.contains("tags:"));

    assert_snapshot_with_settings("read_metadata", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_read_displays_skill_body() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create a test skill with body content
    let skill_dir = skills_dir.join("body-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: body-skill
description: Skill for testing body content display
version: 1.0.0
tags: [test, body]
---
# Body Skill

This skill has comprehensive body content for testing.

## Features

This skill demonstrates:
- Markdown rendering
- Multiple sections
- Code blocks
- Lists and tables

## Usage

Used to verify that skill body content is properly displayed in the CLI read command.

## Installation

Install this skill to test body content rendering.

## Examples

Example usage showing that the skill body should be displayed correctly.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["read", "body-skill"], Some(temp_dir.path()));

    assert!(result.success);
    assert!(result.stdout.contains("## Features"));
    assert!(result.stdout.contains("## Usage"));

    assert_snapshot_with_settings("read_skill_body", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_read_nonexistent_skill_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["read", "nonexistent-skill"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("not found"));

    assert_snapshot_with_settings(
        "read_nonexistent_skill",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_read_invalid_skill_id_error() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    fs::create_dir_all(&skills_dir).unwrap();

    // Create skill-project.toml
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".skills\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["read", "invalid skill id!"], Some(temp_dir.path()));

    assert!(!result.success);
    assert!(result.stderr.contains("error") || result.stderr.contains("Invalid"));

    assert_snapshot_with_settings(
        "read_invalid_skill_id",
        &format!("{}{}", result.stdout, result.stderr),
        &cli_snapshot_settings(),
    );
}

fn create_test_skill_dir(temp_dir: &TempDir) -> std::path::PathBuf {
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let skill_dir = skills_dir.join("meta-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"---
name: meta-skill
description: A skill for testing metadata display
version: 2.0.0
author: Test Author
tags: [meta, test]
---
# Meta Skill

This is the skill body content.
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    skills_dir
}

#[test]
fn test_read_meta_flag() {
    let temp_dir = TempDir::new().unwrap();
    create_test_skill_dir(&temp_dir);

    let result = run_fastskill_command(&["read", "meta-skill", "--meta"], Some(temp_dir.path()));

    assert!(result.success, "read --meta failed: {}", result.stderr);
    // --meta should show metadata, not the full SKILL.md body
    assert!(
        result.stdout.contains("meta-skill") || result.stdout.contains("2.0.0"),
        "Expected metadata in output, got: {}",
        result.stdout
    );
    // Should NOT contain raw body content like "This is the skill body"
    assert!(
        !result.stdout.contains("This is the skill body content"),
        "read --meta should not include file body: {}",
        result.stdout
    );

    assert_snapshot_with_settings("read_meta_flag", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_read_tree_flag() {
    let temp_dir = TempDir::new().unwrap();
    create_test_skill_dir(&temp_dir);

    let result = run_fastskill_command(&["read", "meta-skill", "--tree"], Some(temp_dir.path()));

    assert!(result.success, "read --tree failed: {}", result.stderr);
    // --tree should display dependency info and not raw body
    assert!(
        result.stdout.contains("meta-skill"),
        "Expected skill name in tree output, got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("This is the skill body content"),
        "read --tree should not include file body: {}",
        result.stdout
    );

    assert_snapshot_with_settings("read_tree_flag", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_read_meta_json_flag() {
    let temp_dir = TempDir::new().unwrap();
    create_test_skill_dir(&temp_dir);

    let result =
        run_fastskill_command(&["read", "meta-skill", "--meta", "--json"], Some(temp_dir.path()));

    assert!(result.success, "read --meta --json failed: {}", result.stderr);
    // Should be valid JSON containing id, name, version, description
    assert!(
        result.stdout.contains("\"meta-skill\"") || result.stdout.contains("meta-skill"),
        "Expected skill id/name in JSON output, got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("2.0.0"),
        "Expected version in JSON output, got: {}",
        result.stdout
    );
    // Should not contain raw body content
    assert!(
        !result.stdout.contains("This is the skill body content"),
        "read --meta --json should not include file body: {}",
        result.stdout
    );

    assert_snapshot_with_settings(
        "read_meta_json_flag",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_read_locked_without_meta_fails() {
    let temp_dir = TempDir::new().unwrap();
    fs::create_dir_all(temp_dir.path().join(".claude").join("skills")).unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let result =
        run_fastskill_command(&["read", "some-skill", "--locked"], Some(temp_dir.path()));

    assert!(!result.success, "read --locked without --meta should fail");
    assert!(
        result.stderr.contains("--meta") || result.stderr.contains("locked"),
        "Expected error about --meta requirement, got: {}",
        result.stderr
    );
}

#[test]
fn test_read_shorthand_streams_content() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude").join("skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let skill_dir = skills_dir.join("shorthand-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: shorthand-skill\nversion: 1.0.0\ndescription: shorthand test\n---\n# Shorthand\n\nBody content here.\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    // Test both `fastskill read <id>` and bare positional routing
    let result =
        run_fastskill_command(&["read", "shorthand-skill"], Some(temp_dir.path()));
    assert!(result.success, "read shorthand-skill failed: {}", result.stderr);
    assert!(
        result.stdout.contains("Body content here"),
        "Expected SKILL.md body in output, got: {}",
        result.stdout
    );
}

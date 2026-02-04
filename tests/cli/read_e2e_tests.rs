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

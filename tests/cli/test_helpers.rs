//! Helper functions for CLI tests

use fastskill::{FastSkillService, ServiceConfig};
use std::path::PathBuf;
use tempfile::TempDir;

/// Create a test service instance
pub fn create_test_service(skills_dir: Option<PathBuf>) -> (FastSkillService, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let skills_path = skills_dir.unwrap_or_else(|| temp_dir.path().join("skills"));
    std::fs::create_dir_all(&skills_path).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_path,
        ..Default::default()
    };

    // Note: Service creation is async, so this helper is for sync setup only
    (config, temp_dir)
}

/// Create a temporary skill directory with SKILL.md
pub fn create_temp_skill(temp_dir: &TempDir, skill_name: &str) -> PathBuf {
    let skill_dir = temp_dir.path().join(skill_name);
    std::fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_md,
        format!(
            r#"---
name: {}
description: Test skill for {} testing
version: 1.0.0
tags: [test]
capabilities: [test_capability]
---
# {}
"#,
            skill_name, skill_name, skill_name
        ),
    )
    .unwrap();

    skill_dir
}


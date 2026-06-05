#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests for project lock determinism (RFC-056).
//!
//! Covers acceptance criteria:
//! - AC 4: skills.lock MUST NOT contain `generated_at`
//! - AC 5: skills.lock MUST NOT contain `fetched_at`
//! - AC 6: byte-identical output across saves with the same input
//! - AC 7: entries sorted ascending by id

use chrono::Utc;
use fastskill_core::core::lock::ProjectSkillsLock;
use fastskill_core::core::service::SkillId;
use fastskill_core::core::skill_manager::{SkillDefinition, SourceType};
use std::fs;
use tempfile::TempDir;

fn make_skill(id: &str) -> SkillDefinition {
    SkillDefinition {
        id: SkillId::new(id.to_string()).unwrap(),
        name: id.to_string(),
        description: "test".to_string(),
        version: "1.0.0".to_string(),
        author: None,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        skill_file: std::path::PathBuf::from("SKILL.md"),
        reference_files: None,
        script_files: None,
        asset_files: None,
        execution_environment: None,
        dependencies: None,
        timeout: None,
        source_url: Some("https://github.com/test/repo.git".to_string()),
        source_type: Some(SourceType::GitUrl),
        source_branch: Some("main".to_string()),
        source_tag: None,
        source_subdir: None,
        installed_from: None,
        commit_hash: Some("abc123".to_string()),
        fetched_at: Some(Utc::now()),
        editable: false,
    }
}

#[test]
fn project_lock_double_save_is_byte_identical() {
    // AC 6: byte-identical output across saves with the same input.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let mut lock = ProjectSkillsLock::new_empty();
    lock.update_skill(&make_skill("alpha"));
    lock.update_skill(&make_skill("beta"));
    lock.save_to_file(&lock_path).unwrap();

    let first = fs::read(&lock_path).unwrap();

    // Second save with the same data must be byte-identical.
    lock.save_to_file(&lock_path).unwrap();
    let second = fs::read(&lock_path).unwrap();

    assert_eq!(
        first, second,
        "double-save must produce byte-identical content"
    );
}

#[test]
fn project_lock_entries_sorted_alphabetically_after_save() {
    // AC 7: After adding zebra, alpha, mango (in that order), file lists alpha, mango, zebra.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let mut lock = ProjectSkillsLock::new_empty();
    lock.update_skill(&make_skill("zebra"));
    lock.update_skill(&make_skill("alpha"));
    lock.update_skill(&make_skill("mango"));
    lock.save_to_file(&lock_path).unwrap();

    let content = fs::read_to_string(&lock_path).unwrap();
    let mut ids: Vec<&str> = Vec::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("id = ") {
            ids.push(rest.trim_matches('"'));
        }
    }

    assert_eq!(ids, vec!["alpha", "mango", "zebra"]);
}

#[test]
fn project_lock_omits_volatile_fields() {
    // AC 4 + AC 5: serialized lock has no generated_at or fetched_at.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let mut lock = ProjectSkillsLock::new_empty();
    lock.update_skill(&make_skill("my-skill"));
    lock.save_to_file(&lock_path).unwrap();

    let content = fs::read_to_string(&lock_path).unwrap();
    assert!(
        !content.contains("generated_at"),
        "skills.lock must not contain generated_at"
    );
    assert!(
        !content.contains("fetched_at"),
        "skills.lock must not contain fetched_at"
    );

    // Validate via TOML parser too — defensive against accidental matches.
    let parsed: toml::Value = toml::from_str(&content).unwrap();
    if let Some(metadata) = parsed.get("metadata") {
        assert!(metadata.get("generated_at").is_none());
        assert!(metadata.get("version").is_some());
    }
    if let Some(skills) = parsed.get("skills").and_then(|s| s.as_array()) {
        for skill in skills {
            assert!(skill.get("fetched_at").is_none());
        }
    }
}

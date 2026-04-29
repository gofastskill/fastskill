#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests for two-scope lock routing (RFC-056).
//!
//! Covers acceptance criteria:
//! - AC 1: global add writes a `global-skills.lock` entry with `installed_at`
//!   and does not touch the project lock.
//! - AC 2: removing a global skill removes its entry from the global lock.
//! - AC 3: `update --check --global` updates `last_checked_at` and not the project lock.
//! - AC 8 / AC 9: project and global scopes don't cross-contaminate each other's lock.
//!
//! These tests exercise the lock APIs directly with isolated temp paths so they
//! never touch the user's real `~/.config/fastskill/global-skills.lock`.

use chrono::Utc;
use fastskill_core::core::lock::{GlobalSkillsLock, ProjectSkillsLock};
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
        commit_hash: None,
        fetched_at: Some(Utc::now()),
        editable: false,
    }
}

#[test]
fn global_upsert_writes_installed_at_and_does_not_touch_project_lock() {
    // AC 1 + AC 8: a global upsert writes the global lock; the project lock at
    // a separate path is unaffected.
    let tmp = TempDir::new().unwrap();
    let global_path = tmp.path().join("global-skills.lock");
    let project_path = tmp.path().join("skills.lock");

    // Seed an existing project lock to verify it does NOT change.
    let project = ProjectSkillsLock::new_empty();
    project.save_to_file(&project_path).unwrap();
    let project_before = fs::read(&project_path).unwrap();

    let mut global = GlobalSkillsLock::new_empty();
    global.upsert_skill(&make_skill("globally-installed"), Utc::now());
    global.save_to_file(&global_path).unwrap();

    let global_content = fs::read_to_string(&global_path).unwrap();
    assert!(global_content.contains("globally-installed"));
    assert!(
        global_content.contains("installed_at"),
        "global lock must record installed_at: {}",
        global_content
    );

    let project_after = fs::read(&project_path).unwrap();
    assert_eq!(
        project_before, project_after,
        "global upsert must not alter the project lock"
    );
}

#[test]
fn global_remove_drops_entry_from_lock() {
    // AC 2: after remove --global, the entry is gone.
    let tmp = TempDir::new().unwrap();
    let global_path = tmp.path().join("global-skills.lock");

    let mut global = GlobalSkillsLock::new_empty();
    global.upsert_skill(&make_skill("removable"), Utc::now());
    global.save_to_file(&global_path).unwrap();
    assert!(fs::read_to_string(&global_path)
        .unwrap()
        .contains("removable"));

    let mut global = GlobalSkillsLock::load_from_file(&global_path).unwrap();
    let removed = global.remove_skill("removable");
    assert!(removed, "remove_skill must report success");
    global.save_to_file(&global_path).unwrap();

    let after = fs::read_to_string(&global_path).unwrap();
    assert!(
        !after.contains("removable"),
        "entry must be absent from global lock after remove"
    );
}

#[test]
fn global_check_records_last_checked_at_only() {
    // AC 3: update --check --global updates last_checked_at without
    // changing other operational fields.
    let tmp = TempDir::new().unwrap();
    let global_path = tmp.path().join("global-skills.lock");

    let install_time = Utc::now();
    let mut global = GlobalSkillsLock::new_empty();
    global.upsert_skill(&make_skill("watched"), install_time);
    global.save_to_file(&global_path).unwrap();

    let mut global = GlobalSkillsLock::load_from_file(&global_path).unwrap();
    assert!(global.skills[0].last_checked_at.is_none());
    assert!(global.skills[0].last_updated_at.is_none());

    let check_time = Utc::now();
    global.mark_checked("watched", check_time);
    global.save_to_file(&global_path).unwrap();

    let reloaded = GlobalSkillsLock::load_from_file(&global_path).unwrap();
    assert_eq!(reloaded.skills[0].last_checked_at, Some(check_time));
    assert!(
        reloaded.skills[0].last_updated_at.is_none(),
        "check mode must not bump last_updated_at"
    );
}

#[test]
fn project_lock_does_not_carry_global_only_fields() {
    // AC 9: project lock must never persist the operational timestamps that
    // belong to GlobalLockedSkillEntry (installed_at / last_checked_at / last_updated_at).
    let tmp = TempDir::new().unwrap();
    let project_path = tmp.path().join("skills.lock");

    let mut project = ProjectSkillsLock::new_empty();
    project.update_skill(&make_skill("project-only"));
    project.save_to_file(&project_path).unwrap();

    let content = fs::read_to_string(&project_path).unwrap();
    for forbidden in ["installed_at", "last_checked_at", "last_updated_at"] {
        assert!(
            !content.contains(forbidden),
            "project lock must not contain {}: {}",
            forbidden,
            content
        );
    }
}

#[test]
fn global_and_project_locks_track_independent_skill_sets() {
    // AC 8 + AC 9 (cross): independent writes to each lock at distinct paths
    // never leak into the other.
    let tmp = TempDir::new().unwrap();
    let project_path = tmp.path().join("skills.lock");
    let global_path = tmp.path().join("global-skills.lock");

    let mut project = ProjectSkillsLock::new_empty();
    project.update_skill(&make_skill("project-skill"));
    project.save_to_file(&project_path).unwrap();

    let mut global = GlobalSkillsLock::new_empty();
    global.upsert_skill(&make_skill("global-skill"), Utc::now());
    global.save_to_file(&global_path).unwrap();

    let project_content = fs::read_to_string(&project_path).unwrap();
    let global_content = fs::read_to_string(&global_path).unwrap();

    assert!(project_content.contains("project-skill"));
    assert!(!project_content.contains("global-skill"));
    assert!(global_content.contains("global-skill"));
    assert!(!global_content.contains("project-skill"));
}

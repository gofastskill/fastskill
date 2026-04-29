#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Integration tests for v1.0.0 → v2.0 lock migration (RFC-056).
//!
//! Covers acceptance criteria:
//! - AC 11: loading a v1.0.0 lock with `generated_at` / `fetched_at` and immediately
//!   re-saving it MUST strip those fields.
//! - AC 12: the `SkillsLock` type alias continues to compile.
//! - AC 18: v1.0.0 format files load without error (backward compat).

use fastskill_core::core::lock::{ProjectSkillsLock, SkillsLock};
use std::fs;
use tempfile::TempDir;

const V1_LOCK_FIXTURE: &str = r#"[metadata]
version = "1.0.0"
generated_at = "2026-04-28T10:00:00Z"
fastskill_version = "0.9.100"

[[skills]]
id = "legacy-skill"
name = "Legacy Skill"
version = "1.0.0"
fetched_at = "2026-04-28T10:00:00Z"
source = { type = "source", name = "default", skill = "legacy-skill", version = "1.0.0" }
dependencies = []
groups = []
editable = false
depth = 0
"#;

#[test]
fn migration_strips_volatile_fields_on_resave() {
    // AC 11: re-saving a v1 lock removes generated_at and fetched_at.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    fs::write(&lock_path, V1_LOCK_FIXTURE).unwrap();

    let loaded = ProjectSkillsLock::load_from_file(&lock_path)
        .expect("v1.0.0 lock must load (extra fields ignored via serde defaults)");
    assert_eq!(loaded.skills.len(), 1);
    assert_eq!(loaded.skills[0].id, "legacy-skill");

    loaded.save_to_file(&lock_path).unwrap();
    let migrated = fs::read_to_string(&lock_path).unwrap();

    assert!(
        !migrated.contains("generated_at"),
        "generated_at must be stripped"
    );
    assert!(
        !migrated.contains("fetched_at"),
        "fetched_at must be stripped"
    );
    assert!(
        migrated.contains("legacy-skill"),
        "skill data must be preserved"
    );
    assert!(
        migrated.contains("fastskill_version"),
        "fastskill_version must remain"
    );
}

#[test]
fn v1_lock_loads_without_error() {
    // AC 18: backward compat — v1.0.0 file loads cleanly.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    fs::write(&lock_path, V1_LOCK_FIXTURE).unwrap();

    let lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(lock.skills.len(), 1);
    assert_eq!(lock.skills[0].id, "legacy-skill");
    assert_eq!(lock.skills[0].version, "1.0.0");
}

#[test]
fn skills_lock_type_alias_round_trips() {
    // AC 12: type alias compiles and behaves like ProjectSkillsLock.
    fn accepts_alias(_lock: &SkillsLock) {}

    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let lock = SkillsLock::new_empty();
    lock.save_to_file(&lock_path).unwrap();
    let loaded = SkillsLock::load_from_file(&lock_path).unwrap();

    accepts_alias(&loaded);
    assert!(loaded.skills.is_empty());
    assert_eq!(loaded.metadata.version, "2.0");
}

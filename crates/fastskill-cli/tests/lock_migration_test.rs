#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Integration tests for the lock format version gate (Phase 0 / Origin model).
//!
//! Formerly (RFC-056) this file covered v1.0.0 → v2.0 migration:
//! - AC 11: loading a v1.0.0 lock with `generated_at` / `fetched_at` and immediately
//!   re-saving it stripped those fields.
//! - AC 18: v1.0.0 format files loaded without error (backward compat).
//!
//! The Origin/Resolved reshape (ADR-0005) removed the migrator entirely: any lock
//! whose `metadata.version` isn't the current `LOCK_FORMAT_VERSION` ("3.0") is now
//! rejected with an actionable `LockError::UnsupportedVersion` telling the caller to
//! delete the lock file and re-run `fastskill install`. These tests now assert that
//! rejection instead of a successful migration.

use fastskill_core::core::lock::{LockError, ProjectSkillsLock, LOCK_FORMAT_VERSION};
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
fn pre_origin_lock_is_rejected_not_migrated() {
    // Superseded AC 11: there is no migrator anymore. A v1.0.0 lock (predating the
    // Origin/Resolved reshape) must be rejected with an actionable error rather
    // than silently loaded, stripped, and re-saved.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    fs::write(&lock_path, V1_LOCK_FIXTURE).unwrap();

    let err = ProjectSkillsLock::load_from_file(&lock_path).unwrap_err();
    match err {
        LockError::UnsupportedVersion { found } => assert_eq!(found, "1.0.0"),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn v1_lock_is_rejected_before_touching_skill_shape() {
    // Superseded AC 18: backward compat with pre-3.0 formats is intentionally
    // dropped. The lightweight version pre-check must reject the file before any
    // attempt to deserialize the (now-incompatible) `[[skills]]` shape.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    fs::write(&lock_path, V1_LOCK_FIXTURE).unwrap();

    let result = ProjectSkillsLock::load_from_file(&lock_path);
    assert!(matches!(result, Err(LockError::UnsupportedVersion { .. })));
}

#[test]
fn project_skills_lock_round_trips() {
    // Verify ProjectSkillsLock can be written and reloaded cleanly at the current
    // format version (the Origin/Resolved reshape bumped this to "3.0").
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let lock = ProjectSkillsLock::new_empty();
    lock.save_to_file(&lock_path).unwrap();
    let loaded = ProjectSkillsLock::load_from_file(&lock_path).unwrap();

    assert!(loaded.skills.is_empty());
    assert_eq!(loaded.metadata.version, LOCK_FORMAT_VERSION);
}

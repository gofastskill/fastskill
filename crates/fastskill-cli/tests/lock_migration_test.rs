#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Integration tests for the lock format version gate (Phase 0 / Origin model).
//!
//! Formerly (RFC-056) this file covered v1.0.0 → v2.0 migration:
//! - AC 11: loading a v1.0.0 lock with `generated_at` / `fetched_at` and immediately
//!   re-saving it stripped those fields.
//! - AC 18: v1.0.0 format files loaded without error (backward compat).
//!
//! The Origin/Resolved reshape (ADR-0005) removed the migrator entirely: any lock whose
//! `metadata.version` wasn't the current `LOCK_FORMAT_VERSION` ("3.0") was rejected with
//! an actionable `LockError::UnsupportedVersion` telling the caller to delete the lock and
//! re-run `fastskill install`.
//!
//! That rejection has now been REVERSED for the v1.0.0 project lock, and these tests
//! assert migration again. The reason is specific to what a lock is: it records *pinned*
//! versions. "Delete it and re-run install" re-resolves every dependency against whatever
//! is current, which silently upgrades a working installation — a real cost that the
//! reshape's clean break did not account for. Rejection is still correct for any version
//! we have no specimen of, and for the global lock, which is untouched here.

use fastskill_core::core::lock::{LockError, ProjectSkillsLock, LOCK_FORMAT_VERSION};
use fastskill_core::core::origin::Origin;
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
fn pre_origin_lock_is_migrated_not_rejected() {
    // Restores AC 11's intent: a v1.0.0 lock loads, and its pins survive. The fixture's
    // entry is a legacy `source = "source"` (a configured-repository install), which is
    // now `Origin::Repository` -- including the version constraint the source table
    // carried, since dropping it would turn a pinned dependency into "newest allowed".
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    fs::write(&lock_path, V1_LOCK_FIXTURE).unwrap();

    let lock = ProjectSkillsLock::load_from_file(&lock_path)
        .expect("a v1.0.0 project lock must now migrate rather than fail");

    assert_eq!(lock.metadata.version, LOCK_FORMAT_VERSION);
    assert_eq!(lock.skills.len(), 1);

    let entry = &lock.skills[0];
    assert_eq!(entry.id, "legacy-skill");
    assert_eq!(entry.name, "Legacy Skill");
    // The pinned version is the whole point of migrating rather than re-resolving.
    assert_eq!(entry.resolved.version, "1.0.0");
    match &entry.origin {
        Origin::Repository {
            repo,
            skill,
            version,
        } => {
            assert_eq!(repo, "default");
            assert_eq!(skill, "legacy-skill");
            assert!(
                version.is_some(),
                "the requested constraint must survive migration"
            );
        }
        other => panic!("expected a repository origin, got {other:?}"),
    }
    // v1.0.0 never recorded a commit hash, so absent is honest rather than lossy.
    assert!(entry.resolved.commit_hash.is_none());
}

#[test]
fn loading_a_v1_lock_does_not_rewrite_it() {
    // Replaces AC 18's rejection check. Backward compat is restored, but reading must
    // stay read-only: a legacy lock keeps working untouched until something saves it
    // for its own reasons. Rewriting on read would mutate a file the user never asked
    // to change, and would do it on every command.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    fs::write(&lock_path, V1_LOCK_FIXTURE).unwrap();

    let _ = ProjectSkillsLock::load_from_file(&lock_path).unwrap();

    let on_disk = fs::read_to_string(&lock_path).unwrap();
    assert!(
        on_disk.contains("version = \"1.0.0\""),
        "load must leave the file alone, got:\n{on_disk}"
    );
}

#[test]
fn a_lock_version_we_have_no_specimen_of_is_still_rejected() {
    // Migration is only safe where the shape is known. Anything else -- including a
    // version newer than this build -- must still fail loudly rather than be guessed at.
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    fs::write(&lock_path, "[metadata]\nversion = \"2.0\"\n").unwrap();

    match ProjectSkillsLock::load_from_file(&lock_path) {
        Err(LockError::UnsupportedVersion { found }) => assert_eq!(found, "2.0"),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
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

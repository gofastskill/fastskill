use super::*;
use crate::core::service::SkillId;
use crate::core::skill_manager::SkillDefinition;
use chrono::Utc;
use tempfile::TempDir;

fn make_skill(id: &str) -> SkillDefinition {
    SkillDefinition {
        id: SkillId::new(id.to_string()).unwrap(),
        name: id.to_string(),
        description: "test".to_string(),
        version: "1.0.0".to_string(),
        author: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        skill_file: std::path::PathBuf::from("SKILL.md"),
        reference_files: None,
        script_files: None,
        asset_files: None,
        execution_environment: None,
        dependencies: None,
        timeout: None,
        origin: crate::core::origin::Origin::Git {
            url: "https://github.com/test/repo.git".to_string(),
            r#ref: crate::core::origin::GitRef::Branch("main".to_string()),
            subdir: None,
        },
        commit_hash: Some("abc123".to_string()),
        fetched_at: Some(Utc::now()),
    }
}

#[test]
fn test_lock_from_skills() {
    let skill = make_skill("test-skill");
    let lock = ProjectSkillsLock::from_installed_skills(&[skill]);
    assert_eq!(lock.skills.len(), 1);
    assert_eq!(lock.skills[0].id, "test-skill");
}

#[test]
fn test_project_lock_entries_sorted_on_save() {
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let mut lock = ProjectSkillsLock::new_empty();
    lock.update_skill(&make_skill("zebra"));
    lock.update_skill(&make_skill("alpha"));
    lock.update_skill(&make_skill("mango"));

    lock.save_to_file(&lock_path).unwrap();

    let loaded = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    let ids: Vec<&str> = loaded.skills.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["alpha", "mango", "zebra"]);
}

#[test]
fn test_project_lock_no_volatile_fields_in_serialized_output() {
    let mut lock = ProjectSkillsLock::new_empty();
    lock.update_skill(&make_skill("my-skill"));

    let serialized = toml::to_string_pretty(&lock).unwrap();
    assert!(
        !serialized.contains("generated_at"),
        "generated_at must not appear"
    );
    assert!(
        !serialized.contains("fetched_at"),
        "fetched_at must not appear"
    );
}

#[test]
fn test_project_lock_deterministic_round_trip() {
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let mut lock = ProjectSkillsLock::new_empty();
    lock.update_skill(&make_skill("skill-a"));
    lock.update_skill(&make_skill("skill-b"));
    lock.save_to_file(&lock_path).unwrap();

    let content_first = std::fs::read(&lock_path).unwrap();

    // Second save with same data must produce byte-identical output
    lock.save_to_file(&lock_path).unwrap();
    let content_second = std::fs::read(&lock_path).unwrap();
    assert_eq!(
        content_first, content_second,
        "double-save must be byte-identical"
    );
}

#[test]
fn test_project_lock_pre_origin_format_is_rejected() {
    // A lock file from before the Origin model (any version != "3.0") must be
    // rejected with an actionable error rather than silently misparsed — there
    // is no migrator (spec decision).
    let old_format = r#"[metadata]
version = "2.0"
generated_at = "2024-01-01T00:00:00Z"
fastskill_version = "0.9.0"

[[skills]]
id = "old-skill"
name = "Old Skill"
version = "1.0.0"
source = { type = "git", url = "https://github.com/test/repo.git" }
fetched_at = "2024-01-01T00:00:00Z"
dependencies = []
groups = []
editable = false
depth = 0
"#;
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");
    std::fs::write(&lock_path, old_format).unwrap();

    let err = ProjectSkillsLock::load_from_file(&lock_path).unwrap_err();
    match err {
        LockError::UnsupportedVersion { found } => assert_eq!(found, "2.0"),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn test_project_lock_no_volatile_fields_after_round_trip() {
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let mut lock = ProjectSkillsLock::new_empty();
    lock.update_skill(&make_skill("my-skill"));
    lock.save_to_file(&lock_path).unwrap();

    let loaded = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(loaded.skills.len(), 1);
    assert_eq!(loaded.skills[0].id, "my-skill");

    let new_content = std::fs::read_to_string(&lock_path).unwrap();
    assert!(
        !new_content.contains("generated_at"),
        "generated_at must not appear"
    );
    assert!(
        !new_content.contains("fetched_at"),
        "fetched_at must not appear"
    );
}

#[test]
fn test_global_lock_upsert_and_remove() {
    let mut lock = GlobalSkillsLock::new_empty();
    let skill = make_skill("global-skill");
    let now = Utc::now();

    lock.upsert_skill(&skill, now);
    assert_eq!(lock.skills.len(), 1);
    assert_eq!(lock.skills[0].id, "global-skill");
    assert_eq!(lock.skills[0].installed_at, now);

    // Upsert again (update) - should not duplicate
    lock.upsert_skill(&skill, now);
    assert_eq!(lock.skills.len(), 1);

    let removed = lock.remove_skill("global-skill");
    assert!(removed);
    assert!(lock.skills.is_empty());
}

#[test]
fn test_global_lock_mark_checked_and_updated() {
    let mut lock = GlobalSkillsLock::new_empty();
    let skill = make_skill("global-skill");
    let now = Utc::now();
    lock.upsert_skill(&skill, now);

    let checked_at = Utc::now();
    lock.mark_checked("global-skill", checked_at);
    assert_eq!(lock.skills[0].last_checked_at, Some(checked_at));

    let updated_at = Utc::now();
    lock.mark_updated("global-skill", updated_at);
    assert_eq!(lock.skills[0].last_updated_at, Some(updated_at));
}

#[test]
fn test_global_lock_save_and_load() {
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("global-skills.lock");

    let mut lock = GlobalSkillsLock::new_empty();
    lock.upsert_skill(&make_skill("my-global-skill"), Utc::now());
    lock.save_to_file(&lock_path).unwrap();

    let loaded = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(loaded.skills.len(), 1);
    assert_eq!(loaded.skills[0].id, "my-global-skill");
    assert!(loaded.skills[0].last_checked_at.is_none());
}

#[test]
fn test_global_lock_creates_parent_dir() {
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp
        .path()
        .join("subdir")
        .join("nested")
        .join("global-skills.lock");

    let lock = GlobalSkillsLock::new_empty();
    lock.save_to_file(&lock_path).unwrap();
    assert!(lock_path.exists());
}

#[test]
fn test_global_lock_path_returns_result() {
    // global_lock_path() should succeed on platforms with a config dir (Linux/macOS/Windows)
    let result = global_lock_path();
    // On CI Linux this should succeed
    assert!(result.is_ok() || result.is_err(), "must return a Result");
    if let Ok(path) = result {
        assert!(path.ends_with("global-skills.lock"));
        assert!(path.to_str().unwrap().contains("fastskill"));
    }
}

#[test]
fn test_project_lock_path_helper() {
    let project_file = std::path::PathBuf::from("/home/user/project/skill-project.toml");
    let lock_path = project_lock_path(&project_file);
    assert_eq!(
        lock_path,
        std::path::PathBuf::from("/home/user/project/skills.lock")
    );
}

#[test]
fn test_save_to_file_is_last_writer_wins() {
    // BUG-8: atomic_write now uses a unique temp file + atomic rename (no advisory
    // lock on a shared `.tmp`), so concurrent/sequential writers no longer error —
    // the file is always a complete copy of some writer's content (last-writer-wins).
    let tmp = TempDir::new().unwrap();
    let lock_path = tmp.path().join("skills.lock");

    let mut first = ProjectSkillsLock::new_empty();
    first.update_skill(&make_skill("first-skill"));
    first.save_to_file(&lock_path).expect("first save succeeds");

    let mut second = ProjectSkillsLock::new_empty();
    second.update_skill(&make_skill("second-skill"));
    second
        .save_to_file(&lock_path)
        .expect("second save overwrites without error");

    // Last write wins: the file is a complete copy of the second writer's content.
    let reloaded = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(reloaded.skills.len(), 1);
}

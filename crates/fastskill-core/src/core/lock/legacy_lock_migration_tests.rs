use super::*;

const LEGACY: &str = r#"
[metadata]
version = "1.0.0"
fastskill_version = "0.9.136"

[[skills]]
id = "adapt-to-ai"
name = "adapt-to-ai"
version = "1.0.2"
source_url = "https://github.com/org/skills"
source_branch = "main"
dependencies = []
groups = ["docs"]
editable = false
depth = 0

[skills.source]
type = "git"
url = "https://github.com/org/skills"
branch = "main"

[[skills]]
id = "pinned-default"
name = "pinned-default"
version = "2.0.0"
source_url = "https://github.com/org/other"
dependencies = []
groups = []
editable = false
depth = 1

[skills.source]
type = "git"
url = "https://github.com/org/other"

[[skills]]
id = "win"
name = "win"
version = "0.3.0"
source_url = "/srv/skills/win"
dependencies = []
groups = []
editable = true
depth = 0

[skills.source]
type = "local"
path = "/srv/skills/win"
editable = true
"#;

fn entry<'a>(lock: &'a ProjectSkillsLock, id: &str) -> &'a ProjectLockedSkillEntry {
    lock.skills
        .iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("missing entry {id}"))
}

fn load(content: &str) -> ProjectSkillsLock {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skills.lock");
    std::fs::write(&path, content).unwrap();
    ProjectSkillsLock::load_from_file(&path).expect("legacy lock must load")
}

#[test]
fn legacy_lock_is_migrated_instead_of_rejected() {
    let lock = load(LEGACY);
    assert_eq!(lock.metadata.version, LOCK_FORMAT_VERSION);
    assert_eq!(lock.skills.len(), 3);
}

#[test]
fn pinned_versions_are_preserved() {
    // The entire reason to migrate rather than tell people to delete the file.
    let lock = load(LEGACY);
    assert_eq!(entry(&lock, "adapt-to-ai").resolved.version, "1.0.2");
    assert_eq!(entry(&lock, "pinned-default").resolved.version, "2.0.0");
    assert_eq!(entry(&lock, "win").resolved.version, "0.3.0");
}

#[test]
fn git_branch_and_absent_branch_are_distinguished() {
    let lock = load(LEGACY);
    match &entry(&lock, "adapt-to-ai").origin {
        Origin::Git { url, r#ref, .. } => {
            assert_eq!(url, "https://github.com/org/skills");
            assert_eq!(*r#ref, GitRef::Branch("main".to_string()));
        }
        other => panic!("expected git origin, got {other:?}"),
    }
    // No branch meant the repository default — treating it as a branch named "main"
    // would silently re-point the dependency.
    match &entry(&lock, "pinned-default").origin {
        Origin::Git { r#ref, .. } => assert_eq!(*r#ref, GitRef::Default),
        other => panic!("expected git origin, got {other:?}"),
    }
}

#[test]
fn local_entries_keep_path_and_editable() {
    let lock = load(LEGACY);
    match &entry(&lock, "win").origin {
        Origin::Local { path, editable } => {
            assert_eq!(path, &PathBuf::from("/srv/skills/win"));
            assert!(*editable);
        }
        other => panic!("expected local origin, got {other:?}"),
    }
}

#[test]
fn groups_and_depth_survive() {
    let lock = load(LEGACY);
    assert_eq!(entry(&lock, "adapt-to-ai").groups, vec!["docs".to_string()]);
    assert_eq!(entry(&lock, "pinned-default").depth, 1);
}

/// 1.0.0 never recorded these, so `None` is honest rather than lossy.
#[test]
fn commit_hash_and_checksum_are_absent_not_invented() {
    let lock = load(LEGACY);
    let e = entry(&lock, "adapt-to-ai");
    assert!(e.resolved.commit_hash.is_none());
    assert!(e.resolved.checksum.is_none());
}

#[test]
fn migrated_lock_round_trips_through_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skills.lock");
    std::fs::write(&path, LEGACY).unwrap();

    let migrated = ProjectSkillsLock::load_from_file(&path).unwrap();
    migrated.save_to_file(&path).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("version = \"3.0\""),
        "not stamped:\n{written}"
    );
    assert!(
        !written.contains("source_url"),
        "legacy fields survived:\n{written}"
    );

    let reloaded = ProjectSkillsLock::load_from_file(&path).unwrap();
    assert_eq!(reloaded.skills.len(), 3);
    assert_eq!(entry(&reloaded, "win").resolved.version, "0.3.0");
}

/// Reading must not rewrite: a legacy lock keeps working until something saves it.
#[test]
fn loading_does_not_rewrite_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skills.lock");
    std::fs::write(&path, LEGACY).unwrap();
    let _ = ProjectSkillsLock::load_from_file(&path).unwrap();
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains("version = \"1.0.0\""),
        "load rewrote the file"
    );
}

/// A version we have never seen is still refused — migrating would be guesswork.
#[test]
fn unknown_lock_version_is_still_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skills.lock");
    std::fs::write(&path, "[metadata]\nversion = \"2.0\"\n").unwrap();
    match ProjectSkillsLock::load_from_file(&path) {
        Err(LockError::UnsupportedVersion { found }) => assert_eq!(found, "2.0"),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn unknown_source_type_is_reported_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skills.lock");
    std::fs::write(
        &path,
        "[metadata]\nversion = \"1.0.0\"\n\n[[skills]]\nid = \"weird\"\nname = \"weird\"\n\
             version = \"1.0.0\"\n\n[skills.source]\ntype = \"telepathy\"\n",
    )
    .unwrap();
    let err = ProjectSkillsLock::load_from_file(&path).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("weird") && msg.contains("telepathy"),
        "unhelpful: {msg}"
    );
}

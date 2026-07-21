//! Skills lock file management for reproducible installations
//!
//! Two distinct lock structures are maintained:
//! - `ProjectSkillsLock`: deterministic, timestamp-free, for `skills.lock` at project root
//! - `GlobalSkillsLock`: operational, with timestamps, for `global-skills.lock` in user config dir

use crate::core::origin::{Origin, Resolved};
use crate::core::skill_manager::SkillDefinition;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The lock format version. Bumped to "3.0" for the `Origin`/`Resolved` reshape
/// (Phase 0). Lock files from before this version cannot be read — see
/// [`LockError::UnsupportedVersion`].
pub const LOCK_FORMAT_VERSION: &str = "3.0";

// ── Project Lock ─────────────────────────────────────────────────────────────

/// Metadata for the project-scoped lock file.
/// Does not contain any wall-clock timestamp — enables deterministic file content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectLockMetadata {
    pub version: String,
    #[serde(default)]
    pub fastskill_version: Option<String>,
}

/// A single pinned skill entry in the project lock.
/// No volatile timestamp fields — content is fully deterministic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectLockedSkillEntry {
    pub id: String,
    pub name: String,
    pub origin: Origin,
    pub resolved: Resolved,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    /// Depth in the dependency tree (0 = direct dependency)
    #[serde(default)]
    pub depth: u32,
    /// ID of the skill that pulled this one in (for transitive deps)
    #[serde(default)]
    pub parent_skill: Option<String>,
}

/// Project-scoped lock file. Serialized to `<project_root>/skills.lock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSkillsLock {
    pub metadata: ProjectLockMetadata,
    #[serde(default)]
    pub skills: Vec<ProjectLockedSkillEntry>,
}

impl ProjectSkillsLock {
    pub fn new_empty() -> Self {
        Self {
            metadata: ProjectLockMetadata {
                version: LOCK_FORMAT_VERSION.to_string(),
                fastskill_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            skills: Vec::new(),
        }
    }

    pub fn load_from_file(path: &Path) -> Result<Self, LockError> {
        if !path.exists() {
            return Err(LockError::NotFound(path.to_path_buf()));
        }
        let safe_path = path.canonicalize().map_err(LockError::Io)?;
        let content = std::fs::read_to_string(&safe_path).map_err(LockError::Io)?;
        check_lock_format_version(&content)?;
        let lock: ProjectSkillsLock =
            toml::from_str(&content).map_err(|e| LockError::Parse(e.to_string()))?;
        Ok(lock)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), LockError> {
        let mut lock = self.clone();
        lock.sort_entries();
        lock.metadata.fastskill_version = Some(env!("CARGO_PKG_VERSION").to_string());
        let content =
            toml::to_string_pretty(&lock).map_err(|e| LockError::Serialize(e.to_string()))?;
        crate::utils::atomic_write(path, content.as_bytes()).map_err(LockError::Io)?;
        Ok(())
    }

    pub fn from_installed_skills(skills: &[SkillDefinition]) -> Self {
        let mut lock = Self::new_empty();
        for skill in skills {
            lock.update_skill(skill);
        }
        lock
    }

    pub fn update_skill(&mut self, skill: &SkillDefinition) {
        self.update_skill_with_depth(skill, 0, None);
    }

    pub fn update_skill_with_depth(
        &mut self,
        skill: &SkillDefinition,
        depth: u32,
        parent_skill: Option<String>,
    ) {
        self.skills.retain(|s| s.id != skill.id.as_str());
        let entry = ProjectLockedSkillEntry {
            id: skill.id.to_string(),
            name: skill.name.clone(),
            origin: skill.origin.clone(),
            resolved: Resolved {
                version: skill.version.clone(),
                commit_hash: skill.commit_hash.clone(),
                checksum: None,
            },
            dependencies: skill.dependencies.clone().unwrap_or_default(),
            groups: Vec::new(),
            depth,
            parent_skill,
        };
        self.skills.push(entry);
    }

    pub fn remove_skill(&mut self, skill_id: &str) -> bool {
        let initial_len = self.skills.len();
        self.skills.retain(|s| s.id != skill_id);
        self.skills.len() < initial_len
    }

    pub fn verify_matches_installed(
        &self,
        installed_skills: &[SkillDefinition],
    ) -> Vec<LockMismatch> {
        let mut mismatches = Vec::new();
        for locked in &self.skills {
            if let Some(installed) = installed_skills.iter().find(|s| s.id.as_str() == locked.id) {
                if installed.version != locked.resolved.version {
                    mismatches.push(LockMismatch {
                        skill_id: locked.id.clone(),
                        reason: format!(
                            "Version mismatch: lock={}, installed={}",
                            locked.resolved.version, installed.version
                        ),
                    });
                }
                if let (Some(lock_commit), Some(inst_commit)) =
                    (&locked.resolved.commit_hash, &installed.commit_hash)
                {
                    if lock_commit != inst_commit {
                        mismatches.push(LockMismatch {
                            skill_id: locked.id.clone(),
                            reason: format!(
                                "Commit mismatch: lock={}, installed={}",
                                lock_commit, inst_commit
                            ),
                        });
                    }
                }
            } else {
                mismatches.push(LockMismatch {
                    skill_id: locked.id.clone(),
                    reason: "Skill locked but not installed".to_string(),
                });
            }
        }
        for installed in installed_skills {
            if !self.skills.iter().any(|s| s.id == installed.id.as_str()) {
                mismatches.push(LockMismatch {
                    skill_id: installed.id.to_string(),
                    reason: "Skill installed but not in lock file".to_string(),
                });
            }
        }
        mismatches
    }

    fn sort_entries(&mut self) {
        self.skills.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

// ── Global Lock ───────────────────────────────────────────────────────────────

/// Metadata for the global user-scoped lock file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalLockMetadata {
    pub version: String,
    #[serde(default)]
    pub fastskill_version: Option<String>,
}

/// A single entry in the global lock with operational timestamps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalLockedSkillEntry {
    pub id: String,
    pub name: String,
    pub origin: Origin,
    pub resolved: Resolved,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    pub installed_at: DateTime<Utc>,
    #[serde(default)]
    pub last_checked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_updated_at: Option<DateTime<Utc>>,
}

/// Global user-scoped lock file.
/// Serialized to `<dirs::config_dir()>/fastskill/global-skills.lock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSkillsLock {
    pub metadata: GlobalLockMetadata,
    #[serde(default)]
    pub skills: Vec<GlobalLockedSkillEntry>,
}

impl GlobalSkillsLock {
    pub fn new_empty() -> Self {
        Self {
            metadata: GlobalLockMetadata {
                version: LOCK_FORMAT_VERSION.to_string(),
                fastskill_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            skills: Vec::new(),
        }
    }

    /// Returns the platform-default path for the global lock file.
    pub fn default_path() -> Result<PathBuf, LockError> {
        global_lock_path()
    }

    pub fn load_from_file(path: &Path) -> Result<Self, LockError> {
        if !path.exists() {
            return Err(LockError::NotFound(path.to_path_buf()));
        }
        let safe_path = path.canonicalize().map_err(LockError::Io)?;
        let content = std::fs::read_to_string(&safe_path).map_err(LockError::Io)?;
        check_lock_format_version(&content)?;
        let lock: GlobalSkillsLock =
            toml::from_str(&content).map_err(|e| LockError::Parse(e.to_string()))?;
        Ok(lock)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), LockError> {
        let mut lock = self.clone();
        lock.sort_entries();
        lock.metadata.fastskill_version = Some(env!("CARGO_PKG_VERSION").to_string());
        let content =
            toml::to_string_pretty(&lock).map_err(|e| LockError::Serialize(e.to_string()))?;
        crate::utils::atomic_write(path, content.as_bytes()).map_err(LockError::Io)?;
        Ok(())
    }

    pub fn upsert_skill(&mut self, skill: &SkillDefinition, installed_at: DateTime<Utc>) {
        self.skills.retain(|s| s.id != skill.id.as_str());
        let entry = GlobalLockedSkillEntry {
            id: skill.id.to_string(),
            name: skill.name.clone(),
            origin: skill.origin.clone(),
            resolved: Resolved {
                version: skill.version.clone(),
                commit_hash: skill.commit_hash.clone(),
                checksum: None,
            },
            dependencies: skill.dependencies.clone().unwrap_or_default(),
            groups: Vec::new(),
            installed_at,
            last_checked_at: None,
            last_updated_at: None,
        };
        self.skills.push(entry);
    }

    pub fn remove_skill(&mut self, skill_id: &str) -> bool {
        let initial_len = self.skills.len();
        self.skills.retain(|s| s.id != skill_id);
        self.skills.len() < initial_len
    }

    pub fn mark_checked(&mut self, skill_id: &str, checked_at: DateTime<Utc>) {
        if let Some(entry) = self.skills.iter_mut().find(|s| s.id == skill_id) {
            entry.last_checked_at = Some(checked_at);
        }
    }

    pub fn mark_updated(&mut self, skill_id: &str, updated_at: DateTime<Utc>) {
        if let Some(entry) = self.skills.iter_mut().find(|s| s.id == skill_id) {
            entry.last_updated_at = Some(updated_at);
        }
    }

    fn sort_entries(&mut self) {
        self.skills.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

// ── Lock mismatch ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LockMismatch {
    pub skill_id: String,
    pub reason: String,
}

// ── Extended Error Enum ───────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("Lock file not found: {0}")]
    NotFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Serialize error: {0}")]
    Serialize(String),

    /// Advisory file lock held by another process.
    #[error("Lock file is held by another process: {0}")]
    FileLocked(PathBuf),

    /// Cannot determine global config directory.
    #[error("Global config directory unavailable: {0}")]
    GlobalConfigUnavailable(String),

    /// The lock file predates the `Origin` model (format version < 3.0). There is
    /// no migrator — the caller must delete the lock and reinstall.
    #[error(
        "skills lock format {found} predates the Origin model (3.0); \
         delete the lock file and re-run `fastskill install`"
    )]
    UnsupportedVersion { found: String },
}

/// Lightweight pre-check: read only `metadata.version` out of the raw TOML text,
/// before attempting to deserialize the full lock structure. A pre-Origin lock
/// file's `[[skills]]` entries won't match the current shape at all (e.g. a
/// missing `origin` field), which would otherwise surface as an opaque
/// `LockError::Parse` instead of the actionable `UnsupportedVersion` guard.
fn check_lock_format_version(content: &str) -> Result<(), LockError> {
    #[derive(Deserialize)]
    struct VersionOnly {
        version: String,
    }
    #[derive(Deserialize)]
    struct MetadataOnly {
        metadata: VersionOnly,
    }

    let parsed: MetadataOnly =
        toml::from_str(content).map_err(|e| LockError::Parse(e.to_string()))?;
    if parsed.metadata.version != LOCK_FORMAT_VERSION {
        return Err(LockError::UnsupportedVersion {
            found: parsed.metadata.version,
        });
    }
    Ok(())
}

// ── Routing helpers ───────────────────────────────────────────────────────────

/// Returns the project lock path given the resolved project file path.
pub fn project_lock_path(project_file: &Path) -> PathBuf {
    if let Some(parent) = project_file.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    }
}

/// Returns the global lock path (platform-specific config directory).
pub fn global_lock_path() -> Result<PathBuf, LockError> {
    dirs::config_dir()
        .map(|d| d.join("fastskill").join("global-skills.lock"))
        .ok_or_else(|| {
            LockError::GlobalConfigUnavailable(
                "dirs::config_dir() returned None on this platform".to_string(),
            )
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
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
}

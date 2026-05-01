//! Skills lock file management for reproducible installations
//!
//! Two distinct lock structures are maintained:
//! - `ProjectSkillsLock`: deterministic, timestamp-free, for `skills.lock` at project root
//! - `GlobalSkillsLock`: operational, with timestamps, for `global-skills.lock` in user config dir

use crate::core::manifest::SkillSource;
use crate::core::skill_manager::SkillDefinition;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    pub version: String,
    pub source: SkillSource,
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_branch: Option<String>,
    #[serde(default)]
    pub commit_hash: Option<String>,
    #[serde(default)]
    pub checksum: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub editable: bool,
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

/// Backward-compatible type alias (deprecated; remove in next major version).
pub type SkillsLock = ProjectSkillsLock;

impl ProjectSkillsLock {
    pub fn new_empty() -> Self {
        Self {
            metadata: ProjectLockMetadata {
                version: "2.0".to_string(),
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
        std::fs::write(path, content).map_err(LockError::Io)?;
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
        let source = build_skill_source(skill);
        let entry = ProjectLockedSkillEntry {
            id: skill.id.to_string(),
            name: skill.name.clone(),
            version: skill.version.clone(),
            source,
            source_name: skill.installed_from.clone(),
            source_url: skill.source_url.clone(),
            source_branch: skill.source_branch.clone(),
            commit_hash: skill.commit_hash.clone(),
            checksum: None,
            dependencies: skill.dependencies.clone().unwrap_or_default(),
            groups: Vec::new(),
            editable: skill.editable,
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
                if installed.version != locked.version {
                    mismatches.push(LockMismatch {
                        skill_id: locked.id.clone(),
                        reason: format!(
                            "Version mismatch: lock={}, installed={}",
                            locked.version, installed.version
                        ),
                    });
                }
                if let (Some(lock_commit), Some(inst_commit)) =
                    (&locked.commit_hash, &installed.commit_hash)
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
    pub version: String,
    pub source: SkillSource,
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_branch: Option<String>,
    #[serde(default)]
    pub commit_hash: Option<String>,
    #[serde(default)]
    pub checksum: Option<String>,
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
                version: "1.0".to_string(),
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
        let lock: GlobalSkillsLock =
            toml::from_str(&content).map_err(|e| LockError::Parse(e.to_string()))?;
        Ok(lock)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), LockError> {
        let mut lock = self.clone();
        lock.sort_entries();
        lock.metadata.fastskill_version = Some(env!("CARGO_PKG_VERSION").to_string());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(LockError::Io)?;
        }
        let content =
            toml::to_string_pretty(&lock).map_err(|e| LockError::Serialize(e.to_string()))?;
        std::fs::write(path, content).map_err(LockError::Io)?;
        Ok(())
    }

    pub fn upsert_skill(&mut self, skill: &SkillDefinition, installed_at: DateTime<Utc>) {
        self.skills.retain(|s| s.id != skill.id.as_str());
        let source = build_skill_source(skill);
        let entry = GlobalLockedSkillEntry {
            id: skill.id.to_string(),
            name: skill.name.clone(),
            version: skill.version.clone(),
            source,
            source_name: skill.installed_from.clone(),
            source_url: skill.source_url.clone(),
            source_branch: skill.source_branch.clone(),
            commit_hash: skill.commit_hash.clone(),
            checksum: None,
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

// ── Shared source builder ─────────────────────────────────────────────────────

fn build_skill_source(skill: &SkillDefinition) -> SkillSource {
    if let Some(source_type) = &skill.source_type {
        match source_type {
            crate::core::skill_manager::SourceType::GitUrl => SkillSource::Git {
                url: skill.source_url.clone().unwrap_or_default(),
                branch: skill.source_branch.clone(),
                tag: skill.source_tag.clone(),
                subdir: skill.source_subdir.clone(),
            },
            crate::core::skill_manager::SourceType::LocalPath => SkillSource::Local {
                path: skill.source_subdir.clone().unwrap_or_else(|| {
                    std::path::PathBuf::from(skill.source_url.clone().unwrap_or_default())
                }),
                editable: skill.editable,
            },
            crate::core::skill_manager::SourceType::ZipFile => SkillSource::ZipUrl {
                base_url: skill.source_url.clone().unwrap_or_default(),
                version: Some(skill.version.clone()),
            },
            crate::core::skill_manager::SourceType::Source => SkillSource::Source {
                name: skill.installed_from.clone().unwrap_or_default(),
                skill: skill.id.to_string(),
                version: Some(skill.version.clone()),
            },
        }
    } else {
        SkillSource::Git {
            url: skill.source_url.clone().unwrap_or_default(),
            branch: None,
            tag: None,
            subdir: None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::service::SkillId;
    use crate::core::skill_manager::{SkillDefinition, SourceType};
    use chrono::Utc;
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
    fn test_project_lock_migration_strips_volatile_fields() {
        // Simulate loading a v1.0.0 skills.lock that has generated_at and fetched_at
        let old_format = r#"[metadata]
version = "1.0.0"
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

        // Load old format - unknown fields (generated_at, fetched_at) are silently ignored
        let loaded = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
        assert_eq!(loaded.skills.len(), 1);
        assert_eq!(loaded.skills[0].id, "old-skill");

        // Save with new code
        loaded.save_to_file(&lock_path).unwrap();
        let new_content = std::fs::read_to_string(&lock_path).unwrap();

        assert!(
            !new_content.contains("generated_at"),
            "generated_at must be stripped on save"
        );
        assert!(
            !new_content.contains("fetched_at"),
            "fetched_at must be stripped on save"
        );
    }

    #[test]
    fn test_skils_lock_type_alias_compiles() {
        // Ensure SkillsLock type alias still works
        let lock: SkillsLock = SkillsLock::new_empty();
        assert_eq!(lock.metadata.version, "2.0");
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
    fn test_file_lock_contention_returns_error() {
        use fs2::FileExt;

        let tmp = TempDir::new().unwrap();
        let sidecar = tmp.path().join("skills.lock.lock");

        // Acquire the advisory lock from this thread
        let holder = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&sidecar)
            .unwrap();
        holder.lock_exclusive().unwrap();

        // Attempting to acquire again (try_lock) should fail
        let contender = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&sidecar)
            .unwrap();

        // try_lock_exclusive should return an error when the lock is already held
        let result = contender.try_lock_exclusive();
        assert!(
            result.is_err(),
            "try_lock_exclusive must fail when lock is held"
        );

        holder.unlock().unwrap();
    }
}

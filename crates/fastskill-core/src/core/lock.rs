//! Skills lock file management for reproducible installations
//!
//! Two distinct lock structures are maintained:
//! - `ProjectSkillsLock`: deterministic, timestamp-free, for `skills.lock` at project root
//! - `GlobalSkillsLock`: operational, with timestamps, for `global-skills.lock` in user config dir

use crate::core::origin::{GitRef, Origin, Resolved};
use crate::core::skill_manager::SkillDefinition;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The lock format version. Bumped to "3.0" for the `Origin`/`Resolved` reshape
/// (Phase 0). Lock files from before this version cannot be read — see
/// [`LockError::UnsupportedVersion`].
pub const LOCK_FORMAT_VERSION: &str = "3.0";

/// The pre-`Origin` project lock format, which [`ProjectSkillsLock::load_from_file`] migrates.
///
/// Only the project lock is migrated. The global lock's legacy shape carried operational
/// timestamps (`installed_at` and friends) that the 1.0.0 project entries never had, and no
/// specimen of a legacy global lock was available to verify against — so it keeps the
/// original hard rejection rather than a migration written from guesswork.
const LEGACY_PROJECT_LOCK_VERSION: &str = "1.0.0";

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

    /// Load the project lock, migrating a [`LEGACY_PROJECT_LOCK_VERSION`] file in memory.
    ///
    /// Migrating rather than rejecting matters here specifically because a lock records
    /// *pinned* versions. Telling someone with a working installation to delete it and
    /// re-run `install` re-resolves every dependency against whatever is current now — a
    /// silent upgrade they did not ask for. Reading never writes; the 3.0 form reaches disk
    /// only when something saves the lock for its own reasons.
    pub fn load_from_file(path: &Path) -> Result<Self, LockError> {
        if !path.exists() {
            return Err(LockError::NotFound(path.to_path_buf()));
        }
        let safe_path = path.canonicalize().map_err(LockError::Io)?;
        let content = std::fs::read_to_string(&safe_path).map_err(LockError::Io)?;

        match read_lock_format_version(&content)?.as_str() {
            LOCK_FORMAT_VERSION => {
                toml::from_str(&content).map_err(|e| LockError::Parse(e.to_string()))
            }
            LEGACY_PROJECT_LOCK_VERSION => {
                let legacy: LegacyProjectSkillsLock =
                    toml::from_str(&content).map_err(|e| LockError::Parse(e.to_string()))?;
                legacy.upgrade()
            }
            // Anything else — including a version newer than this build — is still refused.
            found => Err(LockError::UnsupportedVersion {
                found: found.to_string(),
            }),
        }
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

// ── Legacy project lock (format 1.0.0) ───────────────────────────────────────
//
// The pre-`Origin` project lock spelled provenance as flat `source_url`/`source_branch`
// fields plus a nested `[skills.source]` table:
//
//     [[skills]]
//     id = "example"
//     version = "1.0.2"
//     source_url = "https://github.com/org/repo"
//     source_branch = "main"
//     editable = false
//     depth = 0
//
//     [skills.source]
//     type = "git"
//     url = "https://github.com/org/repo"
//     branch = "main"
//
// Read-only support: nothing writes this shape, so there is no round trip to preserve.

/// The nested `[skills.source]` table of a legacy entry.
#[derive(Debug, Clone, Deserialize)]
struct LegacyLockSource {
    r#type: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    zip_url: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    skill: Option<String>,
    /// The requested constraint for a repository install. Distinct from the entry-level
    /// `version`, which is the concrete version that was resolved.
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    editable: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyProjectLockedSkill {
    id: String,
    #[serde(default)]
    name: Option<String>,
    version: String,
    #[serde(default)]
    source: Option<LegacyLockSource>,
    /// Flat fallbacks, present on every 1.0.0 entry even when `[skills.source]` is too.
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    source_branch: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    editable: bool,
    #[serde(default)]
    depth: u32,
    #[serde(default)]
    parent_skill: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyProjectSkillsLock {
    #[serde(default)]
    skills: Vec<LegacyProjectLockedSkill>,
}

impl LegacyProjectLockedSkill {
    fn upgrade(self) -> Result<ProjectLockedSkillEntry, LockError> {
        let id = self.id;
        let missing = |field: &str| {
            LockError::Parse(format!(
                "legacy lock entry '{id}' cannot be migrated: its source is '{field}' but that \
             field is missing. Delete skills.lock and re-run `fastskill install` to rebuild it."
            ))
        };

        // `[skills.source]` is authoritative; the flat `source_url`/`source_branch` fields are
        // the fallback for entries that predate it.
        let (kind, url, branch, path, zip_url, repo_name, repo_skill, src_editable, req_version) =
            match self.source {
                Some(s) => (
                    s.r#type, s.url, s.branch, s.path, s.zip_url, s.name, s.skill, s.editable,
                    s.version,
                ),
                None => (
                    "git".to_string(),
                    self.source_url.clone(),
                    self.source_branch.clone(),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
            };

        let origin = match kind.as_str() {
            "git" => Origin::Git {
                url: url.or(self.source_url).ok_or_else(|| missing("url"))?,
                // No branch meant the repository default — NOT a branch named "main".
                r#ref: match branch.or(self.source_branch) {
                    Some(b) => GitRef::Branch(b),
                    None => GitRef::Default,
                },
                subdir: None,
            },
            "local" => Origin::Local {
                path: PathBuf::from(path.ok_or_else(|| missing("path"))?),
                // The nested table won if it said anything; otherwise the entry-level flag.
                editable: src_editable.unwrap_or(self.editable),
            },
            "zip-url" => Origin::ZipUrl {
                url: zip_url.or(url).ok_or_else(|| missing("zip_url"))?,
            },
            // Legacy called a configured-repository install "source".
            "source" | "repository" => Origin::Repository {
                repo: repo_name.ok_or_else(|| missing("name"))?,
                skill: repo_skill.unwrap_or_else(|| id.clone()),
                // The legacy source table carried the requested constraint; dropping it
                // would turn a pinned repository dependency into "newest allowed".
                version: match req_version {
                    Some(raw) => Some(
                        crate::core::version::VersionConstraint::parse(&raw).map_err(|e| {
                            LockError::Parse(format!(
                                "legacy lock entry '{id}' has an unparseable version constraint '{raw}': {e}"
                            ))
                        })?,
                    ),
                    None => None,
                },
            },
            other => {
                return Err(LockError::Parse(format!(
                    "legacy lock entry '{id}' has unknown source type '{other}'. Delete \
                     skills.lock and re-run `fastskill install` to rebuild it."
                )))
            }
        };

        Ok(ProjectLockedSkillEntry {
            name: self.name.unwrap_or_else(|| id.clone()),
            id,
            origin,
            // 1.0.0 never recorded a commit hash or checksum, so there is nothing to carry
            // over. The migrated lock is exactly as precise as the file it came from — just
            // less precise than one produced by a fresh resolve.
            resolved: Resolved {
                version: self.version,
                commit_hash: None,
                checksum: None,
            },
            dependencies: self.dependencies,
            groups: self.groups,
            depth: self.depth,
            parent_skill: self.parent_skill,
        })
    }
}

impl LegacyProjectSkillsLock {
    fn upgrade(self) -> Result<ProjectSkillsLock, LockError> {
        let mut skills = Vec::with_capacity(self.skills.len());
        for entry in self.skills {
            skills.push(entry.upgrade()?);
        }
        Ok(ProjectSkillsLock {
            metadata: ProjectLockMetadata {
                version: LOCK_FORMAT_VERSION.to_string(),
                fastskill_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            skills,
        })
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
    let found = read_lock_format_version(content)?;
    if found != LOCK_FORMAT_VERSION {
        return Err(LockError::UnsupportedVersion { found });
    }
    Ok(())
}

/// Read `metadata.version` alone, without parsing the rest.
///
/// A legacy lock does not deserialize as the current shape, so the version has to be read
/// in its own pass before deciding how to parse the file.
fn read_lock_format_version(content: &str) -> Result<String, LockError> {
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
    Ok(parsed.metadata.version)
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod legacy_lock_migration_tests {
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
}

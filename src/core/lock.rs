//! Skills lock file management for reproducible installations

use crate::core::manifest::SkillSource;
use crate::core::skill_manager::SkillDefinition;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Lock file structure (exact installed state)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsLock {
    pub metadata: LockMetadata,
    #[serde(default)]
    pub skills: Vec<LockedSkillEntry>,
}

/// Lock file metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockMetadata {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    #[serde(default)]
    pub fastskill_version: Option<String>,
}

/// Locked skill entry (exact installed state)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedSkillEntry {
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
    pub fetched_at: DateTime<Utc>,
    #[serde(default)]
    pub checksum: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub editable: bool,
}

/// Lock file errors
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
}

impl SkillsLock {
    /// Load lock file from TOML
    pub fn load_from_file(path: &Path) -> Result<Self, LockError> {
        if !path.exists() {
            return Err(LockError::NotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path).map_err(LockError::Io)?;

        let lock: SkillsLock =
            toml::from_str(&content).map_err(|e| LockError::Parse(e.to_string()))?;

        Ok(lock)
    }

    /// Save lock file to TOML
    pub fn save_to_file(&self, path: &Path) -> Result<(), LockError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| LockError::Serialize(e.to_string()))?;

        std::fs::write(path, content).map_err(LockError::Io)?;

        Ok(())
    }

    /// Generate lock file from installed skills in registry
    pub fn from_installed_skills(skills: &[SkillDefinition]) -> Self {
        let mut locked_skills = Vec::new();

        for skill in skills {
            // Convert SkillDefinition to LockedSkillEntry
            let source = if let Some(source_type) = &skill.source_type {
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
                // Fallback for skills without source type
                SkillSource::Git {
                    url: skill.source_url.clone().unwrap_or_default(),
                    branch: None,
                    tag: None,
                    subdir: None,
                }
            };

            let locked_entry = LockedSkillEntry {
                id: skill.id.to_string(),
                name: skill.name.clone(),
                version: skill.version.clone(),
                source,
                source_name: skill.installed_from.clone(),
                source_url: skill.source_url.clone(),
                source_branch: skill.source_branch.clone(),
                commit_hash: skill.commit_hash.clone(),
                fetched_at: skill.fetched_at.unwrap_or(skill.created_at),
                checksum: None, // TODO: Calculate checksum
                dependencies: skill.dependencies.clone().unwrap_or_default(),
                groups: Vec::new(), // TODO: Extract groups from manifest when available
                editable: skill.editable,
            };

            locked_skills.push(locked_entry);
        }

        SkillsLock {
            metadata: LockMetadata {
                version: "1.0.0".to_string(),
                generated_at: Utc::now(),
                fastskill_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            },
            skills: locked_skills,
        }
    }

    /// Update lock file with a new skill
    pub fn update_skill(&mut self, skill: &SkillDefinition) {
        // Remove existing entry if present
        self.skills.retain(|s| s.id != skill.id.as_str());

        // Convert to locked entry
        let source = if let Some(source_type) = &skill.source_type {
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
        };

        let locked_entry = LockedSkillEntry {
            id: skill.id.to_string(),
            name: skill.name.clone(),
            version: skill.version.clone(),
            source,
            source_name: skill.installed_from.clone(),
            source_url: skill.source_url.clone(),
            source_branch: skill.source_branch.clone(),
            commit_hash: skill.commit_hash.clone(),
            fetched_at: skill.fetched_at.unwrap_or(skill.created_at),
            checksum: None, // TODO: Calculate checksum
            dependencies: skill.dependencies.clone().unwrap_or_default(),
            groups: Vec::new(),
            editable: skill.editable,
        };

        self.skills.push(locked_entry);
        self.metadata.generated_at = Utc::now();
    }

    /// Remove skill from lock file
    pub fn remove_skill(&mut self, skill_id: &str) -> bool {
        let initial_len = self.skills.len();
        self.skills.retain(|s| s.id != skill_id);
        self.metadata.generated_at = Utc::now();
        self.skills.len() < initial_len
    }

    /// Verify lock matches installed skills
    pub fn verify_matches_installed(
        &self,
        installed_skills: &[SkillDefinition],
    ) -> Vec<LockMismatch> {
        let mut mismatches = Vec::new();

        // Check all locked skills are installed
        for locked in &self.skills {
            if let Some(installed) = installed_skills.iter().find(|s| s.id.as_str() == locked.id) {
                // Check version mismatch
                if installed.version != locked.version {
                    mismatches.push(LockMismatch {
                        skill_id: locked.id.clone(),
                        reason: format!(
                            "Version mismatch: lock={}, installed={}",
                            locked.version, installed.version
                        ),
                    });
                }

                // Check commit hash mismatch (for git sources)
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

        // Check for installed skills not in lock
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
}

/// Lock mismatch information
#[derive(Debug, Clone)]
pub struct LockMismatch {
    pub skill_id: String,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::service::SkillId;
    use crate::core::skill_manager::{SkillDefinition, SourceType};
    use chrono::Utc;

    #[test]
    fn test_lock_from_skills() {
        let skill = SkillDefinition {
            id: SkillId::new("test-skill".to_string()).unwrap(),
            name: "Test Skill".to_string(),
            description: "A test skill".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            tags: Vec::new(),
            capabilities: Vec::new(),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            skill_file: std::path::PathBuf::from("test.md"),
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
        };

        let lock = SkillsLock::from_installed_skills(&[skill]);
        assert_eq!(lock.skills.len(), 1);
        assert_eq!(lock.skills[0].id, "test-skill");
    }
}

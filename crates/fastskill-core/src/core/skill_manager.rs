//! Skill management service implementation

use crate::core::origin::Origin;
use crate::core::service::{ServiceError, SkillId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// Note: SkillInfoMetadata removed - use skill-project.toml via manifest system instead

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // File references
    pub skill_file: std::path::PathBuf,
    pub reference_files: Option<Vec<std::path::PathBuf>>,
    pub script_files: Option<Vec<std::path::PathBuf>>,
    pub asset_files: Option<Vec<std::path::PathBuf>>,

    // Execution configuration
    pub execution_environment: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub timeout: Option<u64>,

    // Provenance (install intent)
    pub origin: Origin,
    // Resolved facts a fetch produced (read by the lock)
    pub commit_hash: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
}

impl SkillDefinition {
    pub fn new(
        id: SkillId,
        name: String,
        description: String,
        version: String,
        origin: Origin,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: id.clone(),
            name,
            description,
            version,
            author: None,
            created_at: now,
            updated_at: now,
            skill_file: std::path::PathBuf::from(format!("./skills/{}/SKILL.md", id)),
            reference_files: None,
            script_files: None,
            asset_files: None,
            execution_environment: None,
            dependencies: None,
            timeout: None,
            origin,
            commit_hash: None,
            fetched_at: None,
        }
    }

    // Note: load_skill_info removed - use skill-project.toml via manifest system instead
}

#[async_trait]
pub trait SkillManagementService: Send + Sync {
    async fn register_skill(&self, skill: SkillDefinition) -> Result<SkillId, ServiceError>;
    async fn force_register_skill(&self, skill: SkillDefinition) -> Result<SkillId, ServiceError>;
    async fn get_skill(&self, skill_id: &SkillId) -> Result<Option<SkillDefinition>, ServiceError>;
    async fn update_skill(
        &self,
        skill_id: &SkillId,
        updates: SkillUpdate,
    ) -> Result<(), ServiceError>;
    async fn unregister_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError>;
    async fn list_skills(&self) -> Result<Vec<SkillDefinition>, ServiceError>;
}

#[derive(Debug)]
pub struct SkillManager {
    skills: Arc<RwLock<HashMap<SkillId, SkillDefinition>>>,
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillManager {
    pub fn new() -> Self {
        Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub origin: Option<Origin>,
    pub commit_hash: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl SkillManagementService for SkillManager {
    async fn register_skill(&self, skill: SkillDefinition) -> Result<SkillId, ServiceError> {
        let mut skills = self.skills.write().await;

        // Check if skill already exists
        if skills.contains_key(&skill.id) {
            return Err(ServiceError::AlreadyIndexed(skill.id.to_string()));
        }

        let skill_id = skill.id.clone();
        skills.insert(skill_id.clone(), skill);
        Ok(skill_id)
    }

    async fn force_register_skill(&self, skill: SkillDefinition) -> Result<SkillId, ServiceError> {
        let mut skills = self.skills.write().await;

        let skill_id = skill.id.clone();
        skills.insert(skill_id.clone(), skill);
        Ok(skill_id)
    }

    async fn get_skill(&self, skill_id: &SkillId) -> Result<Option<SkillDefinition>, ServiceError> {
        let skills = self.skills.read().await;
        Ok(skills.get(skill_id).cloned())
    }

    async fn update_skill(
        &self,
        skill_id: &SkillId,
        updates: SkillUpdate,
    ) -> Result<(), ServiceError> {
        let mut skills = self.skills.write().await;

        if let Some(skill) = skills.get_mut(skill_id) {
            if let Some(name) = updates.name {
                skill.name = name;
            }
            if let Some(description) = updates.description {
                skill.description = description;
            }
            if let Some(version) = updates.version {
                skill.version = version;
            }
            if let Some(author) = updates.author {
                skill.author = Some(author);
            }
            if let Some(origin) = updates.origin {
                skill.origin = origin;
            }
            if let Some(commit_hash) = updates.commit_hash {
                skill.commit_hash = Some(commit_hash);
            }
            if let Some(fetched_at) = updates.fetched_at {
                skill.fetched_at = Some(fetched_at);
            }
            skill.updated_at = Utc::now();
            Ok(())
        } else {
            Err(ServiceError::SkillNotFound(skill_id.to_string()))
        }
    }

    async fn unregister_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError> {
        let mut skills = self.skills.write().await;

        if skills.remove(skill_id).is_some() {
            Ok(())
        } else {
            Err(ServiceError::SkillNotFound(skill_id.to_string()))
        }
    }

    async fn list_skills(&self) -> Result<Vec<SkillDefinition>, ServiceError> {
        let skills = self.skills.read().await;
        Ok(skills.values().cloned().collect())
    }
}

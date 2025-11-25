//! Skill management service implementation

use crate::core::service::{ServiceError, SkillId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

// Note: SkillInfoMetadata removed - use skill-project.toml via manifest system instead

/// Source type for skill installation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceType {
    GitUrl,
    LocalPath,
    ZipFile,
    Source,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub capabilities: Vec<String>,
    pub enabled: bool,
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

    // Source tracking
    pub source_url: Option<String>,
    pub source_type: Option<SourceType>,
    pub source_branch: Option<String>,
    pub source_tag: Option<String>,
    pub source_subdir: Option<PathBuf>,
    pub installed_from: Option<String>,
    pub commit_hash: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub editable: bool,
}

impl SkillDefinition {
    pub fn new(id: SkillId, name: String, description: String, version: String) -> Self {
        let now = Utc::now();
        Self {
            id: id.clone(),
            name,
            description,
            version,
            author: None,
            tags: Vec::new(),
            capabilities: Vec::new(),
            enabled: true,
            created_at: now,
            updated_at: now,
            skill_file: std::path::PathBuf::from(format!("./skills/{}/SKILL.md", id)),
            reference_files: None,
            script_files: None,
            asset_files: None,
            execution_environment: None,
            dependencies: None,
            timeout: None,
            source_url: None,
            source_type: None,
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: None,
            commit_hash: None,
            fetched_at: None,
            editable: false,
        }
    }

    /// Create a skill definition from a dictionary (for Python bindings)
    pub fn from_dict(data: &std::collections::HashMap<String, String>) -> Result<Self, String> {
        let id_str = data.get("id").ok_or("Missing id")?.clone();
        let id = crate::core::service::SkillId::new(id_str).map_err(|e| e.to_string())?;
        let name = data.get("name").ok_or("Missing name")?.clone();
        let description = data
            .get("description")
            .ok_or("Missing description")?
            .clone();
        let version = data.get("version").unwrap_or(&"1.0.0".to_string()).clone();

        let mut skill = Self::new(id, name, description, version);

        // Add optional fields if present
        if let Some(tags_str) = data.get("tags") {
            if !tags_str.is_empty() {
                // Simple comma-separated tag parsing for now
                skill.tags = tags_str.split(',').map(|s| s.trim().to_string()).collect();
            }
        }

        if let Some(capabilities_str) = data.get("capabilities") {
            if !capabilities_str.is_empty() {
                // Simple comma-separated capability parsing for now
                skill.capabilities = capabilities_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
            }
        }

        // Handle file paths
        if let Some(skill_file) = data.get("skill_file") {
            skill.skill_file = std::path::PathBuf::from(skill_file);
        }

        if let Some(reference_files) = data.get("reference_files") {
            if !reference_files.is_empty() {
                skill.reference_files = Some(
                    reference_files
                        .split(',')
                        .map(|s| std::path::PathBuf::from(s.trim()))
                        .collect(),
                );
            }
        }

        if let Some(script_files) = data.get("script_files") {
            if !script_files.is_empty() {
                skill.script_files = Some(
                    script_files
                        .split(',')
                        .map(|s| std::path::PathBuf::from(s.trim()))
                        .collect(),
                );
            }
        }

        if let Some(asset_files) = data.get("asset_files") {
            if !asset_files.is_empty() {
                skill.asset_files = Some(
                    asset_files
                        .split(',')
                        .map(|s| std::path::PathBuf::from(s.trim()))
                        .collect(),
                );
            }
        }

        Ok(skill)
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
    async fn list_skills(
        &self,
        filters: Option<SkillFilters>,
    ) -> Result<Vec<SkillDefinition>, ServiceError>;
    async fn enable_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError>;
    async fn disable_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError>;
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

#[derive(Debug, Clone)]
pub struct SkillFilters {
    pub enabled: Option<bool>,
    pub tags: Option<Vec<String>>,
    pub capabilities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tags: Option<Vec<String>>,
    pub capabilities: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub source_url: Option<String>,
    pub source_type: Option<SourceType>,
    pub source_branch: Option<String>,
    pub source_tag: Option<String>,
    pub source_subdir: Option<PathBuf>,
    pub installed_from: Option<String>,
    pub commit_hash: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub editable: Option<bool>,
}

#[async_trait]
impl SkillManagementService for SkillManager {
    async fn register_skill(&self, skill: SkillDefinition) -> Result<SkillId, ServiceError> {
        let mut skills = self.skills.write().await;

        // Check if skill already exists
        if skills.contains_key(&skill.id) {
            return Err(ServiceError::Custom(format!(
                "Skill {} already exists",
                skill.id
            )));
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
            if let Some(tags) = updates.tags {
                skill.tags = tags;
            }
            if let Some(capabilities) = updates.capabilities {
                skill.capabilities = capabilities;
            }
            if let Some(enabled) = updates.enabled {
                skill.enabled = enabled;
            }
            if let Some(source_url) = updates.source_url {
                skill.source_url = Some(source_url);
            }
            if let Some(source_type) = updates.source_type {
                skill.source_type = Some(source_type);
            }
            if let Some(source_branch) = updates.source_branch {
                skill.source_branch = Some(source_branch);
            }
            if let Some(source_tag) = updates.source_tag {
                skill.source_tag = Some(source_tag);
            }
            if let Some(source_subdir) = updates.source_subdir {
                skill.source_subdir = Some(source_subdir);
            }
            if let Some(installed_from) = updates.installed_from {
                skill.installed_from = Some(installed_from);
            }
            if let Some(commit_hash) = updates.commit_hash {
                skill.commit_hash = Some(commit_hash);
            }
            if let Some(fetched_at) = updates.fetched_at {
                skill.fetched_at = Some(fetched_at);
            }
            if let Some(editable) = updates.editable {
                skill.editable = editable;
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

    async fn list_skills(
        &self,
        filters: Option<SkillFilters>,
    ) -> Result<Vec<SkillDefinition>, ServiceError> {
        let skills = self.skills.read().await;
        let mut filtered_skills: Vec<SkillDefinition> = skills.values().cloned().collect();

        if let Some(filters) = filters {
            // Filter by enabled status
            if let Some(enabled) = filters.enabled {
                filtered_skills.retain(|skill| skill.enabled == enabled);
            }

            // Filter by tags
            if let Some(tags) = filters.tags {
                filtered_skills.retain(|skill| skill.tags.iter().any(|tag| tags.contains(tag)));
            }

            // Filter by capabilities
            if let Some(capabilities) = filters.capabilities {
                filtered_skills.retain(|skill| {
                    skill
                        .capabilities
                        .iter()
                        .any(|cap| capabilities.contains(cap))
                });
            }
        }

        Ok(filtered_skills)
    }

    async fn enable_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError> {
        self.update_skill(
            skill_id,
            SkillUpdate {
                enabled: Some(true),
                ..Default::default()
            },
        )
        .await
    }

    async fn disable_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError> {
        self.update_skill(
            skill_id,
            SkillUpdate {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
    }
}

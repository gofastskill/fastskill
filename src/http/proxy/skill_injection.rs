//! Skill injection middleware for intercepting and modifying requests

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;

/// Skill reference in container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSkill {
    pub r#type: String, // "anthropic" or "custom"
    pub skill_id: String,
    pub version: Option<String>,
}

/// Container specification with skills
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSpec {
    pub skills: Option<Vec<ContainerSkill>>,
}

/// Skill injection service
#[derive(Clone)]
pub struct SkillInjector {
    service: Arc<dyn super::ProxyService>,
    skills_dir: PathBuf,
    hardcoded_skills: Vec<String>,
    anthropic_skill_mappings: HashMap<String, String>,
}

impl SkillInjector {
    /// Create a new skill injector
    pub fn new(
        service: Arc<dyn super::ProxyService>,
        skills_dir: PathBuf,
        hardcoded_skills: Vec<String>,
    ) -> Self {
        let mut anthropic_skill_mappings = HashMap::new();
        // Map Anthropic skill IDs to local directories
        anthropic_skill_mappings.insert("pptx".to_string(), "presentation/pptx".to_string());
        anthropic_skill_mappings.insert("xlsx".to_string(), "spreadsheet/xlsx".to_string());
        anthropic_skill_mappings.insert("docx".to_string(), "document/docx".to_string());
        anthropic_skill_mappings.insert("pdf".to_string(), "document/pdf".to_string());

        Self {
            service,
            skills_dir,
            hardcoded_skills,
            anthropic_skill_mappings,
        }
    }

    /// Inject hardcoded skills from manifest
    pub async fn inject_hardcoded_skills(
        &self,
        body: &mut Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.hardcoded_skills.is_empty() {
            return Ok(());
        }

        // Convert hardcoded skill IDs to ContainerSkill format
        let skills: Vec<ContainerSkill> = self
            .hardcoded_skills
            .iter()
            .map(|skill_id| ContainerSkill {
                r#type: "custom".to_string(),
                skill_id: skill_id.clone(),
                version: Some("latest".to_string()),
            })
            .collect();

        // Inject skills into container (merge with existing skills)
        self.inject_skills_to_container(body, skills).await
    }

    /// Inject skills into container, merging with existing skills
    async fn inject_skills_to_container(
        &self,
        body: &mut Value,
        new_skills: Vec<ContainerSkill>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if new_skills.is_empty() {
            return Ok(());
        }

        // Ensure container exists
        if body.get("container").is_none() {
            if let Some(body_obj) = body.as_object_mut() {
                body_obj.insert("container".to_string(), json!({}));
            } else {
                return Err("Request body is not a JSON object".into());
            }
        }

        if let Some(container) = body.get_mut("container") {
            if let Some(container_obj) = container.as_object_mut() {
                // Get existing skills if any
                let mut existing_skills: Vec<ContainerSkill> =
                    if let Some(skills_value) = container_obj.get("skills") {
                        serde_json::from_value(skills_value.clone()).unwrap_or_default()
                    } else {
                        Vec::new()
                    };

                // Merge new skills with existing, avoiding duplicates
                let existing_ids: std::collections::HashSet<String> =
                    existing_skills.iter().map(|s| s.skill_id.clone()).collect();

                for new_skill in new_skills {
                    if !existing_ids.contains(&new_skill.skill_id) {
                        existing_skills.push(new_skill);
                    }
                }

                // Update container with merged skills
                container_obj.insert("skills".to_string(), json!(existing_skills));
            }
        }

        Ok(())
    }

    /// Process a messages.create request and inject skills
    pub async fn process_request(
        &self,
        body: &mut Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Extract container from request body
        if let Some(container) = body.get_mut("container") {
            if let Some(skills) = container.get_mut("skills") {
                // Parse skills array, filtering out invalid entries
                let skills_array: Vec<ContainerSkill> = if let Some(skills_array) =
                    skills.as_array()
                {
                    skills_array
                        .iter()
                        .filter_map(|skill_value| {
                            // Try to parse as ContainerSkill
                            if let Ok(skill) =
                                serde_json::from_value::<ContainerSkill>(skill_value.clone())
                            {
                                // Validate required fields and type
                                if skill.skill_id.is_empty() {
                                    warn!("Skipping skill with missing or empty skill_id");
                                    None
                                } else if skill.r#type != "anthropic" && skill.r#type != "custom" {
                                    warn!("Skipping skill with invalid type: {}", skill.r#type);
                                    None
                                } else {
                                    Some(skill)
                                }
                            } else {
                                warn!("Skipping invalid skill entry: {:?}", skill_value);
                                None
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                };

                // Update container to only include valid skills
                if let Some(container_obj) = container.as_object_mut() {
                    let valid_skills_json: Vec<Value> = skills_array
                        .iter()
                        .map(|skill| {
                            json!({
                                "type": skill.r#type,
                                "skill_id": skill.skill_id,
                                "version": skill.version
                            })
                        })
                        .collect();
                    container_obj.insert("skills".to_string(), json!(valid_skills_json));
                }

                // Load and inject skills (only if we have valid skills)
                if !skills_array.is_empty() {
                    let skill_contents = self.load_skills(&skills_array).await?;
                    self.inject_skill_content(container, skill_contents)?;
                }
            }
        }

        Ok(())
    }

    /// Enable dynamic skill injection for a request
    pub async fn enable_dynamic_injection(
        &self,
        body: &mut Value,
        max_skills: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Extract user message content for semantic search
        let query = self.extract_user_message(body)?;

        // Search for relevant skills
        let relevant_skills = self.search_relevant_skills(&query, max_skills).await?;

        // Inject skills into container
        self.inject_dynamic_skills(body, relevant_skills).await?;

        Ok(())
    }

    /// Load skill content for the specified skills
    async fn load_skills(
        &self,
        skills: &[ContainerSkill],
    ) -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
        let mut skill_contents = HashMap::new();

        for skill in skills {
            let content = match skill.r#type.as_str() {
                "anthropic" => self.load_anthropic_skill(skill).await?,
                "custom" => self.load_custom_skill(skill).await?,
                _ => continue, // Skip unknown types
            };

            skill_contents.insert(skill.skill_id.clone(), content);
        }

        Ok(skill_contents)
    }

    /// Load an Anthropic-managed skill
    async fn load_anthropic_skill(
        &self,
        skill: &ContainerSkill,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let directory = self.anthropic_skill_mappings
            .get(&skill.skill_id)
            .ok_or_else(|| {
                let available_skills: Vec<&str> = self.anthropic_skill_mappings.keys()
                    .map(|s| s.as_str())
                    .collect();
                format!(
                    "Unknown Anthropic skill '{}'. Available Anthropic skills: {}. For custom skills, use type 'custom' instead of 'anthropic'.",
                    skill.skill_id,
                    available_skills.join(", ")
                )
            })?;

        self.load_skill_from_directory(directory).await
    }

    /// Load a custom skill from the registry
    async fn load_custom_skill(
        &self,
        skill: &ContainerSkill,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        // Use the fastskill service to find the skill
        let skills = self.service.skill_manager().list_skills(None).await?;

        let skill_id_parsed = crate::core::service::SkillId::new(skill.skill_id.clone())
            .map_err(|_| format!("Invalid skill ID format: {}", skill.skill_id))?;
        let skill_def = skills
            .into_iter()
            .find(|s| s.id == skill_id_parsed)
            .ok_or_else(|| format!("Custom skill not found: {}", skill.skill_id))?;

        // Load skill content from the skill directory
        let skill_path = self
            .skills_dir
            .join(&skill_def.skill_file)
            .parent()
            .unwrap_or(&self.skills_dir)
            .to_path_buf();

        self.load_skill_from_directory(skill_path.to_str().unwrap_or("."))
            .await
    }

    /// Load skill content from a directory
    async fn load_skill_from_directory(
        &self,
        directory: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let skill_path = self.skills_dir.join(directory);
        let skill_md_path = skill_path.join("SKILL.md");

        if !skill_md_path.exists() {
            return Err(format!("SKILL.md not found in: {}", skill_path.display()).into());
        }

        // Read SKILL.md content
        let content = tokio::fs::read_to_string(skill_md_path).await?;

        // For now, return the raw content
        // In a full implementation, this would parse the skill and prepare it for injection
        Ok(json!({
            "content": content,
            "directory": directory
        }))
    }

    /// Inject skill content into the container
    fn inject_skill_content(
        &self,
        container: &mut Value,
        skill_contents: HashMap<String, Value>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Add skill content to container
        // This is a simplified implementation - in practice, you'd modify
        // the request to include skill content in a way that Claude can use
        if let Some(obj) = container.as_object_mut() {
            obj.insert("injected_skills".to_string(), json!(skill_contents));
        }

        Ok(())
    }

    /// Extract user message content for semantic search
    fn extract_user_message(&self, body: &Value) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(messages) = body.get("messages") {
            if let Some(messages_array) = messages.as_array() {
                // Find the last user message
                for message in messages_array.iter().rev() {
                    if message.get("role") == Some(&json!("user")) {
                        if let Some(content) = message.get("content") {
                            if let Some(content_str) = content.as_str() {
                                return Ok(content_str.to_string());
                            }
                        }
                    }
                }
            }
        }

        Err("No user message found in request".into())
    }

    /// Search for relevant skills using semantic search
    async fn search_relevant_skills(
        &self,
        query: &str,
        max_skills: usize,
    ) -> Result<Vec<ContainerSkill>, Box<dyn std::error::Error>> {
        // Use the fastskill service to search for skills
        // For now, we'll use a simple implementation that searches through available skills
        let search_results = self.service.skill_manager().list_skills(None).await?;

        // Filter skills based on query relevance (simple text matching)
        let relevant_skills: Vec<_> = search_results
            .into_iter()
            .filter(|skill| {
                let query_lower = query.to_lowercase();
                skill.name.to_lowercase().contains(&query_lower)
                    || skill.description.to_lowercase().contains(&query_lower)
                    || skill
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query_lower))
                    || skill
                        .capabilities
                        .iter()
                        .any(|cap| cap.to_lowercase().contains(&query_lower))
            })
            .take(max_skills)
            .collect();

        // Convert search results to ContainerSkill format
        let mut skills = Vec::new();
        for skill in relevant_skills {
            skills.push(ContainerSkill {
                r#type: "custom".to_string(),
                skill_id: skill.id.to_string(),
                version: Some(skill.version),
            });
        }

        Ok(skills)
    }

    /// Inject dynamically discovered skills into the request
    async fn inject_dynamic_skills(
        &self,
        body: &mut Value,
        skills: Vec<ContainerSkill>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Use the shared method to inject skills
        self.inject_skills_to_container(body, skills).await?;

        // Mark as dynamically injected
        if let Some(container) = body.get_mut("container") {
            if let Some(container_obj) = container.as_object_mut() {
                container_obj.insert("dynamic_injection".to_string(), json!(true));
            }
        }

        Ok(())
    }
}

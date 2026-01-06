//! Metadata and discovery service implementation

use crate::core::service::{ServiceError, SkillId};
use crate::core::skill_manager::{SkillDefinition, SkillManagementService};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub capabilities: Vec<String>,
    pub enabled: bool,
    pub token_estimate: usize,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl From<&SkillDefinition> for SkillMetadata {
    fn from(skill: &SkillDefinition) -> Self {
        Self {
            id: skill.id.clone(),
            name: skill.name.clone(),
            description: skill.description.clone(),
            version: skill.version.clone(),
            author: skill.author.clone(),
            tags: skill.tags.clone(),
            capabilities: skill.capabilities.clone(),
            enabled: skill.enabled,
            token_estimate: skill.description.len() / 4, // Rough estimate
            last_updated: skill.updated_at,
        }
    }
}

/// Structured YAML frontmatter extracted from SKILL.md files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub capabilities: Vec<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
    pub allowed_tools: Option<String>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_yaml::Value>,
}

#[async_trait]
pub trait MetadataService: Send + Sync {
    async fn discover_skills(&self, query: &str) -> Result<Vec<SkillMetadata>, ServiceError>;
    async fn find_skills_by_capability(
        &self,
        capability: &str,
    ) -> Result<Vec<SkillMetadata>, ServiceError>;
    async fn find_skills_by_tag(&self, tag: &str) -> Result<Vec<SkillMetadata>, ServiceError>;
    async fn search_skills(&self, query: &str) -> Result<Vec<SkillMetadata>, ServiceError>;
    async fn get_available_capabilities(&self) -> Result<Vec<String>, ServiceError>;
    async fn get_skill_frontmatter(&self, skill_id: &str)
        -> Result<SkillFrontmatter, ServiceError>;
}

pub struct MetadataServiceImpl {
    skill_manager: Arc<dyn SkillManagementService>,
}

impl MetadataServiceImpl {
    pub fn new(skill_manager: Arc<dyn SkillManagementService>) -> Self {
        Self { skill_manager }
    }

    /// Simple text search implementation
    fn matches_query(&self, skill: &SkillDefinition, query: &str) -> bool {
        let query_lower = query.to_lowercase();

        // Check if query is contained in name or description (exact match)
        if skill.name.to_lowercase().contains(&query_lower)
            || skill.description.to_lowercase().contains(&query_lower)
        {
            return true;
        }

        // Tokenize query and check for word matches
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        for word in query_words {
            if word.len() > 2 {
                // Only match words longer than 2 characters
                if skill.name.to_lowercase().contains(word)
                    || skill.description.to_lowercase().contains(word)
                    || skill.tags.iter().any(|tag| tag.to_lowercase().contains(word))
                    || skill.capabilities.iter().any(|cap| cap.to_lowercase().contains(word))
                {
                    return true;
                }
            }
        }

        false
    }

    /// Score skills based on relevance to query
    fn score_skill(&self, skill: &SkillDefinition, query: &str) -> f32 {
        let query_lower = query.to_lowercase();
        let mut score = 0.0;

        // Exact name match gets highest score
        if skill.name.to_lowercase() == query_lower {
            score += 1.0;
        } else if skill.name.to_lowercase().contains(&query_lower) {
            score += 0.8;
        }

        // Description match
        if skill.description.to_lowercase().contains(&query_lower) {
            score += 0.6;
        }

        // Tag match
        for tag in &skill.tags {
            if tag.to_lowercase().contains(&query_lower) {
                score += 0.4;
            }
        }

        // Capability match
        for cap in &skill.capabilities {
            if cap.to_lowercase().contains(&query_lower) {
                score += 0.5;
            }
        }

        score
    }

    /// Parse YAML frontmatter from SKILL.md content
    fn parse_frontmatter(&self, content: &str) -> Result<SkillFrontmatter, ServiceError> {
        parse_yaml_frontmatter(content)
    }
}

/// Parse YAML frontmatter from SKILL.md content (standalone function)
/// This can be used by CLI and other modules that need to parse skill frontmatter
pub fn parse_yaml_frontmatter(content: &str) -> Result<SkillFrontmatter, ServiceError> {
    let lines: Vec<&str> = content.lines().collect();
    let mut frontmatter_lines = Vec::new();
    let mut in_frontmatter = false;
    let _frontmatter_end;

    // Find frontmatter boundaries (between --- delimiters)
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
            } else {
                _frontmatter_end = Some(i);
                break;
            }
        } else if in_frontmatter {
            frontmatter_lines.push(*line);
        }
    }

    // If no frontmatter found, create from existing metadata or return error
    if frontmatter_lines.is_empty() {
        // Try to parse as YAML without delimiters (some skills might not use ---)
        // For now, return an error if no frontmatter delimiters found
        return Err(ServiceError::Custom(
            "No YAML frontmatter found in SKILL.md".to_string(),
        ));
    }

    // Parse YAML frontmatter
    let frontmatter_str = frontmatter_lines.join("\n");
    let mut frontmatter: std::collections::HashMap<String, serde_yaml::Value> =
        serde_yaml::from_str(&frontmatter_str).map_err(|e| {
            ServiceError::Custom(format!("Failed to parse YAML frontmatter: {}", e))
        })?;

    // Extract known fields
    let name = frontmatter
        .remove("name")
        .and_then(|v| serde_yaml::from_value(v).ok())
        .unwrap_or_else(|| "Unknown".to_string());

    let description = frontmatter
        .remove("description")
        .and_then(|v| serde_yaml::from_value(v).ok())
        .unwrap_or_else(|| "No description".to_string());

    let version = frontmatter
        .remove("version")
        .and_then(|v| serde_yaml::from_value(v).ok())
        .unwrap_or_else(|| "1.0.0".to_string());

    let author = frontmatter.remove("author").and_then(|v| serde_yaml::from_value(v).ok());

    let tags = frontmatter
        .remove("tags")
        .and_then(|v| serde_yaml::from_value(v).ok())
        .unwrap_or_default();

    let capabilities = frontmatter
        .remove("capabilities")
        .and_then(|v| serde_yaml::from_value(v).ok())
        .unwrap_or_default();

    Ok(SkillFrontmatter {
        name,
        description,
        version,
        author,
        tags,
        capabilities,
        license: frontmatter.remove("license").and_then(|v| serde_yaml::from_value(v).ok()),
        compatibility: frontmatter
            .remove("compatibility")
            .and_then(|v| serde_yaml::from_value(v).ok()),
        metadata: frontmatter.remove("metadata").and_then(|v| serde_yaml::from_value(v).ok()),
        allowed_tools: frontmatter
            .remove("allowed_tools")
            .and_then(|v| serde_yaml::from_value(v).ok()),
        extra: frontmatter,
    })
}

#[async_trait]
impl MetadataService for MetadataServiceImpl {
    async fn discover_skills(&self, query: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        let all_skills = self.skill_manager.list_skills(None).await?;

        // Filter and score skills based on query relevance
        let mut scored_skills: Vec<(f32, &SkillDefinition)> = all_skills
            .iter()
            .filter(|skill| self.matches_query(skill, query))
            .map(|skill| (self.score_skill(skill, query), skill))
            .collect();

        // Sort by score (highest first)
        // Handle potential NaN values gracefully
        scored_skills.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Convert to metadata and return top matches
        let results: Vec<SkillMetadata> = scored_skills
            .into_iter()
            .take(10) // Limit to top 10 results
            .map(|(_, skill)| SkillMetadata::from(skill))
            .collect();

        Ok(results)
    }

    async fn find_skills_by_capability(
        &self,
        capability: &str,
    ) -> Result<Vec<SkillMetadata>, ServiceError> {
        let all_skills = self.skill_manager.list_skills(None).await?;

        let results: Vec<SkillMetadata> = all_skills
            .iter()
            .filter(|skill| skill.capabilities.iter().any(|cap| cap == capability))
            .map(SkillMetadata::from)
            .collect();

        Ok(results)
    }

    async fn find_skills_by_tag(&self, tag: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        let all_skills = self.skill_manager.list_skills(None).await?;

        let results: Vec<SkillMetadata> = all_skills
            .iter()
            .filter(|skill| skill.tags.iter().any(|t| t == tag))
            .map(SkillMetadata::from)
            .collect();

        Ok(results)
    }

    async fn search_skills(&self, query: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        // For now, use the same logic as discover_skills
        self.discover_skills(query).await
    }

    async fn get_available_capabilities(&self) -> Result<Vec<String>, ServiceError> {
        let all_skills = self.skill_manager.list_skills(None).await?;

        let mut capabilities: Vec<String> = all_skills
            .iter()
            .flat_map(|skill| skill.capabilities.iter().cloned())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        capabilities.sort();
        Ok(capabilities)
    }

    async fn get_skill_frontmatter(
        &self,
        skill_id: &str,
    ) -> Result<SkillFrontmatter, ServiceError> {
        // Get skill definition to find the SKILL.md file path
        let skill_id_parsed = crate::core::service::SkillId::new(skill_id.to_string())?;
        let skill = self
            .skill_manager
            .get_skill(&skill_id_parsed)
            .await?
            .ok_or_else(|| ServiceError::Custom(format!("Skill not found: {}", skill_id)))?;

        let skill_file = &skill.skill_file;

        // Read SKILL.md file
        if !skill_file.exists() {
            return Err(ServiceError::Custom(format!(
                "Skill file not found: {}",
                skill_file.display()
            )));
        }

        let content = tokio::fs::read_to_string(skill_file).await?;

        // Extract and parse YAML frontmatter
        self.parse_frontmatter(&content)
    }
}

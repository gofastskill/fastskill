//! Routing and context management service implementation

use crate::core::metadata::{MetadataService, SkillMetadata};
use crate::core::service::ServiceError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutedSkill {
    pub skill_id: String,
    pub relevance_score: f32,
    // Add other fields as needed
}

#[async_trait]
pub trait RoutingService: Send + Sync {
    async fn find_relevant_skills(
        &self,
        query: &str,
        context: Option<QueryContext>,
    ) -> Result<Vec<RoutedSkill>, ServiceError>;
    // Add other methods as needed
}

#[derive(Debug, Clone)]
pub struct QueryContext {
    /// Available tokens for loading content
    pub available_tokens: Option<usize>,
    /// Previous conversation context
    pub conversation_history: Option<Vec<String>>,
    /// User preferences
    pub user_preferences: Option<std::collections::HashMap<String, String>>,
}

pub struct RoutingServiceImpl {
    metadata_service: Arc<dyn MetadataService>,
}

impl RoutingServiceImpl {
    pub fn new(metadata_service: Arc<dyn MetadataService>) -> Self {
        Self { metadata_service }
    }

    /// Score skills based on relevance to query using metadata service
    async fn score_skills_for_query(
        &self,
        skills: &[SkillMetadata],
        query: &str,
        context: Option<&QueryContext>,
    ) -> Vec<(f32, SkillMetadata)> {
        let mut scored_skills = Vec::new();

        for skill in skills {
            let score = self.calculate_relevance_score(skill, query, context);
            scored_skills.push((score, skill.clone()));
        }

        // Sort by score (highest first)
        // Handle potential NaN values gracefully
        scored_skills.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored_skills
    }

    /// Calculate relevance score for a skill given a query
    fn calculate_relevance_score(
        &self,
        skill: &SkillMetadata,
        query: &str,
        context: Option<&QueryContext>,
    ) -> f32 {
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

        // Tag matches
        for tag in &skill.tags {
            if tag.to_lowercase().contains(&query_lower) {
                score += 0.4;
            }
        }

        // Capability matches
        for capability in &skill.capabilities {
            if capability.to_lowercase().contains(&query_lower) {
                score += 0.5;
            }
        }

        // Token efficiency bonus (prefer skills with fewer tokens when context is limited)
        if let Some(context) = context {
            if let Some(available_tokens) = context.available_tokens {
                if available_tokens < 2000 && skill.token_estimate < 100 {
                    score += 0.2; // Bonus for lightweight skills when tokens are limited
                }
            }
        }

        score
    }
}

#[async_trait]
impl RoutingService for RoutingServiceImpl {
    async fn find_relevant_skills(
        &self,
        query: &str,
        context: Option<QueryContext>,
    ) -> Result<Vec<RoutedSkill>, ServiceError> {
        // Get all skills from metadata service
        let all_metadata = self.metadata_service.discover_skills("").await?;

        // Score and rank skills
        let scored_skills =
            self.score_skills_for_query(&all_metadata, query, context.as_ref()).await;

        // Convert to routed skills and limit results
        let routed_skills: Vec<RoutedSkill> = scored_skills
            .into_iter()
            .take(20) // Limit to top 20 results
            .map(|(score, metadata)| RoutedSkill {
                skill_id: metadata.id.to_string(),
                relevance_score: score,
            })
            .collect();

        Ok(routed_skills)
    }
}

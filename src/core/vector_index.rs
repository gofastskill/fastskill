//! Vector index service for storing and searching skill embeddings

use crate::core::service::ServiceError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A skill stored in the vector index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedSkill {
    /// Unique skill identifier
    pub id: String,
    /// Path to the skill directory
    pub skill_path: PathBuf,
    /// Frontmatter data as JSON
    pub frontmatter_json: serde_json::Value,
    /// Vector embedding
    pub embedding: Vec<f32>,
    /// SHA256 hash of the SKILL.md file
    pub file_hash: String,
    /// Last updated timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Search result with similarity score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMatch {
    /// The indexed skill
    pub skill: IndexedSkill,
    /// Similarity score (0.0 to 1.0, higher is more similar)
    pub similarity: f32,
}

/// Vector index service trait
#[async_trait]
pub trait VectorIndexService: Send + Sync {
    /// Add or update a skill in the index
    async fn add_or_update_skill(
        &self,
        skill_id: &str,
        skill_path: PathBuf,
        frontmatter_json: serde_json::Value,
        embedding: Vec<f32>,
        file_hash: &str,
    ) -> Result<(), ServiceError>;

    /// Search for similar skills using vector similarity
    async fn search_similar(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SkillMatch>, ServiceError>;

    /// Get a skill by ID
    async fn get_skill_by_id(&self, skill_id: &str) -> Result<Option<IndexedSkill>, ServiceError>;

    /// Remove a skill from the index
    async fn remove_skill(&self, skill_id: &str) -> Result<(), ServiceError>;

    /// Get all skills in the index
    async fn get_all_skills(&self) -> Result<Vec<IndexedSkill>, ServiceError>;
}

/// SQLite-based vector index service implementation
pub struct VectorIndexServiceImpl {
    /// Path to the SQLite database file
    db_path: PathBuf,
}

impl VectorIndexServiceImpl {
    /// Create a new vector index service
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    /// Create a new service with default index path
    pub fn with_default_path(skill_dir: &std::path::Path) -> Self {
        let index_path = skill_dir.join(".fastskill").join("index.db");
        Self::new(index_path)
    }

    /// Create a new service with configured index path
    pub fn with_config(
        config: &crate::core::service::EmbeddingConfig,
        skill_dir: &std::path::Path,
    ) -> Self {
        let index_path = config
            .index_path
            .clone()
            .unwrap_or_else(|| skill_dir.join(".fastskill").join("index.db"));
        Self::new(index_path)
    }

    /// Ensure the database schema is created
    async fn ensure_schema(&self) -> Result<(), ServiceError> {
        let db_path = self.db_path.clone();

        // Run database operations in a blocking task
        tokio::task::spawn_blocking(move || {
            // Create parent directory if it doesn't exist
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ServiceError::Custom(format!("Failed to create database directory: {}", e))
                })?;
            }

            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| ServiceError::Custom(format!("Failed to open database: {}", e)))?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS skills (
                    id TEXT PRIMARY KEY,
                    skill_path TEXT NOT NULL,
                    frontmatter_json TEXT NOT NULL,
                    embedding_json TEXT NOT NULL,
                    file_hash TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                )",
                [],
            )
            .map_err(|e| ServiceError::Custom(format!("Failed to create schema: {}", e)))?;

            // Create index for faster lookups
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_updated_at ON skills(updated_at)",
                [],
            )
            .map_err(|e| ServiceError::Custom(format!("Failed to create index: {}", e)))?;

            Ok(())
        })
        .await
        .map_err(|e| ServiceError::Custom(format!("Database task failed: {}", e)))?
    }

    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let mut dot_product = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;

        for i in 0..a.len() {
            dot_product += a[i] * b[i];
            norm_a += a[i] * a[i];
            norm_b += b[i] * b[i];
        }

        let norm_a = norm_a.sqrt();
        let norm_b = norm_b.sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot_product / (norm_a * norm_b)
        }
    }
}

#[async_trait]
impl VectorIndexService for VectorIndexServiceImpl {
    async fn add_or_update_skill(
        &self,
        skill_id: &str,
        skill_path: PathBuf,
        frontmatter_json: serde_json::Value,
        embedding: Vec<f32>,
        file_hash: &str,
    ) -> Result<(), ServiceError> {
        self.ensure_schema().await?;

        let db_path = self.db_path.clone();
        let skill_id = skill_id.to_string();
        let skill_path_str = skill_path.to_string_lossy().to_string();
        let frontmatter_str = serde_json::to_string(&frontmatter_json)
            .map_err(|e| ServiceError::Custom(format!("Failed to serialize frontmatter: {}", e)))?;
        let embedding_str = serde_json::to_string(&embedding)
            .map_err(|e| ServiceError::Custom(format!("Failed to serialize embedding: {}", e)))?;
        let file_hash = file_hash.to_string();
        let updated_at = chrono::Utc::now().to_rfc3339();

        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| ServiceError::Custom(format!("Failed to open database: {}", e)))?;

            conn.execute(
                "INSERT OR REPLACE INTO skills (id, skill_path, frontmatter_json, embedding_json, file_hash, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    skill_id,
                    skill_path_str,
                    frontmatter_str,
                    embedding_str,
                    file_hash,
                    updated_at
                ],
            )
            .map_err(|e| ServiceError::Custom(format!("Failed to insert skill: {}", e)))?;

            Ok(())
        })
        .await
        .map_err(|e| ServiceError::Custom(format!("Database task failed: {}", e)))?
    }

    async fn search_similar(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SkillMatch>, ServiceError> {
        self.ensure_schema().await?;

        let db_path = self.db_path.clone();
        let query_embedding = query_embedding.to_vec();

        let skills = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| ServiceError::Custom(format!("Failed to open database: {}", e)))?;

            let mut stmt = conn
                .prepare("SELECT id, skill_path, frontmatter_json, embedding_json, file_hash, updated_at FROM skills")
                .map_err(|e| ServiceError::Custom(format!("Failed to prepare query: {}", e)))?;

            let skill_iter = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let skill_path: String = row.get(1)?;
                let frontmatter_str: String = row.get(2)?;
                let embedding_str: String = row.get(3)?;
                let file_hash: String = row.get(4)?;
                let updated_at_str: String = row.get(5)?;

                let frontmatter_json: serde_json::Value = serde_json::from_str(&frontmatter_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                let embedding: Vec<f32> = serde_json::from_str(&embedding_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&chrono::Utc);

                Ok(IndexedSkill {
                    id,
                    skill_path: PathBuf::from(skill_path),
                    frontmatter_json,
                    embedding,
                    file_hash,
                    updated_at,
                })
            })
            .map_err(|e| ServiceError::Custom(format!("Failed to query skills: {}", e)))?;

            let mut skills = Vec::new();
            for skill in skill_iter {
                skills.push(skill.map_err(|e| ServiceError::Custom(format!("Failed to parse skill: {}", e)))?);
            }

            Ok::<Vec<IndexedSkill>, ServiceError>(skills)
        })
        .await
        .map_err(|e| ServiceError::Custom(format!("Database task failed: {}", e)))??;

        // Calculate similarities and sort
        let mut matches: Vec<SkillMatch> = skills
            .into_iter()
            .map(|skill| {
                let similarity = Self::cosine_similarity(&query_embedding, &skill.embedding);
                SkillMatch { skill, similarity }
            })
            .collect();

        // Sort by similarity (highest first)
        // Handle potential NaN values gracefully
        matches.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Take top results
        matches.truncate(limit);

        Ok(matches)
    }

    async fn get_skill_by_id(&self, skill_id: &str) -> Result<Option<IndexedSkill>, ServiceError> {
        self.ensure_schema().await?;

        let db_path = self.db_path.clone();
        let skill_id = skill_id.to_string();

        let skill = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| ServiceError::Custom(format!("Failed to open database: {}", e)))?;

            let mut stmt = conn
                .prepare("SELECT id, skill_path, frontmatter_json, embedding_json, file_hash, updated_at FROM skills WHERE id = ?")
                .map_err(|e| ServiceError::Custom(format!("Failed to prepare query: {}", e)))?;

            let mut rows = stmt.query_map([skill_id], |row| {
                let id: String = row.get(0)?;
                let skill_path: String = row.get(1)?;
                let frontmatter_str: String = row.get(2)?;
                let embedding_str: String = row.get(3)?;
                let file_hash: String = row.get(4)?;
                let updated_at_str: String = row.get(5)?;

                let frontmatter_json: serde_json::Value = serde_json::from_str(&frontmatter_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                let embedding: Vec<f32> = serde_json::from_str(&embedding_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&chrono::Utc);

                Ok(IndexedSkill {
                    id,
                    skill_path: PathBuf::from(skill_path),
                    frontmatter_json,
                    embedding,
                    file_hash,
                    updated_at,
                })
            })
            .map_err(|e| ServiceError::Custom(format!("Failed to query skill: {}", e)))?;

            match rows.next() {
                Some(result) => Ok(Some(result.map_err(|e| ServiceError::Custom(format!("Failed to parse skill: {}", e)))?)),
                None => Ok(None),
            }
        })
        .await
        .map_err(|e| ServiceError::Custom(format!("Database task failed: {}", e)))?;

        skill
    }

    async fn remove_skill(&self, skill_id: &str) -> Result<(), ServiceError> {
        self.ensure_schema().await?;

        let db_path = self.db_path.clone();
        let skill_id = skill_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| ServiceError::Custom(format!("Failed to open database: {}", e)))?;

            conn.execute("DELETE FROM skills WHERE id = ?", [skill_id])
                .map_err(|e| ServiceError::Custom(format!("Failed to delete skill: {}", e)))?;

            Ok(())
        })
        .await
        .map_err(|e| ServiceError::Custom(format!("Database task failed: {}", e)))?
    }

    async fn get_all_skills(&self) -> Result<Vec<IndexedSkill>, ServiceError> {
        self.ensure_schema().await?;

        let db_path = self.db_path.clone();

        let skills = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&db_path)
                .map_err(|e| ServiceError::Custom(format!("Failed to open database: {}", e)))?;

            let mut stmt = conn
                .prepare("SELECT id, skill_path, frontmatter_json, embedding_json, file_hash, updated_at FROM skills")
                .map_err(|e| ServiceError::Custom(format!("Failed to prepare query: {}", e)))?;

            let skill_iter = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let skill_path: String = row.get(1)?;
                let frontmatter_str: String = row.get(2)?;
                let embedding_str: String = row.get(3)?;
                let file_hash: String = row.get(4)?;
                let updated_at_str: String = row.get(5)?;

                let frontmatter_json: serde_json::Value = serde_json::from_str(&frontmatter_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                let embedding: Vec<f32> = serde_json::from_str(&embedding_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
                let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?
                    .with_timezone(&chrono::Utc);

                Ok(IndexedSkill {
                    id,
                    skill_path: PathBuf::from(skill_path),
                    frontmatter_json,
                    embedding,
                    file_hash,
                    updated_at,
                })
            })
            .map_err(|e| ServiceError::Custom(format!("Failed to query skills: {}", e)))?;

            let mut skills = Vec::new();
            for skill in skill_iter {
                skills.push(skill.map_err(|e| ServiceError::Custom(format!("Failed to parse skill: {}", e)))?);
            }

            Ok::<Vec<IndexedSkill>, ServiceError>(skills)
        })
        .await
        .map_err(|e| ServiceError::Custom(format!("Database task failed: {}", e)))??;

        Ok(skills)
    }
}

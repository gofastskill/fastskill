//! Core reindex seam (ADR-0002/0005).
//!
//! Reindex is domain logic (skills dir → Vector index) and lives in core. It runs
//! only when an [`EmbeddingService`](crate::core::embedding::EmbeddingService) has
//! been injected (edge-constructed with the API key); otherwise it **skips
//! silently**. The CLI/serve edge injects the provider via
//! [`FastSkillService::with_embedding_service`].

use crate::core::embedding::EmbeddingService;
use crate::core::metadata::parse_yaml_frontmatter;
use crate::core::service::{FastSkillService, ServiceError};
use crate::core::vector_index::VectorIndexService;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Progress datum emitted per skill as reindex proceeds. The core emits neutral
/// data; the caller (CLI) decides how to render it (HTTP passes no observer).
#[derive(Debug, Clone)]
pub struct ReindexProgress {
    pub current: usize,
    pub total: usize,
    pub skill_id: String,
}

/// Outcome of a reindex call. `reindexed=false` + a `reason` means it was skipped
/// (no provider), which is a success, not a failure (ADR-0002).
#[derive(Debug, Clone)]
pub struct ReindexOutcome {
    pub reindexed: bool,
    pub count: usize,
    pub reason: Option<String>,
}

impl ReindexOutcome {
    pub(crate) fn skipped(reason: &str) -> Self {
        ReindexOutcome {
            reindexed: false,
            count: 0,
            reason: Some(reason.to_string()),
        }
    }
}

impl FastSkillService {
    /// Reindex the vector index for `skills_dir` (defaults to the configured skill
    /// storage path). Skips silently with an outcome reason when no embedding
    /// provider is injected. `observer`, if provided, is called once per skill.
    pub async fn reindex(
        &self,
        skills_dir: Option<&Path>,
        observer: Option<&(dyn Fn(ReindexProgress) + Send + Sync)>,
    ) -> Result<ReindexOutcome, ServiceError> {
        // No provider ⇒ skip silently (the common, non-configured case).
        let Some(embedding_service) = self.embedding_service() else {
            return Ok(ReindexOutcome::skipped("no embedding provider configured"));
        };

        // The vector index is built alongside the embedding config, so this
        // should always be present once an embedding provider is injected. Guard
        // defensively anyway rather than unwrapping.
        let Some(vector_index_service) = self.vector_index_service() else {
            return Ok(ReindexOutcome::skipped("no vector index configured"));
        };

        let dir = skills_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.config().skill_storage_path.clone());

        tracing::info!(
            "Reindexing vector index for skills directory: {}",
            dir.display()
        );

        let skill_files = find_skill_files(&dir)?;

        if skill_files.is_empty() {
            tracing::info!("No skills found in {}", dir.display());
            return Ok(ReindexOutcome {
                reindexed: true,
                count: 0,
                reason: None,
            });
        }

        let total = skill_files.len();

        // Collect current skill IDs so stale index entries (skills removed from
        // disk since the last reindex) can be pruned below.
        let current_skill_ids: HashSet<String> = skill_files
            .iter()
            .filter_map(|f| skill_id_from_path(f))
            .collect();

        let mut count = 0usize;
        for (idx, skill_file) in skill_files.into_iter().enumerate() {
            let skill_id = skill_id_from_path(&skill_file).unwrap_or_else(|| "unknown".to_string());

            if let Some(obs) = observer {
                obs(ReindexProgress {
                    current: idx + 1,
                    total,
                    skill_id: skill_id.clone(),
                });
            }

            match index_skill_file(
                &skill_file,
                &skill_id,
                embedding_service.as_ref(),
                vector_index_service.as_ref(),
            )
            .await
            {
                Ok(true) => count += 1,
                Ok(false) => {
                    // Unchanged hash: nothing to do.
                }
                Err(e) => {
                    // A single skill failing to index should not abort the whole
                    // reindex run; log and continue with the rest.
                    tracing::warn!("Failed to reindex skill {}: {}", skill_id, e);
                }
            }
        }

        // Cleanup: remove skills from the index that are no longer on disk.
        match vector_index_service.get_all_skills().await {
            Ok(all_indexed_skills) => {
                for indexed_skill in all_indexed_skills {
                    if !current_skill_ids.contains(&indexed_skill.id) {
                        tracing::info!("Removing stale index entry: {}", indexed_skill.id);
                        if let Err(e) = vector_index_service.remove_skill(&indexed_skill.id).await {
                            tracing::warn!(
                                "Failed to remove stale index entry {}: {}",
                                indexed_skill.id,
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to retrieve all indexed skills for cleanup: {}", e);
            }
        }

        Ok(ReindexOutcome {
            reindexed: true,
            count,
            reason: None,
        })
    }
}

/// Derive a skill ID from a `SKILL.md` path: the name of its parent directory.
fn skill_id_from_path(skill_file: &Path) -> Option<String> {
    skill_file
        .parent()
        .and_then(|parent_dir| parent_dir.file_name())
        .map(|name| name.to_string_lossy().to_string())
}

/// Find all `SKILL.md` files under `skills_dir`.
fn find_skill_files(skills_dir: &Path) -> Result<Vec<PathBuf>, ServiceError> {
    if !skills_dir.exists() {
        return Err(ServiceError::Config(format!(
            "Skills directory does not exist: {}",
            skills_dir.display()
        )));
    }

    let skill_files = walkdir::WalkDir::new(skills_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.file_name() == "SKILL.md")
        .map(|e| e.path().to_path_buf())
        .collect();

    Ok(skill_files)
}

/// Index a single skill file. Returns `Ok(true)` if the index was updated,
/// `Ok(false)` if the skill was already up to date (unchanged file hash).
async fn index_skill_file(
    skill_file: &Path,
    skill_id: &str,
    embedding_service: &dyn EmbeddingService,
    vector_index_service: &dyn VectorIndexService,
) -> Result<bool, ServiceError> {
    let file_hash = calculate_file_hash(skill_file)?;

    // Skip re-embedding when the file is unchanged since the last index write.
    if let Ok(Some(indexed_skill)) = vector_index_service.get_skill_by_id(skill_id).await {
        if indexed_skill.file_hash == file_hash {
            return Ok(false);
        }
    }

    let skill_dir = skill_file.parent().ok_or_else(|| {
        ServiceError::Validation("Skill file has no parent directory".to_string())
    })?;

    let content = std::fs::read_to_string(skill_file)?;
    let frontmatter = parse_yaml_frontmatter(&content)?;

    let frontmatter_json = serde_json::to_value(&frontmatter)
        .map_err(|e| ServiceError::Validation(format!("Failed to serialize frontmatter: {}", e)))?;

    let embedding_text = format!("{}\n{}", frontmatter.name, frontmatter.description);
    let embedding = embedding_service.embed_text(&embedding_text).await?;

    vector_index_service
        .add_or_update_skill(
            skill_id,
            skill_dir.to_path_buf(),
            frontmatter_json,
            embedding,
            &file_hash,
        )
        .await?;

    Ok(true)
}

/// Calculate the SHA256 hash of a file's contents.
fn calculate_file_hash(file_path: &Path) -> Result<String, ServiceError> {
    let content = std::fs::read(file_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let hash_bytes = hasher.finalize();
    Ok(format!("{:x}", hash_bytes))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::assertions_on_constants
)]
mod tests {
    use super::*;
    use crate::core::service::{EmbeddingConfig, ServiceConfig};
    use async_trait::async_trait;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Deterministic, network-free embedding provider for tests.
    struct MockEmbeddingService {
        calls: AtomicUsize,
    }

    impl MockEmbeddingService {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl EmbeddingService for MockEmbeddingService {
        async fn embed_text(&self, text: &str) -> Result<Vec<f32>, ServiceError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![text.len() as f32, 0.0, 0.0])
        }

        async fn embed_query(&self, query: &str) -> Result<Vec<f32>, ServiceError> {
            self.embed_text(query).await
        }
    }

    fn create_test_skill(skills_dir: &Path, skill_id: &str, name: &str, description: &str) {
        let skill_dir = skills_dir.join(skill_id);
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = format!(
            r#"---
name: {}
description: {}
version: 1.0.0
---

# {}

Test skill content"#,
            name, description, name
        );
        fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();
    }

    #[tokio::test]
    async fn test_reindex_skips_without_embedding_provider() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            embedding: None,
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let outcome = service.reindex(None, None).await.unwrap();

        assert!(!outcome.reindexed);
        assert_eq!(outcome.count, 0);
        assert!(outcome.reason.is_some());
    }

    #[tokio::test]
    async fn test_reindex_indexes_skills_with_injected_provider() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        create_test_skill(&skills_dir, "skill-one", "Skill One", "First test skill");
        create_test_skill(&skills_dir, "skill-two", "Skill Two", "Second test skill");

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            embedding: Some(EmbeddingConfig {
                openai_base_url: "https://api.openai.com/v1".to_string(),
                embedding_model: "text-embedding-3-small".to_string(),
                index_path: None,
            }),
            ..Default::default()
        };

        let mock_embedding = Arc::new(MockEmbeddingService::new());
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(mock_embedding.clone());
        service.initialize().await.unwrap();

        let progress_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let progress_calls_clone = progress_calls.clone();
        let observer = move |p: ReindexProgress| {
            progress_calls_clone.lock().unwrap().push(p.skill_id);
        };

        let outcome = service
            .reindex(Some(&skills_dir), Some(&observer))
            .await
            .unwrap();

        assert!(outcome.reindexed);
        assert_eq!(outcome.count, 2);
        assert!(outcome.reason.is_none());
        assert_eq!(mock_embedding.call_count(), 2);
        assert_eq!(progress_calls.lock().unwrap().len(), 2);

        // Verify the skills actually landed in the vector index.
        let vector_index = service.vector_index_service().unwrap();
        assert!(vector_index
            .get_skill_by_id("skill-one")
            .await
            .unwrap()
            .is_some());
        assert!(vector_index
            .get_skill_by_id("skill-two")
            .await
            .unwrap()
            .is_some());

        // Reindexing again without changes should skip re-embedding (unchanged hash).
        let outcome2 = service.reindex(Some(&skills_dir), None).await.unwrap();
        assert!(outcome2.reindexed);
        assert_eq!(outcome2.count, 0);
        assert_eq!(mock_embedding.call_count(), 2);
    }

    #[tokio::test]
    async fn test_reindex_removes_stale_entries() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        create_test_skill(&skills_dir, "skill-one", "Skill One", "First test skill");
        let skill_two_dir = skills_dir.join("skill-two");
        create_test_skill(&skills_dir, "skill-two", "Skill Two", "Second test skill");

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            embedding: Some(EmbeddingConfig {
                openai_base_url: "https://api.openai.com/v1".to_string(),
                embedding_model: "text-embedding-3-small".to_string(),
                index_path: None,
            }),
            ..Default::default()
        };

        let mock_embedding = Arc::new(MockEmbeddingService::new());
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(mock_embedding);
        service.initialize().await.unwrap();

        let outcome = service.reindex(Some(&skills_dir), None).await.unwrap();
        assert_eq!(outcome.count, 2);

        fs::remove_dir_all(&skill_two_dir).unwrap();

        let outcome2 = service.reindex(Some(&skills_dir), None).await.unwrap();
        assert_eq!(outcome2.count, 0);

        let vector_index = service.vector_index_service().unwrap();
        assert!(vector_index
            .get_skill_by_id("skill-one")
            .await
            .unwrap()
            .is_some());
        assert!(vector_index
            .get_skill_by_id("skill-two")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_reindex_empty_skills_dir() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            embedding: Some(EmbeddingConfig {
                openai_base_url: "https://api.openai.com/v1".to_string(),
                embedding_model: "text-embedding-3-small".to_string(),
                index_path: None,
            }),
            ..Default::default()
        };

        let mock_embedding = Arc::new(MockEmbeddingService::new());
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(mock_embedding);
        service.initialize().await.unwrap();

        let outcome = service.reindex(Some(&skills_dir), None).await.unwrap();
        assert!(outcome.reindexed);
        assert_eq!(outcome.count, 0);
        assert!(outcome.reason.is_none());
    }

    #[tokio::test]
    async fn test_reindex_nonexistent_skills_dir_errors() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            embedding: Some(EmbeddingConfig {
                openai_base_url: "https://api.openai.com/v1".to_string(),
                embedding_model: "text-embedding-3-small".to_string(),
                index_path: None,
            }),
            ..Default::default()
        };

        let mock_embedding = Arc::new(MockEmbeddingService::new());
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(mock_embedding);
        service.initialize().await.unwrap();

        let nonexistent_dir = temp_dir.path().join("nonexistent");
        let result = service.reindex(Some(&nonexistent_dir), None).await;

        assert!(result.is_err());
    }
}

//! Utilities for auto-reindex after skill mutations

use crate::error::CliResult;
use fastskill_core::FastSkillService;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LifecycleIndexResult {
    pub outcome: &'static str,
    pub count: usize,
    pub diagnostic: Option<String>,
}

/// Run derived indexing without writing progress to structured stdout.
/// Package state stays committed when indexing fails, and the caller embeds
/// the failure in its lifecycle result.
pub async fn lifecycle_reindex_result(
    service: &FastSkillService,
    command_name: &str,
    explicit_reindex: bool,
    explicit_no_reindex: bool,
    config_auto_reindex: bool,
) -> LifecycleIndexResult {
    let skip_reason = if explicit_no_reindex {
        Some("disabled by --no-reindex")
    } else if service.config().embedding.is_none() {
        Some("no embedding provider configured")
    } else if !(explicit_reindex || config_auto_reindex) {
        Some("automatic indexing is disabled")
    } else {
        None
    };
    if let Some(reason) = skip_reason {
        return LifecycleIndexResult {
            outcome: "skipped",
            count: 0,
            diagnostic: Some(reason.to_string()),
        };
    }
    match service.reindex(None, None).await {
        Ok(outcome) => LifecycleIndexResult {
            outcome: if outcome.reindexed {
                "succeeded"
            } else {
                "skipped"
            },
            count: outcome.count,
            diagnostic: outcome.reason,
        },
        Err(error) => LifecycleIndexResult {
            outcome: "failed",
            count: 0,
            diagnostic: Some(format!(
                "auto-reindex after '{command_name}' failed: {error}"
            )),
        },
    }
}

/// Disclose derived-index status after the package transaction has committed.
pub fn report_lifecycle_index_result(result: &LifecycleIndexResult) {
    match result.outcome {
        "succeeded" => crate::outln!("   Indexed {} skill(s)", result.count),
        "failed" => eprintln!(
            "Warning: {}",
            result
                .diagnostic
                .as_deref()
                .unwrap_or("derived indexing failed")
        ),
        _ => crate::outln!(
            "   Indexing skipped: {}",
            result.diagnostic.as_deref().unwrap_or("not requested")
        ),
    }
}

/// Run reindex if conditions are met; failures are non-fatal warnings.
pub async fn maybe_auto_reindex(
    service: &FastSkillService,
    command_name: &str,
    explicit_reindex: bool,
    explicit_no_reindex: bool,
    config_auto_reindex: bool,
    verbose: bool,
) -> CliResult<()> {
    if explicit_no_reindex {
        return Ok(());
    }

    if service.config().embedding.is_none() {
        if verbose {
            crate::outln!(
                "Note: skipping auto-reindex after '{}' (no embedding provider configured).",
                command_name
            );
        }
        return Ok(());
    }

    let should_reindex = explicit_reindex || config_auto_reindex;
    if !should_reindex {
        return Ok(());
    }

    let args = crate::commands::reindex::ReindexArgs {
        skills_dir: None,
        force: false,
        max_concurrent: 5,
        progress: false,
        no_progress: true,
    };

    if let Err(e) = crate::commands::reindex::execute_reindex(service, args).await {
        eprintln!(
            "Warning: auto-reindex after '{}' failed: {}",
            command_name, e
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastskill_core::{EmbeddingConfig, EmbeddingService, ServiceConfig, ServiceError};
    use std::sync::Arc;
    use tempfile::TempDir;

    struct TestEmbedding {
        fail: bool,
    }

    #[async_trait::async_trait]
    impl EmbeddingService for TestEmbedding {
        async fn embed_text(&self, text: &str) -> Result<Vec<f32>, ServiceError> {
            if self.fail {
                Err(ServiceError::Custom(
                    "fixture embedding failure".to_string(),
                ))
            } else {
                Ok(vec![text.len() as f32, 0.0, 0.0])
            }
        }

        async fn embed_query(&self, query: &str) -> Result<Vec<f32>, ServiceError> {
            self.embed_text(query).await
        }
    }

    async fn service(root: &TempDir, fail: bool, with_skill: bool) -> FastSkillService {
        let skills = root.path().join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        if with_skill {
            let skill = skills.join("demo");
            std::fs::create_dir_all(&skill).unwrap();
            std::fs::write(
                skill.join("SKILL.md"),
                "---\nname: demo\nversion: 1.0.0\ndescription: fixture\n---\n# Demo\n",
            )
            .unwrap();
        }
        let config = ServiceConfig {
            skill_storage_path: skills,
            embedding: Some(EmbeddingConfig {
                openai_base_url: "http://unused.invalid".to_string(),
                embedding_model: "fixture".to_string(),
                index_path: Some(root.path().join("index.db")),
            }),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(Arc::new(TestEmbedding { fail }));
        service.initialize().await.unwrap();
        service
    }

    #[tokio::test]
    async fn lifecycle_result_explains_every_skip_reason() {
        let root = TempDir::new().unwrap();
        let no_provider = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("no-provider"),
            ..Default::default()
        })
        .await
        .unwrap();
        let disabled = lifecycle_reindex_result(&no_provider, "test", false, true, false).await;
        assert_eq!(
            disabled.diagnostic.as_deref(),
            Some("disabled by --no-reindex")
        );
        let unavailable = lifecycle_reindex_result(&no_provider, "test", true, false, false).await;
        assert_eq!(
            unavailable.diagnostic.as_deref(),
            Some("no embedding provider configured")
        );

        let configured = service(&root, false, false).await;
        let automatic_off =
            lifecycle_reindex_result(&configured, "test", false, false, false).await;
        assert_eq!(
            automatic_off.diagnostic.as_deref(),
            Some("automatic indexing is disabled")
        );
    }

    #[tokio::test]
    async fn lifecycle_result_reports_success_empty_and_failure() {
        let success_root = TempDir::new().unwrap();
        let success = service(&success_root, false, true).await;
        let result = lifecycle_reindex_result(&success, "add", true, false, false).await;
        assert_eq!(result.outcome, "succeeded");
        assert_eq!(result.count, 1);

        let empty_root = TempDir::new().unwrap();
        let empty = service(&empty_root, false, false).await;
        let result = lifecycle_reindex_result(&empty, "add", true, false, false).await;
        assert_eq!(result.outcome, "succeeded");
        assert_eq!(result.count, 0);

        let failure_root = TempDir::new().unwrap();
        let failure = service(&failure_root, false, false).await;
        std::fs::remove_dir_all(&failure.config().skill_storage_path).unwrap();
        let result = lifecycle_reindex_result(&failure, "update", true, false, false).await;
        assert_eq!(result.outcome, "failed");
        assert!(result.diagnostic.unwrap().contains("does not exist"));
    }

    #[tokio::test]
    async fn human_index_status_discloses_success_and_no_provider() {
        let (_, output) = crate::output::capture(async {
            report_lifecycle_index_result(&LifecycleIndexResult {
                outcome: "succeeded",
                count: 2,
                diagnostic: None,
            });
            report_lifecycle_index_result(&LifecycleIndexResult {
                outcome: "skipped",
                count: 0,
                diagnostic: Some("no embedding provider configured".to_string()),
            });
        })
        .await;
        assert!(output.contains("Indexed 2 skill(s)"));
        assert!(output.contains("Indexing skipped: no embedding provider configured"));

        // Failure is deliberately non-fatal: package state is already committed.
        report_lifecycle_index_result(&LifecycleIndexResult {
            outcome: "failed",
            count: 0,
            diagnostic: Some("fixture failure".to_string()),
        });
    }

    #[tokio::test]
    async fn legacy_human_reindex_covers_skip_run_and_nonfatal_failure() {
        let root = TempDir::new().unwrap();
        let no_provider = FastSkillService::new(ServiceConfig {
            skill_storage_path: root.path().join("no-provider"),
            ..Default::default()
        })
        .await
        .unwrap();
        maybe_auto_reindex(&no_provider, "test", false, true, false, false)
            .await
            .unwrap();
        let (_, note) = crate::output::capture(async {
            maybe_auto_reindex(&no_provider, "test", true, false, false, true)
                .await
                .unwrap();
        })
        .await;
        assert!(note.contains("no embedding provider configured"));

        let configured_root = TempDir::new().unwrap();
        let configured = service(&configured_root, false, false).await;
        maybe_auto_reindex(&configured, "test", false, false, false, false)
            .await
            .unwrap();
        maybe_auto_reindex(&configured, "test", true, false, false, false)
            .await
            .unwrap();

        let missing_root = TempDir::new().unwrap();
        let missing = service(&missing_root, false, false).await;
        std::fs::remove_dir_all(&missing.config().skill_storage_path).unwrap();
        // The package mutation has already committed, so index failure remains
        // a warning and the lifecycle call itself succeeds.
        maybe_auto_reindex(&missing, "test", false, false, true, false)
            .await
            .unwrap();

        let failing = TestEmbedding { fail: true };
        assert!(failing.embed_query("fixture").await.is_err());
    }
}

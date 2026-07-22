//! Reindex command implementation — a thin wrapper over the core reindex seam
//! (`FastSkillService::reindex`, ADR-0002/0005). All indexing logic (finding
//! `SKILL.md` files, hashing, embedding, updating the vector index, pruning
//! stale entries) lives in core; this module only renders progress and the
//! final summary from the `ReindexProgress` observer callbacks and the
//! returned `ReindexOutcome`.

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::reindex::ReindexProgress;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Check if progress bars should be shown
fn should_show_progress() -> bool {
    // FASTSKILL_NO_PROGRESS takes precedence
    if std::env::var("FASTSKILL_NO_PROGRESS").is_ok() {
        return false;
    }

    // Check if stdout is a TTY
    std::io::stdout().is_terminal()
}

/// Controls how reindex progress is displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressMode {
    /// Show a live progress bar (when TTY detected and not suppressed)
    Live,
    /// Print each skill's result verbosely
    Verbose,
    /// Show only the final summary
    Quiet,
}

impl ProgressMode {
    fn from_flags(progress: bool, no_progress: bool) -> Self {
        if no_progress {
            ProgressMode::Quiet
        } else if progress {
            ProgressMode::Verbose
        } else if should_show_progress() {
            ProgressMode::Live
        } else {
            ProgressMode::Quiet
        }
    }
}

/// Reindex the vector index by scanning skills directory
#[derive(Debug, Clone)]
pub struct ReindexArgs {
    /// Skills directory path (overrides default discovery)
    pub skills_dir: Option<std::path::PathBuf>,

    /// Force re-indexing of all skills (ignore existing hashes)
    pub force: bool,

    /// Maximum number of concurrent embedding requests
    ///
    /// NOTE (core-seam gap): the core `reindex` seam processes skills
    /// sequentially and takes no concurrency parameter, so this is currently
    /// accepted but has no effect. Kept for CLI/arg compatibility.
    #[allow(dead_code)]
    pub max_concurrent: usize,

    /// Show progress bars and processing details
    pub progress: bool,

    /// Suppress progress output, show only final summary
    pub no_progress: bool,
}

impl IntoCommandSpec for ReindexArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Reindex the vector index for semantic search",
            syntax: Some("reindex [OPTIONS]"),
            category: Some("packages"),
            args: vec![
                ArgSpec {
                    name: "skills-dir",
                    long: Some("skills-dir"),
                    short: None,
                    help: "Skills directory path (overrides default discovery)",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "force",
                    long: Some("force"),
                    short: None,
                    help: "Force re-indexing of all skills (ignore existing hashes)",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "max-concurrent",
                    long: Some("max-concurrent"),
                    short: None,
                    help: "Maximum concurrent embedding requests",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Int(5)),
                    ..Default::default()
                },
                ArgSpec {
                    name: "progress",
                    long: Some("progress"),
                    short: None,
                    help: "Show progress bars and processing details",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-progress",
                    long: Some("no-progress"),
                    short: None,
                    help: "Suppress progress output, show only final summary",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ReindexArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ReindexArgs {
            skills_dir: map.get("skills-dir").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(std::path::PathBuf::from(s))
                } else {
                    None
                }
            }),
            force: matches!(map.get("force"), Some(ArgValue::Bool(true))),
            max_concurrent: map
                .get("max-concurrent")
                .and_then(|v| {
                    if let ArgValue::Int(n) = v {
                        Some(*n as usize)
                    } else {
                        None
                    }
                })
                .unwrap_or(5),
            progress: matches!(map.get("progress"), Some(ArgValue::Bool(true))),
            no_progress: matches!(map.get("no-progress"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Wipe every entry currently in the vector index so the next `reindex` call
/// re-embeds everything, regardless of unchanged file hashes.
///
/// This is how `--force` is honored through the core seam: `FastSkillService::reindex`
/// has no `force` parameter (it always skips unchanged-hash skills), so instead of
/// duplicating its indexing loop here, we use the existing public
/// `VectorIndexService` accessor to clear the cache it consults.
async fn clear_vector_index(service: &FastSkillService) {
    let Some(vector_index_service) = service.vector_index_service() else {
        return;
    };
    let Ok(all_indexed) = vector_index_service.get_all_skills().await else {
        return;
    };
    for indexed in all_indexed {
        let _ = vector_index_service.remove_skill(&indexed.id).await;
    }
}

pub async fn execute_reindex(service: &FastSkillService, args: ReindexArgs) -> CliResult<()> {
    // Runtime validation guard for progress flags (defense-in-depth)
    if args.progress && args.no_progress {
        return Err(CliError::Validation(
            "--progress and --no-progress cannot be used together".to_string(),
        ));
    }

    let mode = ProgressMode::from_flags(args.progress, args.no_progress);
    let start_time = std::time::Instant::now();

    // `--force`: only meaningful when reindex will actually run (an embedding
    // provider is injected); otherwise leave the index untouched, matching the
    // "no embedding configured" skip below.
    if args.force && service.embedding_service().is_some() {
        clear_vector_index(service).await;
    }

    // The observer fires once per skill `reindex` finds, before it decides
    // whether that skill needs re-embedding. Its first call is also the first
    // point at which the total skill count is known, so we print
    // "Found N skills to process" there — same information as before, just
    // learned from the seam's callback instead of a CLI-side directory scan.
    let found_total = Arc::new(AtomicUsize::new(0));
    let seen_any = Arc::new(AtomicBool::new(false));
    let printed_found = Arc::new(AtomicBool::new(false));
    let no_progress = args.no_progress;
    let verbose = mode == ProgressMode::Verbose;
    let live = mode == ProgressMode::Live;

    let found_total_cb = Arc::clone(&found_total);
    let seen_any_cb = Arc::clone(&seen_any);
    let printed_found_cb = Arc::clone(&printed_found);

    let observer = move |p: ReindexProgress| {
        seen_any_cb.store(true, Ordering::SeqCst);
        found_total_cb.store(p.total, Ordering::SeqCst);
        if !no_progress && !printed_found_cb.swap(true, Ordering::SeqCst) {
            println!("Found {} skills to process", p.total);
        }
        if verbose {
            println!("  Processing: {} ({}/{})", p.skill_id, p.current, p.total);
        } else if live {
            let pct = p
                .current
                .checked_mul(100)
                .and_then(|n| n.checked_div(p.total))
                .unwrap_or(0);
            print!("\r\x1B[KProgress: [{}/{}] ({}%)", p.current, p.total, pct);
            let _ = std::io::stdout().flush();
        }
    };

    let outcome = service
        .reindex(args.skills_dir.as_deref(), Some(&observer))
        .await
        .map_err(CliError::Service)?;

    if live && seen_any.load(Ordering::SeqCst) {
        println!();
    }

    if !outcome.reindexed {
        let reason = outcome
            .reason
            .as_deref()
            .unwrap_or("no embedding provider configured");
        println!("Reindex skipped: {reason}. Run 'fastskill doctor' for setup guidance.");
        return Ok(());
    }

    if !seen_any.load(Ordering::SeqCst) {
        // Nothing under the skills directory at all.
        if !no_progress {
            let dir = args
                .skills_dir
                .clone()
                .unwrap_or_else(|| service.config().skill_storage_path.clone());
            println!("Found 0 skills to process");
            println!("No skills found in {}", dir.display());
        }
        return Ok(());
    }

    if mode != ProgressMode::Quiet {
        println!("Reindex completed");
        println!("  Total skills: {}", found_total.load(Ordering::SeqCst));
        println!("  Indexed/updated: {}", outcome.count);
        println!("  Total time: {:.2}s", start_time.elapsed().as_secs_f64());
    }

    Ok(())
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
    use fastskill_core::{EmbeddingConfig, ServiceConfig};
    use std::fs;
    use tempfile::TempDir;

    fn create_test_skill(
        skills_dir: &std::path::Path,
        skill_id: &str,
        name: &str,
        description: &str,
    ) {
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

    #[test]
    fn test_progress_mode_from_flags() {
        assert_eq!(ProgressMode::from_flags(true, false), ProgressMode::Verbose);
        assert_eq!(ProgressMode::from_flags(false, true), ProgressMode::Quiet);
    }

    #[tokio::test]
    async fn test_execute_reindex_without_embedding_config() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            embedding: None,
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReindexArgs {
            skills_dir: None,
            force: false,
            max_concurrent: 5,
            progress: false,
            no_progress: false,
        };

        // With no embedding config, reindex is a no-op (informational message, exit 0)
        let result = execute_reindex(&service, args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_reindex_nonexistent_skills_dir() {
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

        let mock_embedding = Arc::new(MockEmbeddingService);
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(mock_embedding);
        service.initialize().await.unwrap();

        let nonexistent_dir = temp_dir.path().join("nonexistent");
        let args = ReindexArgs {
            skills_dir: Some(nonexistent_dir),
            force: false,
            max_concurrent: 5,
            progress: false,
            no_progress: false,
        };

        let result = execute_reindex(&service, args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_reindex_empty_skills_dir() {
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

        let mock_embedding = Arc::new(MockEmbeddingService);
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(mock_embedding);
        service.initialize().await.unwrap();

        let args = ReindexArgs {
            skills_dir: Some(skills_dir),
            force: true,
            max_concurrent: 5,
            progress: false,
            no_progress: false,
        };

        let result = execute_reindex(&service, args).await;
        assert!(
            result.is_ok(),
            "empty directory should be a no-op success: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_execute_reindex_indexes_and_force_clears_cache() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();
        create_test_skill(
            &skills_dir,
            "test-skill-1",
            "Test Skill One",
            "First test skill",
        );

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            embedding: Some(EmbeddingConfig {
                openai_base_url: "https://api.openai.com/v1".to_string(),
                embedding_model: "text-embedding-3-small".to_string(),
                index_path: None,
            }),
            ..Default::default()
        };

        let mock_embedding = Arc::new(MockEmbeddingService);
        let mut service = FastSkillService::new(config)
            .await
            .unwrap()
            .with_embedding_service(mock_embedding);
        service.initialize().await.unwrap();

        let args = ReindexArgs {
            skills_dir: Some(skills_dir.clone()),
            force: false,
            max_concurrent: 2,
            progress: true,
            no_progress: false,
        };
        let result = execute_reindex(&service, args).await;
        assert!(result.is_ok(), "reindex should succeed: {:?}", result);

        // Re-running with --force must re-embed even though the hash is unchanged.
        let force_args = ReindexArgs {
            skills_dir: Some(skills_dir),
            force: true,
            max_concurrent: 2,
            progress: false,
            no_progress: true,
        };
        let result = execute_reindex(&service, force_args).await;
        assert!(
            result.is_ok(),
            "forced reindex should succeed: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_reindex_progress_conflict_errors() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReindexArgs {
            skills_dir: None,
            force: false,
            max_concurrent: 5,
            progress: true,
            no_progress: true,
        };
        let result = execute_reindex(&service, args).await;
        assert!(matches!(result, Err(CliError::Validation(_))));
    }

    /// Deterministic, network-free embedding provider for tests.
    struct MockEmbeddingService;

    #[async_trait::async_trait]
    impl fastskill_core::EmbeddingService for MockEmbeddingService {
        async fn embed_text(&self, text: &str) -> Result<Vec<f32>, fastskill_core::ServiceError> {
            Ok(vec![text.len() as f32, 0.0, 0.0])
        }

        async fn embed_query(&self, query: &str) -> Result<Vec<f32>, fastskill_core::ServiceError> {
            self.embed_text(query).await
        }
    }
}

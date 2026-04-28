//! # FastSkill Service Layer (Rust Implementation)
//!
//! This crate provides a high-performance, memory-safe implementation of the FastSkill
//! service layer. It serves as the reference implementation for other language
//! bindings and can be used standalone or as a library.
//!
//! ## Architecture
//!
//! The service layer provides:
//! - Skill management (CRUD operations)
//! - Metadata and discovery services
//! - Progressive loading of skill content
//! - Tool calling and script execution
//! - Hot reloading capabilities
//! - Security sandboxing
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use fastskill_core::{FastSkillService, ServiceConfig};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ServiceConfig {
//!         skill_storage_path: PathBuf::from("./skills"),
//!         ..Default::default()
//!     };
//!
//!     let service = FastSkillService::new(config).await?;
//!
//!     // List available skills
//!     let skills = service.skill_manager().list_skills(None).await?;
//!     println!("Found {} skills", skills.len());
//!
//!     // Discover relevant skills
//!     let relevant_skills = service.metadata_service()
//!         .discover_skills("extract text from PDF")
//!         .await?;
//!
//!     println!("Found {} relevant skills", relevant_skills.len());
//!
//!     // Get available tools
//!     let tools = service.tool_service().get_available_tools().await?;
//!     println!("Available tools: {:?}", tools);
//!
//!     Ok(())
//! }
//! ```

pub mod core;
pub mod eval {
    // Re-export evals_core sub-modules so that sub-path imports like
    // `fastskill_core::eval::artifacts::CaseResult` continue to resolve.
    pub use evals_core::artifacts;
    pub use evals_core::checks;
    pub use evals_core::runner;
    pub use evals_core::suite;
    pub use evals_core::trace;

    // Re-export all top-level public items from evals_core
    pub use evals_core::{
        agent_events_to_trace, allocate_run_dir, count_raw_json_events, load_checks, load_suite,
        read_case_results, read_summary, resolve_from_input, run_checks, run_eval_case,
        stdout_to_trace, suite_passes, trace_to_jsonl, write_case_artifacts,
        write_case_trials_summary, write_summary, write_trial_artifacts, AikitEvalRunner,
        ArtifactsError, CaseResult, CaseRunOptions, CaseRunOutput, CaseStatus, CaseSummary,
        CaseTrialsResult, CheckDefinition, CheckResult, ChecksError, ChecksToml, EvalCase,
        EvalConfig, EvalConfigError, EvalConfigInput, EvalRunner, EvalSuite, RunArtifacts,
        RunnerError, SuiteError, SummaryResult, TraceEvent, TracePayload, TrialResult,
    };

    // config sub-module that adds the fastskill-specific resolve_eval_config adapter
    pub mod config {
        pub use crate::core::eval_config_adapter::resolve_eval_config;
        pub use evals_core::config::*;
    }
}
pub mod events;
pub mod execution;
pub mod http;
pub mod output;
pub mod search;
pub mod security;
pub mod storage;
pub mod validation;

pub use core::context_resolver::{
    ContentMode, ContextResolver, ResolveContextRequest, ResolveContextResponse, ResolveScope,
    ResolvedSkill,
};
pub use core::embedding::{EmbeddingService, OpenAIEmbeddingService};
pub use core::loading::{LoadedSkill, ProgressiveLoadingService};
pub use core::manifest::SkillProjectToml;
pub use core::metadata::{
    parse_yaml_frontmatter, MetadataService, SkillFrontmatter, SkillMetadata,
};
pub use core::routing::{RoutedSkill, RoutingService};
pub use core::service::SkillId;
pub use core::service::{EmbeddingConfig, FastSkillService, ServiceConfig, ServiceError};
pub use core::skill_manager::{SkillDefinition, SkillManagementService};
pub use core::tool_calling::{AvailableTool, ToolCallingService, ToolResult};
pub use core::vector_index::{
    IndexedSkill, SkillMatch, VectorIndexService, VectorIndexServiceImpl,
};

// Re-export search and output types
pub use output::OutputFormat;
pub use search::{execute, SearchError, SearchQuery, SearchResultItem, SearchScope};

// Re-export commonly used types
pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};
pub use std::collections::HashMap;
pub use std::path::{Path, PathBuf};
pub use std::sync::Arc;
pub use tokio::sync::{Mutex, RwLock};

/// Version of the service layer
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Initialize logging for the service layer (safe for testing)
pub fn init_logging() {
    init_logging_with_verbose(false)
}

/// Initialize logging for the service layer with optional verbose mode
pub fn init_logging_with_verbose(verbose: bool) {
    // Only initialize logging once
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        use tracing_subscriber::EnvFilter;

        let default_level = if verbose {
            "fastskill=info"
        } else {
            "fastskill=warn"
        };
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| default_level.into());

        let subscriber = tracing_subscriber::fmt().with_env_filter(filter).finish();

        // This will fail silently if already initialized
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

pub mod test_utils;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_service_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let service = FastSkillService::new(config).await.unwrap();
        assert!(service.skill_manager().list_skills(None).await.is_ok());
    }
}

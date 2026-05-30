//! Diagnostic and analysis commands for skill relationships and quality

pub mod cluster;
pub mod duplicates;
pub mod helpers;
pub mod matrix;
pub use cluster::ClusterArgs;
pub use duplicates::DuplicatesArgs;
pub use matrix::MatrixArgs;

use crate::error::{CliError, CliResult};
use clap::{Args, Subcommand};
use fastskill_core::core::vector_index::IndexedSkill;
use fastskill_core::FastSkillService;
use std::sync::Arc;

#[derive(Debug, Args)]
#[command(
    after_help = "Examples:\n  fastskill analyze matrix\n  fastskill analyze matrix --threshold 0.8"
)]
pub struct AnalyzeCommand {
    #[command(subcommand)]
    pub command: AnalyzeSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AnalyzeSubcommand {
    #[command(about = "Show pairwise similarity matrix for all indexed skills")]
    Matrix(MatrixArgs),
    #[command(about = "Group skills by semantic similarity")]
    Cluster(ClusterArgs),
    #[command(about = "Find semantically duplicate or very similar skills")]
    Duplicates(DuplicatesArgs),
}

pub struct AnalysisContext {
    pub skills: Vec<IndexedSkill>,
    #[allow(dead_code)]
    pub vector_svc: Arc<dyn fastskill_core::core::vector_index::VectorIndexService>,
}

pub async fn load_analysis_context(svc: &FastSkillService) -> CliResult<Option<AnalysisContext>> {
    let vector_svc = svc.vector_index_service().ok_or_else(|| {
        CliError::Config("Vector index not available. Run 'fastskill reindex' first.".to_string())
    })?;
    let skills = vector_svc
        .get_all_skills()
        .await
        .map_err(|e| CliError::Validation(format!("Failed to get indexed skills: {}", e)))?;
    if skills.is_empty() {
        println!("No skills indexed. Run 'fastskill reindex' first.");
        return Ok(None);
    }
    Ok(Some(AnalysisContext { skills, vector_svc }))
}

pub fn validate_threshold(s: &str) -> Result<f32, String> {
    let v: f32 = s
        .parse()
        .map_err(|_| format!("'{}' is not a valid number", s))?;
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("threshold must be between 0.0 and 1.0, got {}", v));
    }
    Ok(v)
}

pub async fn execute_analyze(service: &FastSkillService, command: AnalyzeCommand) -> CliResult<()> {
    let Some(ctx) = load_analysis_context(service).await? else {
        return Ok(());
    };
    match command.command {
        AnalyzeSubcommand::Matrix(args) => matrix::execute_matrix(ctx, args).await,
        AnalyzeSubcommand::Cluster(args) => cluster::execute_cluster(ctx, args).await,
        AnalyzeSubcommand::Duplicates(args) => duplicates::execute_duplicates(ctx, args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_validate_threshold_valid() {
        assert!(validate_threshold("0.0").is_ok());
        assert!(validate_threshold("1.0").is_ok());
    }
    #[test]
    fn test_validate_threshold_invalid() {
        assert!(validate_threshold("-0.1").is_err());
        assert!(validate_threshold("abc").is_err());
    }
}

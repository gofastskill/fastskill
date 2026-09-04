//! Diagnostic and analysis commands for skill relationships and quality

pub mod cluster;
pub mod duplicates;
pub mod helpers;
pub mod matrix;
pub use cluster::ClusterArgs;
pub use duplicates::DuplicatesArgs;
pub use matrix::MatrixArgs;

use crate::error::{CliError, CliResult};
use fastskill_core::core::vector_index::IndexedSkill;
use fastskill_core::FastSkillService;
use std::sync::Arc;

#[allow(dead_code)]
#[derive(Debug)]
pub struct AnalyzeCommand {
    pub command: AnalyzeSubcommand,
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum AnalyzeSubcommand {
    Matrix(MatrixArgs),
    Cluster(ClusterArgs),
    Duplicates(DuplicatesArgs),
}

pub struct AnalysisContext {
    pub skills: Vec<IndexedSkill>,
    #[allow(dead_code)]
    pub vector_svc: Arc<dyn fastskill_core::core::vector_index::VectorIndexService>,
}

/// Load the skills every `analyze` subcommand works from.
///
/// `Ok(None)` means "nothing to analyse, and the user has been told what to do
/// about it" — the empty-index case, which is a genuine (if empty) answer.
///
/// A missing Embedding provider is **not** that case: every number `analyze`
/// prints is derived from the vector index, so with no provider there is no
/// analysis to report and no non-semantic path to fall back to. This used to
/// print "Note: semantic analysis requires an embedding provider. Results may
/// be limited to structural analysis." and return `Ok(None)`, so all three
/// subcommands exited 0 having looked at nothing — indistinguishable, to a
/// human or to CI, from a clean "no duplicates found". There was no structural
/// analysis behind that promise. It is now an error (CONTEXT.md, "Vector
/// index": `analyze` *inherits the provider precondition*), and the unkept
/// promise is gone rather than restated.
pub async fn load_analysis_context(svc: &FastSkillService) -> CliResult<Option<AnalysisContext>> {
    let Some(vector_svc) = svc.vector_index_service() else {
        return Err(CliError::MissingEmbeddingProvider("analyze"));
    };
    let skills = vector_svc
        .get_all_skills()
        .await
        .map_err(|e| CliError::Validation(format!("Failed to get indexed skills: {}", e)))?;
    if skills.is_empty() {
        crate::outln!("No skills indexed. Run 'fastskill reindex' first.");
        return Ok(None);
    }
    Ok(Some(AnalysisContext { skills, vector_svc }))
}

#[cfg(test)]
fn validate_threshold(s: &str) -> Result<f32, String> {
    let v: f32 = s
        .parse()
        .map_err(|_| format!("'{}' is not a valid number", s))?;
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("threshold must be between 0.0 and 1.0, got {}", v));
    }
    Ok(v)
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

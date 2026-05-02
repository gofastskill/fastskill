//! Eval configuration resolution

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Input for eval configuration resolution.
/// Callers populate this from their own config format (TOML, YAML, etc.)
/// and pass it to [`resolve_from_input`].
#[derive(Debug, Clone)]
pub struct EvalConfigInput {
    /// Path to prompts CSV file (may be relative; resolved against project_root)
    pub prompts: PathBuf,
    /// Optional path to checks TOML file
    pub checks: Option<PathBuf>,
    /// Timeout in seconds for each case
    pub timeout_seconds: u64,
    /// Trials per case (>= 1)
    pub trials_per_case: u32,
    /// Optional maximum parallelism for trials within one case
    pub parallel: Option<u32>,
    /// Pass threshold for trial aggregation (0.0-1.0)
    pub pass_threshold: f64,
    /// Whether to fail fast if agent is not available
    pub fail_on_missing_agent: bool,
}

/// Resolved eval configuration (all paths absolute, values validated)
#[derive(Debug, Clone)]
pub struct EvalConfig {
    /// Absolute path to prompts CSV file
    pub prompts_path: PathBuf,
    /// Absolute path to checks TOML file (optional)
    pub checks_path: Option<PathBuf>,
    /// Timeout in seconds for each case
    pub timeout_seconds: u64,
    /// Trials per case (>= 1)
    pub trials_per_case: u32,
    /// Optional maximum parallelism for trials within one case
    pub parallel: Option<u32>,
    /// Pass threshold for trial aggregation (0.0-1.0)
    pub pass_threshold: f64,
    /// Whether to fail fast if agent is not available
    pub fail_on_missing_agent: bool,
    /// Skill project root directory
    pub project_root: PathBuf,
}

/// Errors during eval configuration resolution
#[derive(Debug, Error)]
pub enum EvalConfigError {
    #[error("EVAL_CONFIG_MISSING: No [tool.fastskill.eval] section found in skill-project.toml")]
    ConfigMissing,
    #[error("EVAL_PROMPTS_NOT_FOUND: Prompts CSV not found: {0}")]
    PromptsNotFound(PathBuf),
    #[error("EVAL_INVALID_TRIALS_CONFIG: trials_per_case must be in range [1, 1000], got {0}")]
    InvalidTrialsConfig(u32),
    #[error("EVAL_INVALID_THRESHOLD: pass_threshold must be in range [0.0, 1.0], got {0}")]
    InvalidPassThreshold(f64),
    #[error("Failed to read skill-project.toml: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse skill-project.toml: {0}")]
    Parse(String),
}

/// Validate and resolve an [`EvalConfigInput`] against a project root directory.
///
/// Returns [`EvalConfig`] with all paths made absolute and all values validated.
pub fn resolve_from_input(
    input: &EvalConfigInput,
    project_root: &Path,
) -> Result<EvalConfig, EvalConfigError> {
    if input.trials_per_case < 1 || input.trials_per_case > 1000 {
        return Err(EvalConfigError::InvalidTrialsConfig(input.trials_per_case));
    }
    if !(0.0..=1.0).contains(&input.pass_threshold) {
        return Err(EvalConfigError::InvalidPassThreshold(input.pass_threshold));
    }

    let prompts_path = if input.prompts.is_absolute() {
        input.prompts.clone()
    } else {
        project_root.join(&input.prompts)
    };

    if !prompts_path.exists() {
        return Err(EvalConfigError::PromptsNotFound(prompts_path));
    }

    let checks_path = input.checks.as_ref().map(|p| {
        if p.is_absolute() {
            p.clone()
        } else {
            project_root.join(p)
        }
    });

    Ok(EvalConfig {
        prompts_path,
        checks_path,
        timeout_seconds: input.timeout_seconds,
        trials_per_case: input.trials_per_case,
        parallel: input.parallel,
        pass_threshold: input.pass_threshold,
        fail_on_missing_agent: input.fail_on_missing_agent,
        project_root: project_root.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_input(prompts: PathBuf) -> EvalConfigInput {
        EvalConfigInput {
            prompts,
            checks: None,
            timeout_seconds: 600,
            trials_per_case: 1,
            parallel: None,
            pass_threshold: 1.0,
            fail_on_missing_agent: false,
        }
    }

    #[test]
    fn test_resolve_eval_config_prompts_not_found() {
        let dir = TempDir::new().unwrap();
        let input = make_input(PathBuf::from("evals/prompts.csv"));
        let result = resolve_from_input(&input, dir.path());
        assert!(matches!(result, Err(EvalConfigError::PromptsNotFound(_))));
    }

    #[test]
    fn test_resolve_eval_config_rejects_invalid_trials_per_case() {
        let dir = TempDir::new().unwrap();
        let evals_dir = dir.path().join("evals");
        std::fs::create_dir_all(&evals_dir).unwrap();
        std::fs::write(
            evals_dir.join("prompts.csv"),
            "id,prompt,should_trigger\ntest-1,hello,true\n",
        )
        .unwrap();

        let mut input = make_input(PathBuf::from("evals/prompts.csv"));
        input.trials_per_case = 0;
        let result = resolve_from_input(&input, dir.path());
        assert!(matches!(
            result,
            Err(EvalConfigError::InvalidTrialsConfig(0))
        ));
    }

    #[test]
    fn test_resolve_eval_config_rejects_invalid_pass_threshold() {
        let dir = TempDir::new().unwrap();
        let evals_dir = dir.path().join("evals");
        std::fs::create_dir_all(&evals_dir).unwrap();
        std::fs::write(
            evals_dir.join("prompts.csv"),
            "id,prompt,should_trigger\ntest-1,hello,true\n",
        )
        .unwrap();

        let mut input = make_input(PathBuf::from("evals/prompts.csv"));
        input.pass_threshold = 1.5;
        let result = resolve_from_input(&input, dir.path());
        assert!(matches!(
            result,
            Err(EvalConfigError::InvalidPassThreshold(_))
        ));
    }

    #[test]
    fn test_resolve_eval_config_success() {
        let dir = TempDir::new().unwrap();
        let evals_dir = dir.path().join("evals");
        std::fs::create_dir_all(&evals_dir).unwrap();
        std::fs::write(
            evals_dir.join("prompts.csv"),
            "id,prompt,should_trigger\ntest-1,hello,true\n",
        )
        .unwrap();

        let input = make_input(PathBuf::from("evals/prompts.csv"));
        let result = resolve_from_input(&input, dir.path());
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.timeout_seconds, 600);
        assert_eq!(config.trials_per_case, 1);
        assert_eq!(config.pass_threshold, 1.0);
        assert!(!config.fail_on_missing_agent);
    }
}

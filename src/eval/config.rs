//! Eval configuration resolution

use crate::core::manifest::{EvalConfigToml, SkillProjectToml};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Resolved eval configuration (after path resolution)
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

/// Resolve eval configuration from a skill-project.toml file
pub fn resolve_eval_config(
    project_file: &Path,
    project_root: &Path,
) -> Result<EvalConfig, EvalConfigError> {
    let content = std::fs::read_to_string(project_file)?;
    let toml: SkillProjectToml =
        toml::from_str(&content).map_err(|e| EvalConfigError::Parse(e.to_string()))?;

    let eval_config = toml
        .tool
        .as_ref()
        .and_then(|t| t.fastskill.as_ref())
        .and_then(|f| f.eval.as_ref())
        .ok_or(EvalConfigError::ConfigMissing)?;

    resolve_from_toml(eval_config, project_root)
}

/// Resolve eval configuration from parsed TOML config
pub fn resolve_from_toml(
    config: &EvalConfigToml,
    project_root: &Path,
) -> Result<EvalConfig, EvalConfigError> {
    if config.trials_per_case < 1 || config.trials_per_case > 1000 {
        return Err(EvalConfigError::InvalidTrialsConfig(config.trials_per_case));
    }
    if !(0.0..=1.0).contains(&config.pass_threshold) {
        return Err(EvalConfigError::InvalidPassThreshold(config.pass_threshold));
    }

    let prompts_path = if config.prompts.is_absolute() {
        config.prompts.clone()
    } else {
        project_root.join(&config.prompts)
    };

    if !prompts_path.exists() {
        return Err(EvalConfigError::PromptsNotFound(prompts_path));
    }

    let checks_path = config.checks.as_ref().map(|p| {
        if p.is_absolute() {
            p.clone()
        } else {
            project_root.join(p)
        }
    });

    Ok(EvalConfig {
        prompts_path,
        checks_path,
        timeout_seconds: config.timeout_seconds,
        trials_per_case: config.trials_per_case,
        parallel: config.parallel,
        pass_threshold: config.pass_threshold,
        fail_on_missing_agent: config.fail_on_missing_agent,
        project_root: project_root.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_eval_config_missing() {
        let dir = TempDir::new().unwrap();
        let project_file = dir.path().join("skill-project.toml");
        std::fs::write(&project_file, "[metadata]\nid = \"test\"\n").unwrap();

        let result = resolve_eval_config(&project_file, dir.path());
        assert!(matches!(result, Err(EvalConfigError::ConfigMissing)));
    }

    #[test]
    fn test_resolve_eval_config_prompts_not_found() {
        let dir = TempDir::new().unwrap();
        let project_file = dir.path().join("skill-project.toml");
        std::fs::write(
            &project_file,
            "[metadata]\nid = \"test\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 900\nfail_on_missing_agent = true\n",
        )
        .unwrap();

        let result = resolve_eval_config(&project_file, dir.path());
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

        let project_file = dir.path().join("skill-project.toml");
        std::fs::write(
            &project_file,
            "[metadata]\nid = \"test\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntrials_per_case = 0\ntimeout_seconds = 600\nfail_on_missing_agent = false\n",
        )
        .unwrap();

        let result = resolve_eval_config(&project_file, dir.path());
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

        let project_file = dir.path().join("skill-project.toml");
        std::fs::write(
            &project_file,
            "[metadata]\nid = \"test\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\npass_threshold = 1.5\ntimeout_seconds = 600\nfail_on_missing_agent = false\n",
        )
        .unwrap();

        let result = resolve_eval_config(&project_file, dir.path());
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
        let prompts_file = evals_dir.join("prompts.csv");
        std::fs::write(
            &prompts_file,
            "id,prompt,should_trigger\ntest-1,hello,true\n",
        )
        .unwrap();

        let project_file = dir.path().join("skill-project.toml");
        std::fs::write(
            &project_file,
            "[metadata]\nid = \"test\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 600\nfail_on_missing_agent = false\n",
        )
        .unwrap();

        let result = resolve_eval_config(&project_file, dir.path());
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.timeout_seconds, 600);
        assert_eq!(config.trials_per_case, 1);
        assert_eq!(config.pass_threshold, 1.0);
        assert!(!config.fail_on_missing_agent);
    }
}

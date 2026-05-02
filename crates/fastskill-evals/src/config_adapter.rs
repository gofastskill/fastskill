//! Adapter that reads fastskill project files and delegates to fastskill_evals config resolution

use crate::config::{resolve_from_input, EvalConfig, EvalConfigError, EvalConfigInput};
use fastskill_core::core::manifest::SkillProjectToml;
use std::path::Path;

/// Resolve eval configuration from a skill-project.toml file.
///
/// Reads the `[tool.fastskill.eval]` section, converts it to an [`EvalConfigInput`],
/// and delegates path resolution and validation to [`crate::config::resolve_from_input`].
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

    let input = EvalConfigInput {
        prompts: eval_config.prompts.clone(),
        checks: eval_config.checks.clone(),
        timeout_seconds: eval_config.timeout_seconds,
        trials_per_case: eval_config.trials_per_case,
        parallel: eval_config.parallel,
        pass_threshold: eval_config.pass_threshold,
        fail_on_missing_agent: eval_config.fail_on_missing_agent,
    };

    resolve_from_input(&input, project_root)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
    fn test_resolve_eval_config_adapter_success() {
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
            "[metadata]\nid = \"test\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 600\nfail_on_missing_agent = false\n",
        )
        .unwrap();

        let result = resolve_eval_config(&project_file, dir.path());
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.timeout_seconds, 600);
    }
}

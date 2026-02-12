//! Content safety validation (dangerous pattern checks in SKILL.md and scripts).

use crate::core::service::ServiceError;
use crate::validation::result::{ErrorSeverity, ValidationResult};
use std::path::Path;
use tokio::fs;

/// Context for dangerous pattern check (used to build error message).
pub(crate) enum PatternCheckContext<'a> {
    SkillFile,
    ScriptFile(&'a Path),
}

/// Parameters for a single dangerous-pattern check.
pub(crate) struct DangerousPatternCheck<'a> {
    pub content: &'a str,
    pub patterns: &'a [String],
    pub field: &'a str,
    pub context: PatternCheckContext<'a>,
}

pub(crate) fn add_dangerous_pattern_errors(
    mut result: ValidationResult,
    check: DangerousPatternCheck<'_>,
) -> ValidationResult {
    let message = |p: &str| match &check.context {
        PatternCheckContext::SkillFile => {
            format!("Potentially dangerous pattern found in SKILL.md: {}", p)
        }
        PatternCheckContext::ScriptFile(path) => format!(
            "Potentially dangerous pattern found in script {}: {}",
            path.display(),
            p
        ),
    };
    for pattern in check.patterns {
        if check.content.contains(pattern) {
            result = result.with_error(check.field, &message(pattern), ErrorSeverity::Critical);
        }
    }
    result
}

pub(crate) async fn validate_skill_file_content(
    path: &Path,
    result: ValidationResult,
    patterns: &[String],
) -> Result<ValidationResult, ServiceError> {
    let content = match fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(result.with_error(
                "content",
                &format!("Cannot read SKILL.md content for safety validation: {}", e),
                ErrorSeverity::Error,
            ));
        }
    };
    let result = add_dangerous_pattern_errors(
        result,
        DangerousPatternCheck {
            content: &content,
            patterns,
            field: "content",
            context: PatternCheckContext::SkillFile,
        },
    );
    let result = if content.len() > 50_000 {
        result.with_warning(
            "content",
            "SKILL.md content is very large - consider moving detailed information to reference files",
        )
    } else {
        result
    };
    Ok(result)
}

pub(crate) async fn validate_script_file_content(
    path: &Path,
    result: ValidationResult,
    patterns: &[String],
) -> Result<ValidationResult, ServiceError> {
    let content = match fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            return Ok(result.with_warning(
                "script_content",
                &format!(
                    "Cannot read script file {} for safety validation: {}",
                    path.display(),
                    e
                ),
            ));
        }
    };
    let result = add_dangerous_pattern_errors(
        result,
        DangerousPatternCheck {
            content: &content,
            patterns,
            field: "script_content",
            context: PatternCheckContext::ScriptFile(path),
        },
    );
    Ok(result)
}

pub(crate) fn default_dangerous_patterns() -> Vec<String> {
    vec![
        "import os".to_string(),
        "import subprocess".to_string(),
        "import sys".to_string(),
        "exec(".to_string(),
        "eval(".to_string(),
        "system(".to_string(),
        "popen(".to_string(),
        "rm -rf".to_string(),
        "sudo".to_string(),
        "chmod 777".to_string(),
        "chown".to_string(),
        "su ".to_string(),
        "passwd".to_string(),
    ]
}

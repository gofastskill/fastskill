//! File structure validation (SKILL.md existence/size/readable, path lists).

use crate::core::service::ServiceError;
use crate::core::skill_manager::SkillDefinition;
use crate::validation::result::{ErrorSeverity, ValidationResult};
use std::path::{Path, PathBuf};
use tokio::fs;

fn add_path_list_existence_warnings(
    mut result: ValidationResult,
    paths: Option<&[PathBuf]>,
    field_label: &str,
    missing_message: &str,
) -> ValidationResult {
    if let Some(list) = paths {
        for (i, path) in list.iter().enumerate() {
            if !path.exists() {
                result = result.with_warning(
                    &format!("{}[{}]", field_label, i),
                    &format!("{} {}", missing_message, path.display()),
                );
            }
        }
    }
    result
}

async fn check_skill_file_size(
    path: &Path,
    mut result: ValidationResult,
    max_file_size_mb: usize,
) -> Result<ValidationResult, ServiceError> {
    let metadata = fs::metadata(path).await?;
    let file_size_mb = metadata.len() / (1024 * 1024);
    if file_size_mb > max_file_size_mb as u64 {
        result = result.with_error(
            "skill_file",
            &format!(
                "SKILL.md file is too large ({} MB, max: {} MB)",
                file_size_mb, max_file_size_mb
            ),
            ErrorSeverity::Error,
        );
    }
    Ok(result)
}

async fn check_skill_file_readable(
    path: &Path,
    mut result: ValidationResult,
) -> Result<ValidationResult, ServiceError> {
    if let Err(e) = fs::read_to_string(path).await {
        result = result.with_error(
            "skill_file",
            &format!("Cannot read SKILL.md file: {}", e),
            ErrorSeverity::Critical,
        );
    }
    Ok(result)
}

pub(crate) async fn validate_file_structure(
    skill: &SkillDefinition,
    mut result: ValidationResult,
    max_file_size_mb: usize,
) -> Result<ValidationResult, ServiceError> {
    if !skill.skill_file.exists() {
        result = result.with_error(
            "skill_file",
            "SKILL.md file does not exist",
            ErrorSeverity::Critical,
        );
    } else {
        result = check_skill_file_size(&skill.skill_file, result, max_file_size_mb).await?;
        result = check_skill_file_readable(&skill.skill_file, result).await?;
    }

    result = add_path_list_existence_warnings(
        result,
        skill.reference_files.as_deref(),
        "reference_files",
        "Reference file does not exist:",
    );
    result = add_path_list_existence_warnings(
        result,
        skill.script_files.as_deref(),
        "script_files",
        "Script file does not exist:",
    );
    result = add_path_list_existence_warnings(
        result,
        skill.asset_files.as_deref(),
        "asset_files",
        "Asset file does not exist:",
    );

    Ok(result)
}

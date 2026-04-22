//! Skill directory structure validation (SKILL.md presence, scripts/references/assets).

use crate::validation::result::{ErrorSeverity, ValidationResult};
use std::path::Path;
use tokio::fs;

pub(crate) fn ensure_skill_md_exists(
    skill_path: &Path,
    mut result: ValidationResult,
) -> ValidationResult {
    let skill_file = skill_path.join("SKILL.md");
    if !skill_file.exists() {
        result = result.with_error(
            "structure",
            "Missing required SKILL.md file",
            ErrorSeverity::Critical,
        );
    }
    result
}

pub(crate) async fn scan_skill_directory_entries(
    skill_path: &Path,
    mut result: ValidationResult,
) -> Result<(bool, bool, bool, ValidationResult), crate::core::service::ServiceError> {
    let mut has_scripts = false;
    let mut has_references = false;
    let mut has_assets = false;
    let mut entries = fs::read_dir(skill_path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        match name.as_str() {
            "SKILL.md" => {}
            "scripts" => {
                if path.is_dir() {
                    has_scripts = true;
                } else {
                    result = result.with_warning("structure", "scripts should be a directory");
                }
            }
            "references" => {
                if path.is_dir() {
                    has_references = true;
                } else {
                    result = result.with_warning("structure", "references should be a directory");
                }
            }
            "assets" => {
                if path.is_dir() {
                    has_assets = true;
                } else {
                    result = result.with_warning("structure", "assets should be a directory");
                }
            }
            _ => {
                result = result
                    .with_warning("structure", &format!("Unexpected file/directory: {}", name));
            }
        }
    }
    Ok((has_scripts, has_references, has_assets, result))
}

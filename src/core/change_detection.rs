//! Change detection for skills using git and file hashing

use crate::core::build_cache::BuildCache;
use crate::core::service::ServiceError;
use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, info};

/// Detect changed skills using git diff
pub fn detect_changed_skills_git(
    base_ref: &str,
    head_ref: &str,
    skills_dir: &Path,
) -> Result<Vec<String>, ServiceError> {
    info!(
        "Detecting changed skills using git diff: {}..{}",
        base_ref, head_ref
    );

    use std::process::Command;

    // Use git diff to get changed files
    let output = Command::new("git")
        .args(["diff", "--name-only", base_ref, head_ref])
        .output()
        .map_err(|e| ServiceError::Custom(format!("Failed to execute git diff: {}", e)))?;

    if !output.status.success() {
        return Err(ServiceError::Custom(format!(
            "Git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    // Parse output and collect changed skill directories
    let mut changed_skills = HashSet::new();
    let skills_dir_str = skills_dir.to_string_lossy().to_string();
    let output_str = String::from_utf8_lossy(&output.stdout);

    for line in output_str.lines() {
        let path_str = line.trim();
        if path_str.starts_with(&skills_dir_str) {
            // Extract skill ID from path (e.g., "skills/web-scraper/file.md" -> "web-scraper")
            let relative_path = path_str
                .strip_prefix(&skills_dir_str)
                .and_then(|p| p.strip_prefix('/'))
                .or_else(|| path_str.strip_prefix(&skills_dir_str))
                .unwrap_or(path_str);

            if let Some(skill_id) = relative_path.split('/').next() {
                if !skill_id.is_empty() {
                    changed_skills.insert(skill_id.to_string());
                }
            }
        }
    }

    let skills: Vec<String> = changed_skills.into_iter().collect();
    info!("Found {} changed skills via git diff", skills.len());
    Ok(skills)
}

/// Detect changed skills using file hash comparison
pub fn detect_changed_skills_hash(
    skills_dir: &Path,
    cache: &BuildCache,
) -> Result<Vec<String>, ServiceError> {
    info!("Detecting changed skills using file hash comparison");

    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut changed_skills = Vec::new();

    // Scan for skill directories
    let entries = std::fs::read_dir(skills_dir).map_err(ServiceError::Io)?;

    for entry in entries {
        let entry = entry.map_err(ServiceError::Io)?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(skill_id) = path.file_name().and_then(|n| n.to_str()) {
                // Check if SKILL.md exists (indicates this is a skill directory)
                if path.join("SKILL.md").exists() {
                    let current_hash = calculate_skill_hash(&path)?;

                    // Check if hash changed
                    let cached_hash = cache.get_cached_hash(skill_id);

                    if cached_hash.as_ref().map(|h| h != &current_hash).unwrap_or(true) {
                        debug!(
                            "Skill '{}' changed (hash: {} -> {})",
                            skill_id,
                            cached_hash.as_deref().unwrap_or("none"),
                            &current_hash
                        );
                        changed_skills.push(skill_id.to_string());
                    }
                }
            }
        }
    }

    info!(
        "Found {} changed skills via hash comparison",
        changed_skills.len()
    );
    Ok(changed_skills)
}

/// Calculate SHA256 hash of all files in a skill directory
pub fn calculate_skill_hash(skill_path: &Path) -> Result<String, ServiceError> {
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;

    let mut hasher = Sha256::new();

    // Walk through all files in the skill directory
    let entries = walkdir::WalkDir::new(skill_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    let mut file_paths: Vec<PathBuf> = entries.map(|e| e.path().to_path_buf()).collect();

    // Sort to ensure consistent hashing
    file_paths.sort();

    for file_path in file_paths {
        // Read file content
        let content = fs::read(&file_path).map_err(ServiceError::Io)?;

        // Include relative path in hash
        let relative_path =
            file_path.strip_prefix(skill_path).unwrap_or(&file_path).to_string_lossy();

        hasher.update(relative_path.as_bytes());
        hasher.update(&content);
    }

    let hash = format!("sha256:{:x}", hasher.finalize());
    Ok(hash)
}

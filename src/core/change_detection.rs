//! Change detection for skills using git and file hashing

use crate::core::build_cache::BuildCache;
use crate::core::service::ServiceError;
use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, info};

#[cfg(feature = "git-support")]
use git2::Repository;

/// Detect changed skills using git diff
#[cfg(feature = "git-support")]
pub fn detect_changed_skills_git(
    base_ref: &str,
    head_ref: &str,
    skills_dir: &Path,
) -> Result<Vec<String>, ServiceError> {
    info!(
        "Detecting changed skills using git diff: {}..{}",
        base_ref, head_ref
    );

    // Open the repository
    let repo = Repository::open(".")
        .map_err(|e| ServiceError::Custom(format!("Failed to open git repository: {}", e)))?;

    // Resolve base and head references
    let base_oid = repo
        .revparse_single(base_ref)
        .map_err(|e| {
            ServiceError::Custom(format!("Failed to resolve base ref '{}': {}", base_ref, e))
        })?
        .id();

    let head_oid = repo
        .revparse_single(head_ref)
        .map_err(|e| {
            ServiceError::Custom(format!("Failed to resolve head ref '{}': {}", head_ref, e))
        })?
        .id();

    // Get the diff between base and head
    let base_tree = repo
        .find_tree(base_oid)
        .map_err(|e| ServiceError::Custom(format!("Failed to find base tree: {}", e)))?;

    let head_tree = repo
        .find_tree(head_oid)
        .map_err(|e| ServiceError::Custom(format!("Failed to find head tree: {}", e)))?;

    let diff = repo
        .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)
        .map_err(|e| ServiceError::Custom(format!("Failed to create diff: {}", e)))?;

    // Collect changed skill directories
    let mut changed_skills = HashSet::new();
    let skills_dir_str = skills_dir.to_string_lossy().to_string();

    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                let path_str = path.to_string_lossy();

                // Check if this file is in a skill directory
                if path_str.starts_with(&skills_dir_str) {
                    // Extract skill ID from path (e.g., "skills/web-scraper/file.md" -> "web-scraper")
                    let relative_path = path_str
                        .strip_prefix(&skills_dir_str)
                        .and_then(|p| p.strip_prefix('/'))
                        .or_else(|| path_str.strip_prefix(&skills_dir_str))
                        .unwrap_or(&path_str);

                    if let Some(skill_id) = relative_path.split('/').next() {
                        if !skill_id.is_empty() {
                            changed_skills.insert(skill_id.to_string());
                        }
                    }
                }
            }
            true
        },
        None,
        None,
        None,
    )
    .map_err(|e| ServiceError::Custom(format!("Failed to process diff: {}", e)))?;

    let skills: Vec<String> = changed_skills.into_iter().collect();
    info!("Found {} changed skills via git diff", skills.len());
    Ok(skills)
}

/// Fallback when git-support is not available
#[cfg(not(feature = "git-support"))]
pub fn detect_changed_skills_git(
    _base_ref: &str,
    _head_ref: &str,
    _skills_dir: &Path,
) -> Result<Vec<String>, ServiceError> {
    Err(ServiceError::Custom(
        "Git support is not enabled. Compile with --features git-support".to_string(),
    ))
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

                    if cached_hash
                        .as_ref()
                        .map(|h| h != &current_hash)
                        .unwrap_or(true)
                    {
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
        let relative_path = file_path
            .strip_prefix(skill_path)
            .unwrap_or(&file_path)
            .to_string_lossy();

        hasher.update(relative_path.as_bytes());
        hasher.update(&content);
    }

    let hash = format!("sha256:{:x}", hasher.finalize());
    Ok(hash)
}

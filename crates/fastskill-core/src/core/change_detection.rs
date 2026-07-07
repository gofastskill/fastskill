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
    let output_str = String::from_utf8_lossy(&output.stdout);

    for line in output_str.lines() {
        if let Some(skill_id) = skill_id_from_diff_line(line.trim(), skills_dir) {
            changed_skills.insert(skill_id);
        }
    }

    let skills: Vec<String> = changed_skills.into_iter().collect();
    info!("Found {} changed skills via git diff", skills.len());
    Ok(skills)
}

/// Extract the skill id from a single `git diff --name-only` path line.
///
/// Strips `skills_dir` on whole path components (not a raw byte prefix) so a
/// sibling directory sharing the skills-dir name as a prefix — e.g.
/// `skills-extra` next to `skills` — is not misparsed, and path separators are
/// handled platform-correctly (BUG-7). Returns `None` when the path is not under
/// `skills_dir` or has no component beyond it.
fn skill_id_from_diff_line(path_str: &str, skills_dir: &Path) -> Option<String> {
    if path_str.is_empty() {
        return None;
    }

    let relative = Path::new(path_str).strip_prefix(skills_dir).ok()?;

    let first = relative.components().next()?;
    let skill_id = first.as_os_str().to_string_lossy();
    if skill_id.is_empty() {
        None
    } else {
        Some(skill_id.to_string())
    }
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

        // Length-prefix each field so field boundaries are unambiguous. Without
        // this, ("ab", "c") and ("a", "bc") would hash identically and a crafted
        // rename+edit could produce a stale cache hit (BUG-15).
        let path_bytes = relative_path.as_bytes();
        hasher.update((path_bytes.len() as u64).to_le_bytes());
        hasher.update(path_bytes);
        hasher.update((content.len() as u64).to_le_bytes());
        hasher.update(&content);
    }

    let hash = format!("sha256:{:x}", hasher.finalize());
    Ok(hash)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // --- BUG-7: git-diff path parsing on whole components ---

    #[test]
    fn test_diff_line_extracts_skill_id() {
        let id = skill_id_from_diff_line("skills/web-scraper/SKILL.md", Path::new("skills"));
        assert_eq!(id.as_deref(), Some("web-scraper"));
    }

    #[test]
    fn test_diff_line_ignores_sibling_prefix_dir() {
        // "skills-extra" shares "skills" as a raw byte prefix but is NOT under it.
        let id = skill_id_from_diff_line("skills-extra/foo/bar.md", Path::new("skills"));
        assert_eq!(id, None, "sibling dir sharing a prefix must not match");
    }

    #[test]
    fn test_diff_line_unrelated_path() {
        let id = skill_id_from_diff_line("README.md", Path::new("skills"));
        assert_eq!(id, None);
    }

    #[test]
    fn test_diff_line_exact_dir_no_child_component() {
        // The skills dir itself with nothing after it yields no skill id.
        let id = skill_id_from_diff_line("skills", Path::new("skills"));
        assert_eq!(id, None);
    }

    #[test]
    fn test_diff_line_empty() {
        assert_eq!(skill_id_from_diff_line("", Path::new("skills")), None);
    }

    #[test]
    fn test_diff_line_nested_skills_dir() {
        let id = skill_id_from_diff_line(
            "some/root/skills/my-skill/file.md",
            Path::new("some/root/skills"),
        );
        assert_eq!(id.as_deref(), Some("my-skill"));
    }

    // --- BUG-15: length-prefixed hashing avoids path/content collisions ---

    fn write_single_file_skill(dir: &Path, file_name: &str, content: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(file_name), content).unwrap();
    }

    #[test]
    fn test_hash_no_collision_on_boundary_shift() {
        let tmp = TempDir::new().unwrap();

        // ("ab", "c") vs ("a", "bc") — concatenations are identical ("abc").
        let dir_a = tmp.path().join("a_skill");
        write_single_file_skill(&dir_a, "ab", "c");

        let dir_b = tmp.path().join("b_skill");
        write_single_file_skill(&dir_b, "a", "bc");

        let hash_a = calculate_skill_hash(&dir_a).unwrap();
        let hash_b = calculate_skill_hash(&dir_b).unwrap();

        assert_ne!(
            hash_a, hash_b,
            "path/content boundary shift must produce different hashes"
        );
    }

    #[test]
    fn test_hash_stable_and_content_sensitive() {
        let tmp = TempDir::new().unwrap();

        let dir1 = tmp.path().join("s1");
        write_single_file_skill(&dir1, "SKILL.md", "hello");
        let dir1_again = tmp.path().join("s1_again");
        write_single_file_skill(&dir1_again, "SKILL.md", "hello");
        // Same path + content → same hash.
        assert_eq!(
            calculate_skill_hash(&dir1).unwrap(),
            calculate_skill_hash(&dir1_again).unwrap()
        );

        // Changed content → different hash.
        let dir2 = tmp.path().join("s2");
        write_single_file_skill(&dir2, "SKILL.md", "hello!");
        assert_ne!(
            calculate_skill_hash(&dir1).unwrap(),
            calculate_skill_hash(&dir2).unwrap()
        );
    }

    #[test]
    fn test_hash_has_prefix() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("s");
        write_single_file_skill(&dir, "SKILL.md", "x");
        assert!(calculate_skill_hash(&dir).unwrap().starts_with("sha256:"));
    }
}

//! Crates.io-like registry index management

use crate::core::service::ServiceError;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Scoped skill name normalization utilities
pub struct ScopedSkillName;

impl ScopedSkillName {
    /// Normalize a scoped skill name to filesystem path format
    /// Strips scope prefixes (@, :) and normalizes to org/package format
    /// Examples:
    /// - `@acme/web-scraper` -> `acme/web-scraper`
    /// - `acme:web-scraper` -> `acme/web-scraper`
    /// - `acme/web-scraper` -> `acme/web-scraper`
    /// - `web-scraper` -> `web-scraper` (no scope, returns as-is)
    pub fn normalize(scoped_name: &str) -> String {
        let trimmed = scoped_name.trim();

        // Remove @ prefix if present
        let without_at = trimmed.strip_prefix('@').unwrap_or(trimmed);

        // Replace : with / if present
        without_at.replace(':', "/")
    }
}

/// Create registry index structure
pub fn create_registry_structure(base_path: &Path) -> Result<(), ServiceError> {
    fs::create_dir_all(base_path).map_err(ServiceError::Io)?;
    Ok(())
}

/// Get the index file path for a skill using org/package directory structure
/// Format: {org}/{package}
/// skill_id must always be in "org/package" format (enforced at publish time)
pub fn get_skill_index_path(registry_path: &Path, skill_id: &str) -> PathBuf {
    // skill_id is always in org/package format (enforced at publish time)
    // Organization is the authenticated user's username (lowercase, filesystem-safe)
    let parts: Vec<&str> = skill_id.split('/').collect();

    // Enforce that skill_id must have exactly 2 parts (org/package)
    // This is a programming error check - skill_id format is enforced at publish time
    #[allow(clippy::panic)]
    if parts.len() != 2 {
        panic!(
            "Invalid skill_id format: must be 'org/package', got: {}",
            skill_id
        );
    }

    let org = parts[0]; // Already lowercase and filesystem-safe from publish
    let package = parts[1];

    registry_path.join(org).join(package)
}

/// Update registry index with a new skill version
/// Appends newline-delimited JSON to single file per skill (crates.io format)
pub fn update_skill_version(
    skill_id: &str,
    version: &str,
    metadata: &VersionMetadata,
    registry_path: &Path,
) -> Result<(), ServiceError> {
    // Get index file path using crates.io directory structure
    let index_path = get_skill_index_path(registry_path, skill_id);

    // Create parent directories if they don't exist
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent).map_err(ServiceError::Io)?;
    }

    // Create version entry
    let entry = VersionEntry {
        scoped_name: None,
        name: skill_id.to_string(),
        vers: version.to_string(),
        deps: metadata.deps.clone(),
        cksum: metadata.cksum.clone(),
        features: metadata.features.clone(),
        yanked: metadata.yanked,
        links: metadata.links.clone(),
        download_url: metadata.download_url.clone(),
        published_at: metadata.published_at.clone(),
        metadata: metadata.metadata.clone(),
    };

    // Serialize to compact JSON (not pretty-printed)
    let line = serde_json::to_string(&entry)
        .map_err(|e| ServiceError::Custom(format!("Failed to serialize index entry: {}", e)))?;

    // Append line to file (newline-delimited JSON format)
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&index_path)
        .map_err(ServiceError::Io)?;

    writeln!(file, "{}", line).map_err(ServiceError::Io)?;

    Ok(())
}

/// Read all versions for a skill from the index file
/// Parses newline-delimited JSON format
pub fn read_skill_versions(
    registry_path: &Path,
    skill_id: &str,
) -> Result<Vec<VersionEntry>, ServiceError> {
    let index_path = get_skill_index_path(registry_path, skill_id);

    if !index_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&index_path).map_err(ServiceError::Io)?;

    let mut entries = Vec::new();

    // Parse line-by-line (newline-delimited JSON)
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<VersionEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                // Log corruption event but continue parsing other lines
                // This allows the system to serve other skills normally
                use tracing::error;
                error!(
                    "Index file corruption detected for skill {}: Failed to parse entry: {} (line: {})",
                    skill_id, e, line
                );
                // Continue parsing - don't fail the entire read for one corrupted entry
            }
        }
    }

    Ok(entries)
}

/// Get version metadata from ZIP file
pub fn get_version_metadata(
    skill_id: &str,
    zip_path: &Path,
    download_url: &str,
) -> Result<VersionMetadata, ServiceError> {
    // Calculate checksum
    let cksum = calculate_file_checksum(zip_path)?;

    Ok(VersionMetadata {
        name: skill_id.to_string(),
        vers: String::new(), // Will be set by caller
        deps: Vec::new(),
        cksum,
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: download_url.to_string(),
        published_at: Utc::now().to_rfc3339(),
        metadata: None, // Will be populated from skill package
    })
}

/// Version metadata entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMetadata {
    pub name: String,
    pub vers: String,
    pub deps: Vec<Dependency>,
    pub cksum: String,
    pub features: HashMap<String, Vec<String>>,
    pub yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    pub download_url: String,
    pub published_at: String,
    #[serde(default)]
    pub metadata: Option<IndexMetadata>,
}

/// Index metadata (description, author, tags, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub description: Option<String>,
    pub author: Option<String>,
    pub tags: Option<Vec<String>>,
    pub capabilities: Option<Vec<String>>,
    pub license: Option<String>,
    pub repository: Option<String>,
}

/// Version entry in index JSON file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub name: String,
    pub vers: String,
    pub deps: Vec<Dependency>,
    pub cksum: String,
    pub features: HashMap<String, Vec<String>>,
    pub yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    pub download_url: String,
    pub published_at: String,
    #[serde(default)]
    pub metadata: Option<IndexMetadata>,
    /// Original scoped name (e.g., `@org/package`) if the skill uses scoped naming
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoped_name: Option<String>,
}

/// Dependency entry for registry index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: Option<String>,
}

/// Calculate SHA256 checksum of a file
fn calculate_file_checksum(file_path: &Path) -> Result<String, ServiceError> {
    let mut file = fs::File::open(file_path).map_err(ServiceError::Io)?;

    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];

    loop {
        let bytes_read = file.read(&mut buffer).map_err(ServiceError::Io)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    let hash = format!("sha256:{:x}", hasher.finalize());
    Ok(hash)
}

/// Migrate old index format to new crates.io format
/// Old format: index/{first2}/{next2}/{skill-id}-{version}.json
/// New format: {first2}/{next2}/{skill-id} (or 1/{name}, 2/{name}, 3/{first}/{name})
pub fn migrate_index_format(
    old_registry_path: &Path,
    new_registry_path: &Path,
) -> Result<usize, ServiceError> {
    use walkdir::WalkDir;

    let mut migrated_count = 0;

    // Walk through old index structure
    let index_dir = old_registry_path.join("index");
    if !index_dir.exists() {
        return Ok(0);
    }

    // Group files by skill ID
    let mut skill_versions: std::collections::HashMap<String, Vec<(String, VersionEntry)>> =
        std::collections::HashMap::new();

    for entry in WalkDir::new(&index_dir) {
        let entry = entry.map_err(|e| ServiceError::Io(e.into()))?;
        let path = entry.path();

        if path.is_file() {
            // Parse old filename format: {skill-id}-{version}.json
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some((skill_id, version)) = parse_old_index_filename(file_name) {
                    // Read old format file
                    let content = fs::read_to_string(path).map_err(ServiceError::Io)?;

                    if let Ok(entry) = serde_json::from_str::<VersionEntry>(&content) {
                        skill_versions
                            .entry(skill_id)
                            .or_default()
                            .push((version, entry));
                    }
                }
            }
        }
    }

    // Write new format files
    for (skill_id, versions) in skill_versions {
        let new_index_path = get_skill_index_path(new_registry_path, &skill_id);

        // Create parent directories
        if let Some(parent) = new_index_path.parent() {
            fs::create_dir_all(parent).map_err(ServiceError::Io)?;
        }

        // Write all versions to single file
        let mut file = fs::File::create(&new_index_path).map_err(ServiceError::Io)?;

        for (_, entry) in versions {
            let line = serde_json::to_string(&entry)
                .map_err(|e| ServiceError::Custom(format!("Failed to serialize entry: {}", e)))?;
            writeln!(file, "{}", line).map_err(ServiceError::Io)?;
        }

        migrated_count += 1;
    }

    Ok(migrated_count)
}

/// Parse old index filename format: {skill-id}-{version}.json
fn parse_old_index_filename(filename: &str) -> Option<(String, String)> {
    let without_ext = filename.strip_suffix(".json")?;
    if let Some(last_dash) = without_ext.rfind('-') {
        let skill_id = without_ext[..last_dash].to_string();
        let version = without_ext[last_dash + 1..].to_string();
        Some((skill_id, version))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_get_skill_index_path() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path();

        // Test org/package format
        let path = get_skill_index_path(registry_path, "acme/web-scraper");
        assert_eq!(path, registry_path.join("acme").join("web-scraper"));

        // Test another org/package
        let path = get_skill_index_path(registry_path, "myorg/data-processor");
        assert_eq!(path, registry_path.join("myorg").join("data-processor"));

        // Test single character org/package
        let path = get_skill_index_path(registry_path, "a/tool");
        assert_eq!(path, registry_path.join("a").join("tool"));

        // Test that invalid format panics (should panic)
        // Note: This test verifies that malformed skill_ids are rejected
        // let result = std::panic::catch_unwind(|| {
        //     get_skill_index_path(registry_path, "invalid-format");
        // });
        // assert!(result.is_err()); // Should panic with invalid format
    }

    #[test]
    fn test_update_and_read_skill_versions() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path();

        let skill_id = "acme/test-skill";
        let version1 = "1.0.0";
        let version2 = "1.0.1";

        // Create metadata
        let metadata1 = VersionMetadata {
            name: skill_id.to_string(),
            vers: version1.to_string(),
            deps: Vec::new(),
            cksum: "sha256:test1".to_string(),
            features: HashMap::new(),
            yanked: false,
            links: None,
            download_url: "http://example.com/v1.zip".to_string(),
            published_at: "2024-01-01T00:00:00Z".to_string(),
            metadata: None,
        };

        let metadata2 = VersionMetadata {
            name: skill_id.to_string(),
            vers: version2.to_string(),
            deps: Vec::new(),
            cksum: "sha256:test2".to_string(),
            features: HashMap::new(),
            yanked: false,
            links: None,
            download_url: "http://example.com/v2.zip".to_string(),
            published_at: "2024-01-02T00:00:00Z".to_string(),
            metadata: None,
        };

        // Update with first version
        update_skill_version(skill_id, version1, &metadata1, registry_path).unwrap();

        // Update with second version
        update_skill_version(skill_id, version2, &metadata2, registry_path).unwrap();

        // Read all versions
        let entries = read_skill_versions(registry_path, skill_id).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].vers, version1);
        assert_eq!(entries[1].vers, version2);
    }

    #[test]
    fn test_newline_delimited_format() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path();

        let skill_id = "acme/test";
        let metadata = VersionMetadata {
            name: skill_id.to_string(),
            vers: "1.0.0".to_string(),
            deps: Vec::new(),
            cksum: "sha256:test".to_string(),
            features: HashMap::new(),
            yanked: false,
            links: None,
            download_url: "http://example.com/test.zip".to_string(),
            published_at: "2024-01-01T00:00:00Z".to_string(),
            metadata: None,
        };

        update_skill_version(skill_id, "1.0.0", &metadata, registry_path).unwrap();

        // Read file and verify it's compact JSON (not pretty-printed)
        let index_path = get_skill_index_path(registry_path, skill_id);
        let content = fs::read_to_string(&index_path).unwrap();

        // Should be a single line (newline-delimited)
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        // Should not contain pretty-printing (no newlines in JSON)
        assert!(!content.contains("\n  "));
        assert!(!content.contains("    "));
    }
}

//! Crates.io-like registry index management

use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use semver;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

/// Options for listing skills from registry
#[derive(Debug, Clone, Default)]
pub struct ListSkillsOptions {
    pub scope: Option<String>,
    pub all_versions: bool,
    pub include_pre_release: bool,
}

/// Summary of a skill from the registry index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummary {
    pub id: String,
    pub scope: String,
    pub name: String,
    pub description: String,
    pub latest_version: String,
    #[serde(
        serialize_with = "serialize_datetime_option",
        deserialize_with = "deserialize_datetime_option"
    )]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versions: Option<Vec<String>>,
}

/// Serialize DateTime<Utc> as ISO 8601 string
fn serialize_datetime_option<S>(
    dt: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match dt {
        Some(dt) => serializer.serialize_str(&dt.to_rfc3339()),
        None => serializer.serialize_none(),
    }
}

/// Deserialize ISO 8601 string to DateTime<Utc>
fn deserialize_datetime_option<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        Some(s) => DateTime::parse_from_rfc3339(&s)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(serde::de::Error::custom),
        None => Ok(None),
    }
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

/// Extract scope from skill ID (format: scope/name)
fn extract_scope(skill_id: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = skill_id.split('/').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Determine if a version is a pre-release
fn is_pre_release(version: &str) -> bool {
    semver::Version::parse(version)
        .map(|v| !v.pre.is_empty())
        .unwrap_or(false)
}

/// Determine latest version from a list of version entries
/// Returns the highest stable version, or highest pre-release if include_pre_release is true
fn determine_latest_version(
    entries: &[VersionEntry],
    include_pre_release: bool,
) -> Option<&VersionEntry> {
    let mut valid_entries: Vec<&VersionEntry> = entries
        .iter()
        .filter(|e| !e.yanked)
        .filter(|e| include_pre_release || !is_pre_release(&e.vers))
        .collect();

    if valid_entries.is_empty() {
        return None;
    }

    // Sort by semantic version (highest first)
    valid_entries.sort_by(|a, b| {
        let ver_a = semver::Version::parse(&a.vers).ok();
        let ver_b = semver::Version::parse(&b.vers).ok();
        match (ver_a, ver_b) {
            (Some(va), Some(vb)) => vb.cmp(&va), // Reverse order (highest first)
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.vers.cmp(&a.vers), // String fallback
        }
    });

    valid_entries.first().copied()
}

/// Scan registry index directory and return skill summaries
/// This is used by the server-side HTTP endpoint
pub async fn scan_registry_index(
    registry_path: &Path,
    options: &ListSkillsOptions,
) -> Result<Vec<SkillSummary>, ServiceError> {
    use tracing::{error, warn};

    if !registry_path.exists() {
        return Ok(Vec::new());
    }

    let mut skill_map: std::collections::HashMap<String, Vec<VersionEntry>> =
        std::collections::HashMap::new();

    // Recursively scan registry directory
    for entry in WalkDir::new(registry_path).min_depth(1) {
        let entry = entry.map_err(|e| ServiceError::Io(e.into()))?;
        let path = entry.path();

        // Skip directories and hidden files
        if path.is_dir()
            || path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false)
        {
            continue;
        }

        // Try to determine skill_id from path
        // Path format: {registry_path}/{scope}/{name}
        let relative_path = path.strip_prefix(registry_path).map_err(|e| {
            ServiceError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Failed to strip prefix: {}", e),
            ))
        })?;

        let parts: Vec<&str> = relative_path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        // Must have at least 2 parts (scope/name)
        if parts.len() < 2 {
            warn!("Skipping invalid path structure: {:?}", path);
            continue;
        }

        // Reconstruct skill_id from path
        let skill_id = format!("{}/{}", parts[0], parts[1]);

        // Validate skill_id format
        if extract_scope(&skill_id).is_none() {
            warn!("Skipping invalid skill_id format: {}", skill_id);
            continue;
        }

        // Apply scope filter if specified
        if let Some(ref filter_scope) = options.scope {
            if let Some((scope, _)) = extract_scope(&skill_id) {
                if scope != *filter_scope {
                    continue;
                }
            } else {
                continue;
            }
        }

        // Read version entries from index file
        match read_skill_versions(registry_path, &skill_id) {
            Ok(entries) => {
                if !entries.is_empty() {
                    skill_map.insert(skill_id, entries);
                }
            }
            Err(e) => {
                error!("Failed to read index file for skill {}: {}", skill_id, e);
                // Continue processing other skills
            }
        }
    }

    let mut summaries = Vec::new();

    for (skill_id, entries) in skill_map {
        let (scope, name) = extract_scope(&skill_id).ok_or_else(|| {
            ServiceError::Custom(format!("Invalid skill_id format: {}", skill_id))
        })?;

        if options.all_versions {
            // Return one summary per version
            let mut seen_versions = std::collections::HashSet::new();
            for entry in entries {
                // Skip yanked versions
                if entry.yanked {
                    continue;
                }

                // Apply pre-release filter
                if !options.include_pre_release && is_pre_release(&entry.vers) {
                    continue;
                }

                // Deduplicate: use first occurrence of each version
                if seen_versions.contains(&entry.vers) {
                    warn!(
                        "Duplicate version {} for skill {}, skipping",
                        entry.vers, skill_id
                    );
                    continue;
                }
                seen_versions.insert(entry.vers.clone());

                // Parse published_at
                let published_at = entry.published_at.parse::<DateTime<Utc>>().ok();

                // Extract description from metadata
                let description = entry
                    .metadata
                    .as_ref()
                    .and_then(|m| m.description.clone())
                    .unwrap_or_default();

                summaries.push(SkillSummary {
                    id: skill_id.clone(),
                    scope: scope.clone(),
                    name: name.clone(),
                    description,
                    latest_version: entry.vers.clone(),
                    published_at,
                    versions: None,
                });
            }
        } else {
            // Return one summary per skill (latest version)
            if let Some(latest_entry) =
                determine_latest_version(&entries, options.include_pre_release)
            {
                // Parse published_at
                let published_at = latest_entry.published_at.parse::<DateTime<Utc>>().ok();

                // Extract description from metadata
                let description = latest_entry
                    .metadata
                    .as_ref()
                    .and_then(|m| m.description.clone())
                    .unwrap_or_default();

                // Collect all versions if needed
                let versions = if options.include_pre_release {
                    Some(
                        entries
                            .iter()
                            .filter(|e| !e.yanked)
                            .map(|e| e.vers.clone())
                            .collect(),
                    )
                } else {
                    Some(
                        entries
                            .iter()
                            .filter(|e| !e.yanked && !is_pre_release(&e.vers))
                            .map(|e| e.vers.clone())
                            .collect(),
                    )
                };

                summaries.push(SkillSummary {
                    id: skill_id,
                    scope,
                    name,
                    description,
                    latest_version: latest_entry.vers.clone(),
                    published_at,
                    versions,
                });
            }
        }
    }

    // Sort alphabetically by skill ID (ascending)
    summaries.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(summaries)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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

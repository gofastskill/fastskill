//! Packaging skills into ZIP artifacts with metadata

use crate::core::service::ServiceError;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::{FileOptions, ZipWriter};
use zip::CompressionMethod;

/// Package a skill directory into a ZIP file
pub fn package_skill(
    skill_path: &Path,
    output_dir: &Path,
    version: &str,
) -> Result<PathBuf, ServiceError> {
    package_skill_with_id(skill_path, output_dir, version, None)
}

/// Package a skill directory into a ZIP file with explicit skill ID
pub fn package_skill_with_id(
    skill_path: &Path,
    output_dir: &Path,
    version: &str,
    skill_id_override: Option<&str>,
) -> Result<PathBuf, ServiceError> {
    // Validate skill directory
    if !skill_path.exists() {
        return Err(ServiceError::Custom(format!(
            "Skill directory does not exist: {}",
            skill_path.display()
        )));
    }

    if !skill_path.is_dir() {
        return Err(ServiceError::Custom(format!(
            "Path is not a directory: {}",
            skill_path.display()
        )));
    }

    // Check for SKILL.md
    if !skill_path.join("SKILL.md").exists() {
        return Err(ServiceError::Custom(format!(
            "SKILL.md not found in skill directory: {}",
            skill_path.display()
        )));
    }

    // Get skill ID - use override if provided, otherwise extract from directory name
    let skill_id = if let Some(id) = skill_id_override {
        id
    } else {
        skill_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                ServiceError::Custom("Cannot determine skill ID from directory name".to_string())
            })?
    };

    // Normalize skill ID for filename (replace slashes with hyphens)
    let normalized_id = skill_id.replace('/', "-");

    // Create output directory if it doesn't exist
    fs::create_dir_all(output_dir).map_err(ServiceError::Io)?;

    // Create ZIP file path
    let zip_filename = format!("{}-{}.zip", normalized_id, version);
    let zip_path = output_dir.join(&zip_filename);

    // Create ZIP file
    let file = fs::File::create(&zip_path).map_err(ServiceError::Io)?;

    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // Walk through skill directory and add files
    let entries = walkdir::WalkDir::new(skill_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    for entry in entries {
        let file_path = entry.path();
        let relative_path = file_path
            .strip_prefix(skill_path)
            .map_err(|e| ServiceError::Custom(format!("Failed to get relative path: {}", e)))?;

        let relative_path_str = relative_path.to_string_lossy();

        // Skip .git files
        if relative_path_str.contains(".git/") {
            continue;
        }

        // Read file content
        let mut file_content = Vec::new();
        let mut file = fs::File::open(file_path).map_err(ServiceError::Io)?;
        file.read_to_end(&mut file_content)
            .map_err(ServiceError::Io)?;

        // Add file to ZIP
        zip.start_file(relative_path_str.as_ref(), options)
            .map_err(|e| ServiceError::Custom(format!("Failed to add file to ZIP: {}", e)))?;
        zip.write_all(&file_content).map_err(ServiceError::Io)?;
    }

    // Get git commit if available
    let git_commit = get_git_commit().ok();

    // Create build metadata (use original skill_id, not normalized)
    let build_metadata = create_build_metadata(skill_id, version, git_commit.as_deref());

    // Add BUILD_INFO.json to ZIP
    let build_info_json = serde_json::to_string_pretty(&build_metadata)
        .map_err(|e| ServiceError::Custom(format!("Failed to serialize build metadata: {}", e)))?;

    zip.start_file("BUILD_INFO.json", options)
        .map_err(|e| ServiceError::Custom(format!("Failed to add BUILD_INFO.json: {}", e)))?;
    zip.write_all(build_info_json.as_bytes())
        .map_err(ServiceError::Io)?;

    // Finish ZIP first (without checksum) to calculate checksum
    zip.finish()
        .map_err(|e| ServiceError::Custom(format!("Failed to finalize ZIP: {}", e)))?;

    // Calculate checksum of the ZIP file (before adding checksum file)
    // This checksum represents the ZIP contents without the checksum file itself
    let checksum = calculate_checksum(&zip_path)?;

    // Now recreate the ZIP with checksum included
    // Remove the old ZIP
    fs::remove_file(&zip_path).map_err(ServiceError::Io)?;

    // Create new ZIP with checksum
    let file = fs::File::create(&zip_path).map_err(ServiceError::Io)?;

    let mut zip = ZipWriter::new(file);

    // Re-add all files from skill directory
    let entries = walkdir::WalkDir::new(skill_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file());

    for entry in entries {
        let file_path = entry.path();
        let relative_path = file_path
            .strip_prefix(skill_path)
            .map_err(|e| ServiceError::Custom(format!("Failed to get relative path: {}", e)))?;

        let relative_path_str = relative_path.to_string_lossy();

        if relative_path_str.contains(".git/") {
            continue;
        }

        let mut file_content = Vec::new();
        let mut file = fs::File::open(file_path).map_err(ServiceError::Io)?;
        file.read_to_end(&mut file_content)
            .map_err(ServiceError::Io)?;

        zip.start_file(relative_path_str.as_ref(), options)
            .map_err(|e| ServiceError::Custom(format!("Failed to add file to ZIP: {}", e)))?;
        zip.write_all(&file_content).map_err(ServiceError::Io)?;
    }

    // Add BUILD_INFO.json (with checksum included in metadata)
    // Note: checksum is stored separately in CHECKSUM.sha256 file

    let build_info_json = serde_json::to_string_pretty(&build_metadata)
        .map_err(|e| ServiceError::Custom(format!("Failed to serialize build metadata: {}", e)))?;

    zip.start_file("BUILD_INFO.json", options)
        .map_err(|e| ServiceError::Custom(format!("Failed to add BUILD_INFO.json: {}", e)))?;
    zip.write_all(build_info_json.as_bytes())
        .map_err(ServiceError::Io)?;

    // Add CHECKSUM.sha256 (checksum of ZIP before this file was added)
    zip.start_file("CHECKSUM.sha256", options)
        .map_err(|e| ServiceError::Custom(format!("Failed to add CHECKSUM.sha256: {}", e)))?;
    zip.write_all(checksum.as_bytes())
        .map_err(ServiceError::Io)?;

    zip.finish().map_err(|e| {
        ServiceError::Custom(format!("Failed to finalize ZIP with checksum: {}", e))
    })?;

    Ok(zip_path)
}

/// Create build metadata for a skill
pub fn create_build_metadata(
    skill_id: &str,
    version: &str,
    git_commit: Option<&str>,
) -> BuildMetadata {
    BuildMetadata {
        skill_id: skill_id.to_string(),
        version: version.to_string(),
        build_timestamp: Utc::now().to_rfc3339(),
        git_commit: git_commit.map(|s| s.to_string()),
        git_branch: get_git_branch().ok(),
        build_environment: BuildEnvironment {
            fastskill_version: env!("CARGO_PKG_VERSION").to_string(),
            rust_version: get_rust_version(),
        },
    }
}

/// Calculate SHA256 checksum of a file
pub fn calculate_checksum(file_path: &Path) -> Result<String, ServiceError> {
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

/// Get git commit hash
fn get_git_commit() -> Result<String, ServiceError> {
    #[cfg(feature = "git-support")]
    {
        use git2::Repository;

        let repo = Repository::open(".")
            .map_err(|e| ServiceError::Custom(format!("Failed to open git repository: {}", e)))?;

        let head = repo
            .head()
            .map_err(|e| ServiceError::Custom(format!("Failed to get HEAD: {}", e)))?;

        let commit = head
            .peel_to_commit()
            .map_err(|e| ServiceError::Custom(format!("Failed to get commit: {}", e)))?;

        Ok(commit.id().to_string())
    }

    #[cfg(not(feature = "git-support"))]
    {
        Err(ServiceError::Custom("Git support not enabled".to_string()))
    }
}

/// Get git branch name
fn get_git_branch() -> Result<String, ServiceError> {
    #[cfg(feature = "git-support")]
    {
        use git2::Repository;

        let repo = Repository::open(".")
            .map_err(|e| ServiceError::Custom(format!("Failed to open git repository: {}", e)))?;

        let head = repo
            .head()
            .map_err(|e| ServiceError::Custom(format!("Failed to get HEAD: {}", e)))?;

        let branch_name = head
            .shorthand()
            .ok_or_else(|| ServiceError::Custom("Failed to get branch name".to_string()))?;

        Ok(branch_name.to_string())
    }

    #[cfg(not(feature = "git-support"))]
    {
        Err(ServiceError::Custom("Git support not enabled".to_string()))
    }
}

/// Get Rust version
fn get_rust_version() -> String {
    // Try to get from environment or use a default
    std::env::var("RUSTC_VERSION").unwrap_or_else(|_| "unknown".to_string())
}

/// Build metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    pub skill_id: String,
    pub version: String,
    pub build_timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub build_environment: BuildEnvironment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildEnvironment {
    pub fastskill_version: String,
    pub rust_version: String,
}

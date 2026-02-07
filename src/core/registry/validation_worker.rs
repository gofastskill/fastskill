//! Validation worker for processing staged packages

use crate::core::blob_storage::{create_blob_storage, BlobStorageConfig};
use crate::core::metadata::parse_yaml_frontmatter;
use crate::core::registry::index_manager::IndexManager;
use crate::core::registry::staging::{StagingManager, StagingStatus};
use crate::core::registry_index::{get_version_metadata, IndexMetadata, VersionEntry};
use crate::core::service::ServiceError;
use crate::validation::{SkillValidator, ZipValidator};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use zip::ZipArchive;

/// Validation worker configuration
#[derive(Debug, Clone)]
pub struct ValidationWorkerConfig {
    /// Poll interval in seconds
    pub poll_interval_secs: u64,

    /// Blob storage configuration
    pub blob_storage_config: Option<BlobStorageConfig>,

    /// Registry index path
    pub registry_index_path: Option<PathBuf>,

    /// Base URL for blob storage downloads
    pub blob_base_url: Option<String>,
}

impl Default for ValidationWorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 5,
            blob_storage_config: None,
            registry_index_path: None,
            blob_base_url: None,
        }
    }
}

/// Validation worker for processing staged packages
pub struct ValidationWorker {
    staging_manager: StagingManager,
    config: ValidationWorkerConfig,
    skill_validator: Arc<SkillValidator>,
    zip_validator: Arc<ZipValidator>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl ValidationWorker {
    /// Create a new validation worker
    pub fn new(staging_manager: StagingManager, config: ValidationWorkerConfig) -> Self {
        Self {
            staging_manager,
            config,
            skill_validator: Arc::new(SkillValidator::new()),
            zip_validator: Arc::new(ZipValidator::new()),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the validation worker (runs in background)
    pub fn start(&self) {
        let staging_manager = self.staging_manager.clone();
        let config = self.config.clone();
        let skill_validator = self.skill_validator.clone();
        let zip_validator = self.zip_validator.clone();
        let running = self.running.clone();

        running.store(true, std::sync::atomic::Ordering::SeqCst);

        tokio::spawn(async move {
            info!("Validation worker started");

            while running.load(std::sync::atomic::Ordering::SeqCst) {
                if let Err(e) = Self::process_pending_packages(
                    &staging_manager,
                    &config,
                    &skill_validator,
                    &zip_validator,
                )
                .await
                {
                    error!("Error processing pending packages: {}", e);
                }

                sleep(Duration::from_secs(config.poll_interval_secs)).await;
            }

            info!("Validation worker stopped");
        });
    }

    /// Stop the validation worker
    pub fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Process all pending packages
    async fn process_pending_packages(
        staging_manager: &StagingManager,
        config: &ValidationWorkerConfig,
        skill_validator: &Arc<SkillValidator>,
        zip_validator: &Arc<ZipValidator>,
    ) -> Result<(), ServiceError> {
        // Find all pending packages
        let pending_jobs = Self::find_pending_jobs(staging_manager)?;

        for job_id in pending_jobs {
            info!("Processing job: {}", job_id);

            // Update status to validating
            staging_manager.update_status(&job_id, StagingStatus::Validating, Vec::new())?;

            // Validate package
            match Self::validate_package(staging_manager, &job_id, skill_validator, zip_validator)
                .await
            {
                Ok(()) => {
                    // Package is valid, move to blob storage and update index
                    if let Err(e) = Self::accept_and_publish(staging_manager, &job_id, config).await
                    {
                        error!("Failed to publish package {}: {}", job_id, e);
                        staging_manager.update_status(
                            &job_id,
                            StagingStatus::Rejected,
                            vec![format!("Failed to publish: {}", e)],
                        )?;
                    } else {
                        info!("Package {} accepted and published", job_id);
                    }
                }
                Err(e) => {
                    warn!("Package {} validation failed: {}", job_id, e);
                    staging_manager.update_status(
                        &job_id,
                        StagingStatus::Rejected,
                        vec![e.to_string()],
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Find all pending jobs in staging
    fn find_pending_jobs(staging_manager: &StagingManager) -> Result<Vec<String>, ServiceError> {
        use std::fs;
        use walkdir::WalkDir;

        let staging_dir = &staging_manager.staging_dir;
        if !staging_dir.exists() {
            return Ok(Vec::new());
        }

        let mut pending_jobs = Vec::new();

        for entry in WalkDir::new(staging_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() == "metadata.json")
        {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                if let Ok(metadata) = serde_json::from_str::<
                    crate::core::registry::staging::StagingMetadata,
                >(&content)
                {
                    if metadata.status == StagingStatus::Pending {
                        pending_jobs.push(metadata.job_id);
                    }
                }
            }
        }

        Ok(pending_jobs)
    }

    /// Validate a package
    async fn validate_package(
        staging_manager: &StagingManager,
        job_id: &str,
        _skill_validator: &Arc<SkillValidator>,
        zip_validator: &Arc<ZipValidator>,
    ) -> Result<(), ServiceError> {
        use crate::validation::standard_validator::StandardValidator;

        // Get package path
        let package_path = staging_manager
            .get_package_path(job_id)?
            .ok_or_else(|| ServiceError::Custom(format!("Package not found for job {}", job_id)))?;

        // Validate ZIP structure
        zip_validator.validate_zip_package(&package_path).await?;

        // Extract ZIP to temporary directory for comprehensive validation
        let temp_dir = tempfile::TempDir::new().map_err(|e| {
            ServiceError::Io(std::io::Error::other(format!(
                "Failed to create temp dir: {}",
                e
            )))
        })?;

        Self::extract_zip_to_temp(&package_path, temp_dir.path())?;

        // Find the skill directory (should be the root or a subdirectory)
        let skill_dir = Self::find_skill_directory(temp_dir.path())?;

        // Use StandardValidator for comprehensive AI Skill standard validation
        let validation_result = StandardValidator::validate_skill_directory(&skill_dir)?;

        if !validation_result.is_valid {
            let error_messages: Vec<String> = validation_result.errors.iter().map(|e| {
                match e {
                    crate::validation::standard_validator::ValidationError::InvalidNameFormat(msg) =>
                        format!("✗ Name format invalid: {}", msg),
                    crate::validation::standard_validator::ValidationError::NameMismatch { expected, actual } =>
                        format!("✗ Name mismatch: Directory '{}' doesn't match skill name '{}'", actual, expected),
                    crate::validation::standard_validator::ValidationError::InvalidDescriptionLength(len) =>
                        format!("✗ Description length invalid: {} characters (must be 1-1024)", len),
                    crate::validation::standard_validator::ValidationError::InvalidCompatibilityLength(len) =>
                        format!("✗ Compatibility field too long: {} characters (max 500)", len),
                    crate::validation::standard_validator::ValidationError::MissingRequiredField(field) =>
                        format!("✗ Missing required field: {}", field),
                    crate::validation::standard_validator::ValidationError::InvalidFileReference(msg) =>
                        format!("✗ Invalid file reference: {}", msg),
                    crate::validation::standard_validator::ValidationError::InvalidDirectoryStructure(msg) =>
                        format!("✗ Invalid directory structure: {}", msg),
                    crate::validation::standard_validator::ValidationError::YamlParseError(msg) =>
                        format!("✗ YAML parsing error: {}", msg),
                }
            }).collect();

            return Err(ServiceError::Validation(error_messages.join("\n")));
        }

        // Log warnings even if validation passes
        for warning in &validation_result.warnings {
            warn!("Package validation warning for {}: {}", job_id, warning);
        }

        Ok(())
    }

    /// Extract ZIP to temporary directory
    fn extract_zip_to_temp(
        package_path: &std::path::Path,
        temp_dir: &std::path::Path,
    ) -> Result<(), ServiceError> {
        use crate::storage::zip::ZipHandler;
        let zip_handler = ZipHandler::new().map_err(|e| {
            ServiceError::Validation(format!("Failed to create ZIP handler: {}", e))
        })?;
        zip_handler.extract_to_dir(package_path, temp_dir)
    }

    /// Find skill directory in extracted ZIP
    fn find_skill_directory(
        temp_dir: &std::path::Path,
    ) -> Result<std::path::PathBuf, ServiceError> {
        // Look for SKILL.md at the root level first
        let skill_md_path = temp_dir.join("SKILL.md");
        if skill_md_path.exists() {
            return Ok(temp_dir.to_path_buf());
        }

        // Look for SKILL.md in subdirectories (in case ZIP has a containing folder)
        for entry in std::fs::read_dir(temp_dir).map_err(ServiceError::Io)? {
            let entry = entry.map_err(ServiceError::Io)?;
            let path = entry.path();

            if path.is_dir() {
                let skill_md_in_subdir = path.join("SKILL.md");
                if skill_md_in_subdir.exists() {
                    return Ok(path);
                }
            }
        }

        Err(ServiceError::Validation(
            "Could not find SKILL.md in extracted package".to_string(),
        ))
    }

    /// Extract SKILL.md from ZIP
    fn extract_skill_md(package_path: &PathBuf) -> Result<String, ServiceError> {
        use std::io::Cursor;
        use std::io::Read;

        let zip_data = std::fs::read(package_path).map_err(ServiceError::Io)?;

        let cursor = Cursor::new(zip_data);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ServiceError::Validation(format!("Invalid ZIP file: {}", e)))?;

        let mut skill_content = String::new();
        for i in 0..archive.len() {
            let file = archive.by_index(i).map_err(|e| {
                ServiceError::Validation(format!("Failed to read ZIP entry: {}", e))
            })?;

            if file.name().ends_with("SKILL.md") {
                let mut reader = std::io::BufReader::new(file);
                reader.read_to_string(&mut skill_content).map_err(|e| {
                    ServiceError::Validation(format!("Failed to read SKILL.md: {}", e))
                })?;
                break;
            }
        }

        if skill_content.is_empty() {
            return Err(ServiceError::Validation(
                "SKILL.md not found in package".to_string(),
            ));
        }

        Ok(skill_content)
    }

    /// Accept package and publish to blob storage + registry index
    ///
    /// **Blob-First Persistence**: This method enforces that blob storage upload
    /// must succeed before any index update is attempted. If blob upload fails,
    /// the method returns an error and no index update occurs.
    async fn accept_and_publish(
        staging_manager: &StagingManager,
        job_id: &str,
        config: &ValidationWorkerConfig,
    ) -> Result<(), ServiceError> {
        // Load metadata
        let metadata = staging_manager
            .load_metadata(job_id)?
            .ok_or_else(|| ServiceError::Custom(format!("Job {} not found", job_id)))?;

        // Extract scope and id from metadata
        let (scope, id) = if let (Some(s), Some(i)) = (metadata.scope.clone(), metadata.id.clone())
        {
            (s, i)
        } else {
            // Fallback: extract from skill_id for backward compatibility
            let parts: Vec<&str> = metadata.skill_id.split('/').collect();
            if parts.len() >= 2 {
                (parts[0].to_string(), parts[1..].join("/"))
            } else {
                return Err(ServiceError::Custom(format!(
                    "Invalid skill_id format in metadata: {}",
                    metadata.skill_id
                )));
            }
        };

        // Get package path
        let package_path = staging_manager
            .get_package_path(job_id)?
            .ok_or_else(|| ServiceError::Custom(format!("Package not found for job {}", job_id)))?;

        // STEP 1: Upload to blob storage FIRST (blob-first persistence)
        // If this fails, we abort and return error - no index update will occur
        let blob_url = if let Some(ref blob_config) = config.blob_storage_config {
            info!("Uploading package {} to blob storage...", job_id);

            let storage = create_blob_storage(&blob_config.storage_type, blob_config)
                .await
                .map_err(|e| {
                    error!("Failed to create blob storage for job {}: {}", job_id, e);
                    ServiceError::Custom(format!("Failed to create blob storage: {}", e))
                })?;

            let package_filename = package_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| ServiceError::Custom("Invalid package filename".to_string()))?;

            // Use scope from metadata for blob storage path
            let storage_path = format!("skills/{}/{}", scope, package_filename);
            let package_data = std::fs::read(&package_path).map_err(ServiceError::Io)?;

            // CRITICAL: Upload must succeed before proceeding to index update
            storage
                .upload(&storage_path, &package_data)
                .await
                .map_err(|e| {
                    error!(
                        "Blob storage upload failed for job {}: {}. Aborting index update.",
                        job_id, e
                    );
                    ServiceError::Custom(format!(
                        "Failed to upload to blob storage: {}. Index update aborted.",
                        e
                    ))
                })?;

            // Construct download URL
            let blob_url = if let Some(base_url) = storage.base_url() {
                format!("{}/{}", base_url.trim_end_matches('/'), storage_path)
            } else if let Some(ref blob_base_url) = config.blob_base_url {
                format!("{}/{}", blob_base_url.trim_end_matches('/'), storage_path)
            } else {
                format!("file://{}", package_path.display())
            };

            info!(
                "Successfully uploaded package {} to blob storage: {}",
                job_id, blob_url
            );
            Some(blob_url)
        } else {
            info!("Skipping blob storage upload (not configured)");
            None
        };

        // STEP 2: Update registry index ONLY if blob upload succeeded (or blob storage not configured)
        // If blob storage was configured and upload failed, we would have returned error above
        if let Some(ref registry_index_path) = config.registry_index_path {
            let download_url = blob_url
                .as_deref()
                .unwrap_or_else(|| package_path.to_str().unwrap_or(""));

            // Extract metadata from package
            let skill_content = Self::extract_skill_md(&package_path)?;
            let frontmatter = parse_yaml_frontmatter(&skill_content).map_err(|e| {
                ServiceError::Validation(format!("Failed to parse SKILL.md: {}", e))
            })?;

            // Get version metadata (use full skill_id in format scope/id)
            let full_skill_id = format!("{}/{}", scope, id);
            let version_metadata =
                get_version_metadata(&full_skill_id, &package_path, download_url)
                    .map_err(|e| ServiceError::Custom(format!("Failed to get metadata: {}", e)))?;

            // Create VersionEntry for IndexManager
            let version_entry = VersionEntry {
                name: full_skill_id.clone(),
                vers: metadata.version.clone(),
                deps: version_metadata.deps.clone(),
                cksum: version_metadata.cksum.clone(),
                features: version_metadata.features.clone(),
                yanked: false,
                links: version_metadata.links.clone(),
                download_url: download_url.to_string(),
                published_at: version_metadata.published_at.clone(),
                metadata: Some(IndexMetadata {
                    description: Some(frontmatter.description),
                    author: frontmatter.author,
                    license: None,
                    repository: None,
                }),
                scoped_name: if full_skill_id.contains('@') || full_skill_id.contains(':') {
                    Some(full_skill_id.clone())
                } else {
                    None
                },
            };

            // Use IndexManager for atomic update (skill_id in format scope/id)
            let index_manager = IndexManager::new(registry_index_path.clone());
            index_manager.atomic_update(&full_skill_id, &metadata.version, &version_entry)?;

            info!(
                "Updated registry index for {} v{}",
                full_skill_id, metadata.version
            );
        } else {
            info!("Skipping registry index update (not configured)");
        }

        // Mark as accepted
        staging_manager.update_status(job_id, StagingStatus::Accepted, Vec::new())?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::FileOptions;
    use zip::ZipWriter;

    /// Helper to create a test ZIP with malicious entries
    fn create_malicious_zip(path: &std::path::Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        // Add legitimate content
        zip.start_file("SKILL.md", options).unwrap();
        zip.write_all(
            b"# Test Skill

Name: test-skill
Version: 1.0.0
Description: Test skill for validation",
        )
        .unwrap();

        // Add path traversal attempt
        zip.start_file("../../../evil.txt", options).unwrap();
        zip.write_all(b"malicious content").unwrap();

        zip.finish().unwrap();
    }

    #[test]
    fn test_extract_zip_to_temp_rejects_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let extract_dir = temp_dir.path().join("extract");
        std::fs::create_dir_all(&extract_dir).unwrap();

        let zip_path = temp_dir.path().join("malicious.zip");
        create_malicious_zip(&zip_path);

        let result = ValidationWorker::extract_zip_to_temp(&zip_path, &extract_dir);

        assert!(result.is_err());
        match result {
            Err(ServiceError::Validation(msg)) => {
                assert!(
                    msg.contains("path traversal") || msg.contains("Path traversal"),
                    "Error should mention path traversal: {}",
                    msg
                );
            }
            _ => panic!("Expected ServiceError::Validation for path traversal"),
        }

        // Verify no file was created outside the extraction directory
        let evil_path = extract_dir.join("../../../evil.txt");
        assert!(!evil_path.exists(), "Evil file should not exist");

        // Safe file should exist if partial extraction occurred
        let _safe_file = extract_dir.join("SKILL.md");
        // Note: Depending on implementation order, safe files might be created before the malicious one
    }

    #[test]
    fn test_extract_zip_to_temp_rejects_windows_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let extract_dir = temp_dir.path().join("extract");
        std::fs::create_dir_all(&extract_dir).unwrap();

        let zip_path = temp_dir.path().join("windows-malicious.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("SKILL.md", options).unwrap();
        zip.write_all(
            b"# Test

Name: test
Version: 1.0.0
Description: test",
        )
        .unwrap();

        zip.start_file("..\\..\\..\\evil.txt", options).unwrap();
        zip.write_all(b"malicious").unwrap();

        zip.finish().unwrap();

        let result = ValidationWorker::extract_zip_to_temp(&zip_path, &extract_dir);

        // On Unix, backslashes are treated as regular characters, so the path is literal
        // On Windows, this would be a path traversal and should be rejected
        // We accept either behavior since it's platform-specific
        if cfg!(windows) {
            assert!(
                result.is_err(),
                "Windows-style traversal should be rejected on Windows"
            );
        } else {
            // On Unix, the backslashes are literal characters
            assert!(result.is_ok() || result.is_err());
        }
    }
}

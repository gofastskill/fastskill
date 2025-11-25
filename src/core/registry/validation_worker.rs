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
        // Get package path
        let package_path = staging_manager
            .get_package_path(job_id)?
            .ok_or_else(|| ServiceError::Custom(format!("Package not found for job {}", job_id)))?;

        // Validate ZIP structure
        zip_validator.validate_zip_package(&package_path).await?;

        // Extract and validate SKILL.md
        let skill_content = Self::extract_skill_md(&package_path)?;

        // Parse frontmatter
        let _frontmatter = parse_yaml_frontmatter(&skill_content).map_err(|e| {
            ServiceError::Validation(format!("Invalid SKILL.md frontmatter: {}", e))
        })?;

        // Basic validation: check ZIP integrity
        let file = std::fs::File::open(&package_path).map_err(ServiceError::Io)?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| ServiceError::Validation(format!("Invalid ZIP file: {}", e)))?;

        // Check for SKILL.md
        let mut found_skill_md = false;
        for i in 0..archive.len() {
            let file = archive.by_index(i).map_err(|e| {
                ServiceError::Validation(format!("Failed to read ZIP entry: {}", e))
            })?;
            if file.name().ends_with("SKILL.md") {
                found_skill_md = true;
                break;
            }
        }

        if !found_skill_md {
            return Err(ServiceError::Validation(
                "SKILL.md not found in package".to_string(),
            ));
        }

        Ok(())
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

            // Extract scope from the user who uploaded the package
            // Scope is the part before '/' in uploaded_by, or the entire uploaded_by if no '/'
            let scope = metadata
                .uploaded_by
                .as_ref()
                .map(|u| u.split('/').next().unwrap_or(u).to_string())
                .unwrap_or_else(|| "unknown".to_string());

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

            // Get version metadata
            let version_metadata =
                get_version_metadata(&metadata.skill_id, &package_path, download_url)
                    .map_err(|e| ServiceError::Custom(format!("Failed to get metadata: {}", e)))?;

            // Create VersionEntry for IndexManager
            let version_entry = VersionEntry {
                name: metadata.skill_id.clone(),
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
                    tags: Some(frontmatter.tags),
                    capabilities: Some(frontmatter.capabilities),
                    license: None,
                    repository: None,
                }),
                scoped_name: if metadata.skill_id.contains('@') || metadata.skill_id.contains(':') {
                    Some(metadata.skill_id.clone())
                } else {
                    None
                },
            };

            // Use IndexManager for atomic update
            let index_manager = IndexManager::new(registry_index_path.clone());
            index_manager.atomic_update(&metadata.skill_id, &metadata.version, &version_entry)?;

            info!(
                "Updated registry index for {} v{}",
                metadata.skill_id, metadata.version
            );
        } else {
            info!("Skipping registry index update (not configured)");
        }

        // Mark as accepted
        staging_manager.update_status(job_id, StagingStatus::Accepted, Vec::new())?;

        Ok(())
    }
}

//! Staging area management for registry publishing

use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

/// Staging metadata for a published package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagingMetadata {
    pub skill_id: String,
    pub version: String,
    pub checksum: String,
    pub uploaded_at: DateTime<Utc>,
    pub uploaded_by: Option<String>,
    pub status: StagingStatus,
    pub validation_errors: Vec<String>,
    pub job_id: String,
}

/// Status of a package in staging
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StagingStatus {
    Pending,
    Validating,
    Accepted,
    Rejected,
}

impl StagingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StagingStatus::Pending => "pending",
            StagingStatus::Validating => "validating",
            StagingStatus::Accepted => "accepted",
            StagingStatus::Rejected => "rejected",
        }
    }
}

/// Staging area manager
#[derive(Clone)]
pub struct StagingManager {
    pub(crate) staging_dir: PathBuf,
}

impl StagingManager {
    /// Create a new staging manager
    pub fn new(staging_dir: PathBuf) -> Self {
        Self { staging_dir }
    }

    /// Initialize staging directory structure
    pub fn initialize(&self) -> Result<(), ServiceError> {
        fs::create_dir_all(&self.staging_dir).map_err(ServiceError::Io)?;
        Ok(())
    }

    /// Get staging path for a skill version
    pub fn get_staging_path(&self, skill_id: &str, version: &str) -> PathBuf {
        // skill_id is now in org/package format, so we need to handle the '/' properly
        // Instead of sanitizing the whole skill_id, we split on '/' and sanitize each component
        let mut path = self.staging_dir.clone();

        // Split skill_id on '/' and add each component as a directory
        for component in skill_id.split('/') {
            path = path.join(sanitize_path(component));
        }

        // Add version as final directory
        path.join(sanitize_path(version))
    }

    /// Store package in staging area
    pub async fn store_package(
        &self,
        skill_id: &str,
        version: &str,
        package_data: &[u8],
        uploaded_by: Option<&str>,
    ) -> Result<(PathBuf, String), ServiceError> {
        let staging_path = self.get_staging_path(skill_id, version);
        tracing::info!(
            "StagingManager: creating directory: {}",
            staging_path.display()
        );
        match fs::create_dir_all(&staging_path) {
            Ok(()) => {
                tracing::info!(
                    "StagingManager: successfully created directory: {}",
                    staging_path.display()
                );
            }
            Err(e) => {
                tracing::error!(
                    "StagingManager: failed to create directory {}: {}",
                    staging_path.display(),
                    e
                );
                tracing::error!("StagingManager: error kind: {:?}", e.kind());
                tracing::error!("StagingManager: raw OS error: {:?}", e.raw_os_error());
                // Check if parent directories exist
                if let Some(parent) = staging_path.parent() {
                    tracing::error!(
                        "StagingManager: parent directory exists: {}",
                        parent.exists()
                    );
                    tracing::error!("StagingManager: parent path: {}", parent.display());
                }
                return Err(ServiceError::Io(e));
            }
        }

        // Generate job ID
        let job_id = format!("job_{}", Uuid::new_v4().simple());

        // Calculate checksum
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(package_data);
        let checksum = format!("sha256:{:x}", hasher.finalize());

        // Use package name from skill_id (part after '/') for filename
        // skill_id format is scope/package, so package_name is the part after '/'
        let package_name = skill_id.split('/').nth(1).unwrap_or(skill_id);
        let package_filename = format!("{}-{}.zip", package_name, version);
        let package_path = staging_path.join(&package_filename);
        tracing::info!(
            "StagingManager: writing package file: {}",
            package_path.display()
        );
        fs::write(&package_path, package_data).map_err(ServiceError::Io)?;

        // Create metadata
        let metadata = StagingMetadata {
            skill_id: skill_id.to_string(),
            version: version.to_string(),
            checksum,
            uploaded_at: Utc::now(),
            uploaded_by: uploaded_by.map(|s| s.to_string()),
            status: StagingStatus::Pending,
            validation_errors: Vec::new(),
            job_id: job_id.clone(),
        };

        // Write metadata
        let metadata_path = staging_path.join("metadata.json");
        tracing::info!(
            "StagingManager: writing metadata file: {}",
            metadata_path.display()
        );
        let metadata_json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| ServiceError::Custom(format!("Failed to serialize metadata: {}", e)))?;
        fs::write(&metadata_path, metadata_json).map_err(ServiceError::Io)?;

        Ok((staging_path, job_id))
    }

    /// Load metadata for a job
    pub fn load_metadata(&self, job_id: &str) -> Result<Option<StagingMetadata>, ServiceError> {
        // Search for metadata with matching job_id
        if !self.staging_dir.exists() {
            return Ok(None);
        }

        for entry in walkdir::WalkDir::new(&self.staging_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() == "metadata.json")
        {
            let metadata_path = entry.path();
            if let Ok(content) = fs::read_to_string(metadata_path) {
                if let Ok(metadata) = serde_json::from_str::<StagingMetadata>(&content) {
                    if metadata.job_id == job_id {
                        return Ok(Some(metadata));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Update metadata status
    pub fn update_status(
        &self,
        job_id: &str,
        status: StagingStatus,
        validation_errors: Vec<String>,
    ) -> Result<(), ServiceError> {
        let metadata = self
            .load_metadata(job_id)?
            .ok_or_else(|| ServiceError::Custom(format!("Job {} not found", job_id)))?;

        let staging_path = self.get_staging_path(&metadata.skill_id, &metadata.version);
        let metadata_path = staging_path.join("metadata.json");

        let updated_metadata = StagingMetadata {
            status,
            validation_errors,
            ..metadata
        };

        let metadata_json = serde_json::to_string_pretty(&updated_metadata)
            .map_err(|e| ServiceError::Custom(format!("Failed to serialize metadata: {}", e)))?;
        fs::write(&metadata_path, metadata_json).map_err(ServiceError::Io)?;

        Ok(())
    }

    /// Get package path for a job
    pub fn get_package_path(&self, job_id: &str) -> Result<Option<PathBuf>, ServiceError> {
        let metadata = self
            .load_metadata(job_id)?
            .ok_or_else(|| ServiceError::Custom(format!("Job {} not found", job_id)))?;

        let staging_path = self.get_staging_path(&metadata.skill_id, &metadata.version);
        // Use package name from skill_id (part after '/') for filename
        // This must match the filename used in store_package
        let package_name = metadata
            .skill_id
            .split('/')
            .nth(1)
            .unwrap_or(&metadata.skill_id);
        let package_filename = format!("{}-{}.zip", package_name, metadata.version);
        let package_path = staging_path.join(&package_filename);

        if package_path.exists() {
            Ok(Some(package_path))
        } else {
            Ok(None)
        }
    }
}

/// Sanitize path component to avoid directory traversal
fn sanitize_path(component: &str) -> String {
    component
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_store_and_load_package() {
        let temp_dir = TempDir::new().unwrap();
        let manager = StagingManager::new(temp_dir.path().to_path_buf());
        manager.initialize().unwrap();

        let package_data = b"test package data";
        let (staging_path, job_id) = manager
            .store_package("test-skill", "1.0.0", package_data, Some("user1"))
            .await
            .unwrap();

        assert!(staging_path.join("test-skill-1.0.0.zip").exists());
        assert!(staging_path.join("metadata.json").exists());

        let metadata = manager.load_metadata(&job_id).unwrap().unwrap();
        assert_eq!(metadata.skill_id, "test-skill");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.status, StagingStatus::Pending);
        assert_eq!(metadata.uploaded_by, Some("user1".to_string()));
    }
}

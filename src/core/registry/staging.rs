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
    /// Full skill ID in format scope/id (kept for backward compatibility)
    pub skill_id: String,
    /// Scope (e.g., "dev-user")
    #[serde(default)]
    pub scope: Option<String>,
    /// Package ID without scope (e.g., "test-skill")
    #[serde(default)]
    pub id: Option<String>,
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
    /// Accepts scope and id separately
    pub fn get_staging_path(&self, scope: &str, id: &str, version: &str) -> PathBuf {
        self.staging_dir
            .join(sanitize_path(scope))
            .join(sanitize_path(id))
            .join(sanitize_path(version))
    }

    /// Store package in staging area
    /// Accepts scope and id separately
    pub async fn store_package(
        &self,
        scope: &str,
        id: &str,
        version: &str,
        package_data: &[u8],
        uploaded_by: Option<&str>,
    ) -> Result<(PathBuf, String), ServiceError> {
        let staging_path = self.get_staging_path(scope, id, version);
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

        // Use id directly for package filename
        let package_filename = format!("{}-{}.zip", id, version);
        let package_path = staging_path.join(&package_filename);
        tracing::info!(
            "StagingManager: writing package file: {}",
            package_path.display()
        );
        fs::write(&package_path, package_data).map_err(ServiceError::Io)?;

        // Create metadata with scope and id stored separately
        let full_skill_id = format!("{}/{}", scope, id);
        let metadata = StagingMetadata {
            skill_id: full_skill_id.clone(),
            scope: Some(scope.to_string()),
            id: Some(id.to_string()),
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
        let staging_path = self.get_staging_path(&scope, &id, &metadata.version);
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
        let staging_path = self.get_staging_path(&scope, &id, &metadata.version);
        // Use id directly for filename (must match store_package)
        let package_filename = format!("{}-{}.zip", id, metadata.version);
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
#[allow(clippy::unwrap_used)]
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
            .store_package(
                "test-scope",
                "test-skill",
                "1.0.0",
                package_data,
                Some("user1"),
            )
            .await
            .unwrap();

        assert!(staging_path.join("test-skill-1.0.0.zip").exists());
        assert!(staging_path.join("metadata.json").exists());

        let metadata = manager.load_metadata(&job_id).unwrap().unwrap();
        assert_eq!(metadata.skill_id, "test-scope/test-skill");
        assert_eq!(metadata.scope, Some("test-scope".to_string()));
        assert_eq!(metadata.id, Some("test-skill".to_string()));
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.status, StagingStatus::Pending);
        assert_eq!(metadata.uploaded_by, Some("user1".to_string()));
    }
}

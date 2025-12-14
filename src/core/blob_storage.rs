//! Blob storage abstraction for artifact publishing

use crate::core::service::ServiceError;
use async_trait::async_trait;
#[allow(unused_imports)] // StreamExt is used in download() method
use futures::StreamExt;
#[cfg(feature = "registry-publish")]
use tracing::info;
use std::sync::Arc;

/// Trait for blob storage backends
#[async_trait]
pub trait BlobStorage: Send + Sync {
    /// Upload data to blob storage
    async fn upload(&self, path: &str, data: &[u8]) -> Result<String, ServiceError>;

    /// Download data from blob storage
    async fn download(&self, path: &str) -> Result<Vec<u8>, ServiceError>;

    /// Check if a path exists in blob storage
    async fn exists(&self, path: &str) -> Result<bool, ServiceError>;

    /// Delete a path from blob storage
    async fn delete(&self, path: &str) -> Result<(), ServiceError>;

    /// Get the base URL for downloads
    fn base_url(&self) -> Option<&str>;
}

/// Local filesystem blob storage (for testing)
pub struct LocalBlobStorage {
    base_path: std::path::PathBuf,
    base_url: Option<String>,
}

impl LocalBlobStorage {
    pub fn new(base_path: std::path::PathBuf, base_url: Option<String>) -> Self {
        Self {
            base_path,
            base_url,
        }
    }
}

#[async_trait]
impl BlobStorage for LocalBlobStorage {
    async fn upload(&self, path: &str, data: &[u8]) -> Result<String, ServiceError> {
        let full_path = self.base_path.join(path);

        // Create parent directory if needed
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(ServiceError::Io)?;
        }

        tokio::fs::write(&full_path, data).await.map_err(ServiceError::Io)?;

        Ok(path.to_string())
    }

    async fn download(&self, path: &str) -> Result<Vec<u8>, ServiceError> {
        let full_path = self.base_path.join(path);
        let data = tokio::fs::read(&full_path).await.map_err(ServiceError::Io)?;
        Ok(data)
    }

    async fn exists(&self, path: &str) -> Result<bool, ServiceError> {
        let full_path = self.base_path.join(path);
        Ok(full_path.exists())
    }

    async fn delete(&self, path: &str) -> Result<(), ServiceError> {
        let full_path = self.base_path.join(path);
        if full_path.exists() {
            if full_path.is_dir() {
                tokio::fs::remove_dir_all(&full_path).await.map_err(ServiceError::Io)?;
            } else {
                tokio::fs::remove_file(&full_path).await.map_err(ServiceError::Io)?;
            }
        }
        Ok(())
    }

    fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }
}

/// S3 blob storage (supports AWS S3 and S3-compatible services via endpoint)
#[cfg(feature = "registry-publish")]
pub struct S3BlobStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
    base_url: Option<String>,
}

#[cfg(feature = "registry-publish")]
impl S3BlobStorage {
    pub async fn new(
        bucket: String,
        region: String,
        endpoint: Option<String>,
        access_key: String,
        secret_key: String,
        base_url: Option<String>,
    ) -> Result<Self, ServiceError> {
        use aws_config::meta::region::RegionProviderChain;
        use aws_config::Region;
        use aws_sdk_s3::config::Credentials;

        // Build AWS config
        let mut config_builder = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(RegionProviderChain::first_try(if region.is_empty() {
                Region::new("us-east-1")
            } else {
                Region::new(region.clone())
            }));

        // Set custom endpoint if provided (for S3-compatible services)
        if let Some(endpoint_url) = endpoint {
            config_builder = config_builder.endpoint_url(endpoint_url);
        }

        // Handle credentials: use config if provided, otherwise let AWS SDK use default chain
        if !access_key.is_empty() && !secret_key.is_empty() {
            let credentials = Credentials::new(access_key, secret_key, None, None, "fastskill");
            config_builder = config_builder.credentials_provider(credentials);
        }

        let config = config_builder.load().await;
        let client = aws_sdk_s3::Client::new(&config);

        Ok(Self {
            client,
            bucket,
            base_url,
        })
    }
}

#[cfg(feature = "registry-publish")]
#[async_trait]
impl BlobStorage for S3BlobStorage {
    async fn upload(&self, path: &str, data: &[u8]) -> Result<String, ServiceError> {
        use aws_sdk_s3::primitives::ByteStream;

        let body = ByteStream::from(data.to_vec());

        let request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(path)
            .body(body)
            .content_type("application/zip");

        let _result = request.send().await.map_err(|e| {
            ServiceError::Custom(format!(
                "Failed to upload to S3: {}",
                map_s3_error(&e as &dyn std::error::Error)
            ))
        })?;

        // Return the path that was uploaded
        Ok(path.to_string())
    }

    async fn download(&self, path: &str) -> Result<Vec<u8>, ServiceError> {
        let result =
            self.client
                .get_object()
                .bucket(&self.bucket)
                .key(path)
                .send()
                .await
                .map_err(|e| {
                    let error_msg = map_s3_error(&e as &dyn std::error::Error);
                    if error_msg.contains("NoSuchKey") || error_msg.contains("not found") {
                        ServiceError::Custom(format!("Object not found: {}", path))
                    } else {
                        ServiceError::Custom(format!("Failed to download from S3: {}", error_msg))
                    }
                })?;

        let mut body = result.body;
        let mut data = Vec::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk
                .map_err(|e| ServiceError::Custom(format!("Failed to read S3 response: {}", e)))?;
            data.extend_from_slice(&chunk);
        }

        Ok(data)
    }

    async fn exists(&self, path: &str) -> Result<bool, ServiceError> {
        let result = self.client.head_object().bucket(&self.bucket).key(path).send().await;

        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                let error_msg = map_s3_error(&e as &dyn std::error::Error);
                if error_msg.contains("NoSuchKey")
                    || error_msg.contains("404")
                    || error_msg.contains("not found")
                {
                    Ok(false)
                } else {
                    Err(ServiceError::Custom(format!(
                        "Failed to check object existence in S3: {}",
                        error_msg
                    )))
                }
            }
        }
    }

    async fn delete(&self, path: &str) -> Result<(), ServiceError> {
        let result = self.client.delete_object().bucket(&self.bucket).key(path).send().await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_msg = map_s3_error(&e as &dyn std::error::Error);
                // Delete is idempotent - if object doesn't exist, that's OK
                if error_msg.contains("NoSuchKey") || error_msg.contains("not found") {
                    Ok(())
                } else {
                    Err(ServiceError::Custom(format!(
                        "Failed to delete from S3: {}",
                        error_msg
                    )))
                }
            }
        }
    }

    fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }
}

/// Map AWS SDK errors to user-friendly error messages
#[cfg(feature = "registry-publish")]
fn map_s3_error(err: &dyn std::error::Error) -> String {
    let err_str = err.to_string();

    // Check for common S3 error patterns
    if err_str.contains("NoSuchKey") || err_str.contains("not found") {
        "Object not found".to_string()
    } else if err_str.contains("NoSuchBucket") {
        "Bucket not found".to_string()
    } else if err_str.contains("AccessDenied") || err_str.contains("access denied") {
        "Access denied".to_string()
    } else if err_str.contains("timeout") {
        "Request timeout".to_string()
    } else {
        format!("S3 error: {}", err_str)
    }
}

/// Create blob storage from configuration
pub async fn create_blob_storage(
    storage_type: &str,
    config: &BlobStorageConfig,
) -> Result<Arc<dyn BlobStorage>, ServiceError> {
    match storage_type {
        "local" => {
            let base_path = std::path::PathBuf::from(&config.base_path);
            Ok(Arc::new(LocalBlobStorage::new(
                base_path,
                config.base_url.clone(),
            )))
        }
        #[cfg(feature = "registry-publish")]
        "s3" => {
            // S3 storage supports both AWS S3 and S3-compatible services
            // When endpoint is set, it points to an S3-compatible service
            info!(
                "Creating S3 blob storage: bucket='{}', region='{}', endpoint='{}', base_url='{}'",
                config.bucket,
                config.region,
                config.endpoint.as_deref().unwrap_or("<none>"),
                config.base_url.as_deref().unwrap_or("<none>"),
            );

            // Note: access_key / secret_key are intentionally NOT logged for security.
            let storage = S3BlobStorage::new(
                config.bucket.clone(),
                config.region.clone(),
                config.endpoint.clone(),
                config.access_key.clone(),
                config.secret_key.clone(),
                config.base_url.clone(),
            )
            .await?;
            Ok(Arc::new(storage))
        }
        #[cfg(not(feature = "registry-publish"))]
        "s3" => {
            Err(ServiceError::Custom(
                "S3 storage requires the 'registry-publish' feature to be enabled".to_string(),
            ))
        }
        _ => Err(ServiceError::Custom(format!(
            "Unsupported storage type: {}",
            storage_type
        ))),
    }
}

/// Blob storage configuration
#[derive(Debug, Clone)]
pub struct BlobStorageConfig {
    pub storage_type: String,
    pub base_path: String,
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
    pub base_url: Option<String>,
}

impl Default for BlobStorageConfig {
    fn default() -> Self {
        Self {
            storage_type: "local".to_string(),
            base_path: "./artifacts".to_string(),
            bucket: String::new(),
            region: String::new(),
            endpoint: None,
            access_key: String::new(),
            secret_key: String::new(),
            base_url: None,
        }
    }
}

//! HTTP client utilities for API calls

use crate::cli::error::{CliError, CliResult};
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// API client for FastSkill registry
pub struct ApiClient {
    base_url: String,
    token: Option<String>,
    client: Client,
}

/// Publish response from API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PublishApiResponse {
    pub job_id: String,
    #[allow(dead_code)]
    pub status: String,
    pub skill_id: String,
    pub version: String,
    #[allow(dead_code)]
    pub message: String,
}

/// Publish status response from API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PublishStatusApiResponse {
    #[allow(dead_code)]
    pub job_id: String,
    pub status: String,
    pub skill_id: String,
    pub version: String,
    #[allow(dead_code)]
    pub checksum: String,
    #[allow(dead_code)]
    pub uploaded_at: String,
    #[allow(dead_code)]
    pub uploaded_by: Option<String>,
    pub validation_errors: Vec<String>,
    #[allow(dead_code)]
    pub message: Option<String>,
}

/// Generic API response wrapper
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<serde_json::Value>,
}

impl ApiClient {
    /// Create a new API client
    pub fn new(base_url: &str, token: Option<String>) -> CliResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(300)) // 5 minute timeout for large uploads
            .build()
            .map_err(|e| CliError::Validation(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            client,
        })
    }

    /// Publish a package to the registry API
    /// The server will extract the scope from the JWT token's user account
    pub async fn publish_package(&self, package_path: &Path) -> CliResult<PublishApiResponse> {
        let url = format!("{}/api/registry/publish", self.base_url);

        // Read package file
        let package_data = std::fs::read(package_path).map_err(CliError::Io)?;

        // Get filename
        let filename = package_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| CliError::Validation("Invalid package filename".to_string()))?;

        // Create multipart form (server will extract scope from JWT token)
        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(package_data).file_name(filename.to_string()),
        );

        // Build request
        let mut request = self.client.post(&url).multipart(form);

        // Add authentication
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| CliError::Validation(format!("Failed to send request: {}", e)))?;

        // Check status
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CliError::Validation(format!(
                "Publish failed with status {}: {}",
                status, error_text
            )));
        }

        // Parse response
        let api_response: ApiResponse<PublishApiResponse> = response
            .json()
            .await
            .map_err(|e| CliError::Validation(format!("Failed to parse response: {}", e)))?;

        if !api_response.success {
            return Err(CliError::Validation(format!(
                "Publish failed: {:?}",
                api_response.error
            )));
        }

        api_response
            .data
            .ok_or_else(|| CliError::Validation("No data in response".to_string()))
    }

    /// Get publish job status
    pub async fn get_publish_status(&self, job_id: &str) -> CliResult<PublishStatusApiResponse> {
        let url = format!("{}/api/registry/publish/status/{}", self.base_url, job_id);

        // Build request
        let mut request = self.client.get(&url);

        // Add authentication
        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| CliError::Validation(format!("Failed to send request: {}", e)))?;

        // Check status
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(CliError::Validation(format!(
                "Status check failed with status {}: {}",
                status, error_text
            )));
        }

        // Parse response
        let api_response: ApiResponse<PublishStatusApiResponse> = response
            .json()
            .await
            .map_err(|e| CliError::Validation(format!("Failed to parse response: {}", e)))?;

        if !api_response.success {
            return Err(CliError::Validation(format!(
                "Status check failed: {:?}",
                api_response.error
            )));
        }

        api_response
            .data
            .ok_or_else(|| CliError::Validation("No data in response".to_string()))
    }

    /// Poll for publish status until accepted or rejected
    pub async fn wait_for_completion(
        &self,
        job_id: &str,
        max_wait_seconds: u64,
    ) -> CliResult<PublishStatusApiResponse> {
        use std::time::{Duration, Instant};
        use tokio::time::sleep;

        let start = Instant::now();
        let poll_interval = Duration::from_secs(2);

        loop {
            let status = self.get_publish_status(job_id).await?;

            match status.status.as_str() {
                "accepted" => return Ok(status),
                "rejected" => {
                    return Err(CliError::Validation(format!(
                        "Package was rejected: {:?}",
                        status.validation_errors
                    )));
                }
                _ => {
                    // Still pending or validating
                    if start.elapsed().as_secs() > max_wait_seconds {
                        // Return current status instead of error, but this indicates timeout
                        // The caller should handle this appropriately
                        return Ok(status);
                    }
                    sleep(poll_interval).await;
                }
            }
        }
    }
}

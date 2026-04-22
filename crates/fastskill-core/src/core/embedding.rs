//! Embedding service for generating vector representations of text

use crate::core::service::ServiceError;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Response from OpenAI embeddings API
#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingData {
    embedding: Vec<f32>,
}

/// Embedding service trait
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    /// Generate embeddings for text content
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, ServiceError>;

    /// Generate embeddings for a search query
    async fn embed_query(&self, query: &str) -> Result<Vec<f32>, ServiceError>;
}

/// OpenAI embedding service implementation
pub struct OpenAIEmbeddingService {
    client: Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl OpenAIEmbeddingService {
    /// Create a new OpenAI embedding service
    pub fn new(base_url: String, model: String, api_key: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            model,
            api_key,
        }
    }

    /// Create from embedding config and API key
    pub fn from_config(config: &crate::core::service::EmbeddingConfig, api_key: String) -> Self {
        Self::new(
            config.openai_base_url.clone(),
            config.embedding_model.clone(),
            api_key,
        )
    }

    /// Make the actual API call to OpenAI
    async fn call_openai_api(&self, text: &str) -> Result<Vec<f32>, ServiceError> {
        #[derive(Serialize)]
        struct OpenAIRequest {
            input: String,
            model: String,
        }

        let request = OpenAIRequest {
            input: text.to_string(),
            model: self.model.clone(),
        };

        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| ServiceError::Custom(format!("OpenAI API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ServiceError::Custom(format!(
                "OpenAI API error {}: {}",
                status, body
            )));
        }

        let embedding_response: OpenAIEmbeddingResponse = response
            .json()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to parse OpenAI response: {}", e)))?;

        if embedding_response.data.is_empty() {
            return Err(ServiceError::Custom(
                "No embeddings returned from OpenAI".to_string(),
            ));
        }

        Ok(embedding_response.data[0].embedding.clone())
    }
}

#[async_trait]
impl EmbeddingService for OpenAIEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, ServiceError> {
        // For text content, we might want to limit length or preprocess
        // For now, just call the API directly
        self.call_openai_api(text).await
    }

    async fn embed_query(&self, query: &str) -> Result<Vec<f32>, ServiceError> {
        // Queries are typically shorter, so we can pass them through directly
        self.call_openai_api(query).await
    }
}

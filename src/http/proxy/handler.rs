//! Proxy request handler for intercepting and forwarding API calls

use crate::http::proxy::{config::ProxyConfig, skill_injection::SkillInjector};
use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Proxy request handler
#[derive(Clone)]
pub struct ProxyHandler {
    config: ProxyConfig,
    #[allow(dead_code)] // Reserved for future functionality
    service: Arc<dyn super::ProxyService>,
    injector: SkillInjector,
    client: reqwest::Client,
}

impl ProxyHandler {
    /// Create a new proxy handler
    pub fn new(
        config: ProxyConfig,
        service: Arc<dyn super::ProxyService>,
    ) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
            .build()?;

        let injector = SkillInjector::new(
            service.clone(),
            config.skills_dir.clone(),
            config.hardcoded_skills.clone(),
        );

        Ok(Self {
            config,
            service,
            injector,
            client,
        })
    }

    /// Handle an incoming request
    pub async fn handle_request(&self, request: Request) -> Response {
        // Only intercept POST requests to /v1/messages
        if request.method() != Method::POST {
            return self.forward_request(request).await;
        }

        let path = request.uri().path();
        if !path.ends_with("/v1/messages") {
            return self.forward_request(request).await;
        }

        info!("Intercepting Anthropic API call: {}", path);

        // Read and parse request body
        match self.process_request_body(request).await {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to process request: {}", e);
                self.create_error_response(StatusCode::BAD_REQUEST, &e.to_string())
            }
        }
    }

    /// Process the request body and inject skills
    async fn process_request_body(
        &self,
        request: Request,
    ) -> Result<Response, Box<dyn std::error::Error>> {
        // Extract headers before consuming the body
        let headers = request.headers().clone();

        // Read request body
        let body_bytes = axum::body::to_bytes(request.into_body(), usize::MAX).await?;
        let mut body_json: Value = serde_json::from_slice(&body_bytes)?;

        // Log request if enabled
        if self.config.enable_logging {
            debug!(
                "Original request body: {}",
                serde_json::to_string_pretty(&body_json)?
            );
        }

        // Check if this is a beta.messages.create call
        if let Some(model) = body_json.get("model") {
            if let Some(model_str) = model.as_str() {
                if model_str.contains("claude") {
                    // Inject hardcoded skills first (if configured)
                    self.injector
                        .inject_hardcoded_skills(&mut body_json)
                        .await?;

                    // Process existing skills in container (if any)
                    self.injector.process_request(&mut body_json).await?;

                    // Enable dynamic injection if configured (hybrid mode)
                    if self.config.enable_dynamic_injection {
                        self.injector
                            .enable_dynamic_injection(
                                &mut body_json,
                                self.config.max_dynamic_skills,
                            )
                            .await?;
                    }
                }
            }
        }

        // Log modified request if enabled
        if self.config.enable_logging {
            debug!(
                "Modified request body: {}",
                serde_json::to_string_pretty(&body_json)?
            );
        }

        // Forward the modified request
        self.forward_modified_request(body_json, &headers).await
    }

    /// Forward a modified request to Anthropic API
    async fn forward_modified_request(
        &self,
        body_json: Value,
        headers: &HeaderMap,
    ) -> Result<Response, Box<dyn std::error::Error>> {
        let target_url = format!(
            "{}/v1/messages",
            self.config.target_url.trim_end_matches('/')
        );

        // Build request to Anthropic API
        let mut request_builder = self.client.post(&target_url);

        // Copy relevant headers
        for (key, value) in headers.iter() {
            if key == "x-api-key"
                || key == "anthropic-version"
                || key.as_str().starts_with("anthropic-beta")
                || key == "content-type"
            {
                if let Ok(value_str) = value.to_str() {
                    request_builder = request_builder.header(key.as_str(), value_str);
                }
            }
        }

        // Set body
        let response = request_builder.json(&body_json).send().await?;

        // Convert response back to Axum response
        let status = StatusCode::from_u16(response.status().as_u16())?;
        let headers = response.headers().clone();
        let body = response.bytes().await?;

        let mut axum_response = Response::builder().status(status);

        // Copy response headers
        for (key, value) in headers.iter() {
            axum_response = axum_response.header(key.as_str(), value.as_bytes());
        }

        let response = axum_response.body(Body::from(body))?;

        Ok(response)
    }

    /// Forward a request without modification
    async fn forward_request(&self, request: Request) -> Response {
        // For non-intercepted requests, return a 404 or forward as needed
        // For now, we'll just return a not found response
        warn!(
            "Request not intercepted: {} {}",
            request.method(),
            request.uri()
        );
        StatusCode::NOT_FOUND.into_response()
    }

    /// Create an error response with JSON body
    fn create_error_response(&self, status: StatusCode, message: &str) -> Response {
        let error_body = serde_json::json!({
            "type": "error",
            "error": {
                "type": if status.is_client_error() { "invalid_request_error" } else { "internal_error" },
                "message": message
            }
        });

        Response::builder()
            .status(status)
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&error_body)
                    .unwrap_or_else(|_| format!(r#"{{"error": "{}"}}"#, message)),
            ))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }
}

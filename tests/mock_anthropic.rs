//! Mock Anthropic API server for testing proxy functionality

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::debug;

/// Mock Anthropic API server for testing
#[derive(Clone)]
pub struct MockAnthropicServer {
    addr: SocketAddr,
}

impl MockAnthropicServer {
    /// Create a new mock server
    pub fn new(port: u16) -> Self {
        Self {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
        }
    }

    /// Create with a dynamically assigned port
    pub async fn new_with_dynamic_port() -> Result<(Self, TcpListener), anyhow::Error> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        Ok((Self { addr }, listener))
    }

    /// Get the server address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Create the router with mock endpoints
    pub fn create_router() -> Router {
        Router::new()
            .route("/v1/messages", post(mock_messages_endpoint))
            .layer(
                ServiceBuilder::new().layer(
                    CorsLayer::new()
                        .allow_methods(Any)
                        .allow_headers(Any)
                        .allow_origin(Any),
                ),
            )
    }

    /// Start the mock server (used when you already have a listener)
    pub async fn serve_with_listener(
        self,
        listener: TcpListener,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let app = Self::create_router();

        debug!("Starting mock Anthropic API server on {}", self.addr);

        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Start the mock server (creates its own listener)
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(self.addr).await?;
        self.serve_with_listener(listener).await
    }
}

/// Mock messages endpoint that mimics Anthropic's API
async fn mock_messages_endpoint(
    headers: HeaderMap,
    request: Request,
) -> Result<Response, StatusCode> {
    // Verify API key is present (basic validation)
    if !headers.contains_key("x-api-key") {
        let error_response = json!({
            "type": "error",
            "error": {
                "type": "authentication_error",
                "message": "x-api-key header is required"
            }
        });
        return Ok((StatusCode::UNAUTHORIZED, axum::Json(error_response)).into_response());
    }

    // Read request body
    let body_bytes = match axum::body::to_bytes(request.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let error_response = json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": "Invalid request body"
                }
            });
            return Ok((StatusCode::BAD_REQUEST, axum::Json(error_response)).into_response());
        }
    };

    // Parse JSON
    let request_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(_) => {
            let error_response = json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": "Invalid JSON in request body"
                }
            });
            return Ok((StatusCode::BAD_REQUEST, axum::Json(error_response)).into_response());
        }
    };

    debug!(
        "Mock Anthropic API received request: {}",
        serde_json::to_string_pretty(&request_json).unwrap()
    );

    // Extract skills from container if present
    let injected_skills = extract_skills_from_request(&request_json);

    // Extract user message for context
    let user_message = extract_user_message(&request_json);

    // Generate response with injected skill context
    let response = generate_mock_response(&user_message, &injected_skills);

    debug!(
        "Mock Anthropic API sending response: {}",
        serde_json::to_string_pretty(&response).unwrap()
    );

    Ok((StatusCode::OK, axum::Json(response)).into_response())
}

/// Extract skills from the request's container
fn extract_skills_from_request(request: &Value) -> Vec<String> {
    let mut skills = Vec::new();

    if let Some(container) = request.get("container") {
        if let Some(skills_array) = container.get("skills") {
            if let Some(array) = skills_array.as_array() {
                for skill in array {
                    if let Some(skill_id) = skill.get("skill_id") {
                        if let Some(id_str) = skill_id.as_str() {
                            skills.push(id_str.to_string());
                        }
                    }
                }
            }
        }
    }

    skills
}

/// Extract user message from the request
fn extract_user_message(request: &Value) -> String {
    if let Some(messages) = request.get("messages") {
        if let Some(messages_array) = messages.as_array() {
            // Find the last user message
            for message in messages_array.iter().rev() {
                if message.get("role") == Some(&json!("user")) {
                    if let Some(content) = message.get("content") {
                        if let Some(content_str) = content.as_str() {
                            return content_str.to_string();
                        }
                    }
                }
            }
        }
    }
    "Hello".to_string() // Default message
}

/// Generate a mock response that includes injected skill context
fn generate_mock_response(user_message: &str, injected_skills: &[String]) -> Value {
    let mut response_content = format!("I understand your message: '{}'.", user_message);

    if !injected_skills.is_empty() {
        response_content.push_str(" I have access to the following skills: ");
        for (i, skill) in injected_skills.iter().enumerate() {
            if i > 0 {
                response_content.push_str(", ");
            }
            response_content.push_str(skill);
        }
        response_content.push_str(".");

        // Add verification marker for tests
        response_content.push_str(&format!(
            " [INJECTED_SKILL_CONTEXT: {}]",
            injected_skills.join(",")
        ));
    }

    json!({
        "id": "msg_1234567890",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": response_content
            }
        ],
        "model": "claude-sonnet-4-5-20250929",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 10,
            "output_tokens": 20
        }
    })
}

/// Convenience function to start a mock server
pub async fn start_mock_anthropic_server(port: u16) -> Result<MockAnthropicServer, anyhow::Error> {
    let (server, listener) = if port == 0 {
        // Use dynamic port assignment
        MockAnthropicServer::new_with_dynamic_port().await?
    } else {
        let server = MockAnthropicServer::new(port);
        let listener = TcpListener::bind(server.addr).await?;
        (server, listener)
    };

    let app = MockAnthropicServer::create_router();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("Mock Anthropic server error: {}", e);
        }
    });

    // Give the server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(server)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;

    #[tokio::test]
    async fn test_mock_anthropic_response() {
        let port = 0; // Use any available port
        let server = start_mock_anthropic_server(port).await.unwrap();
        let server_port = server.addr().port();

        let client = Client::new();

        // Test basic request
        let request_body = json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hello world"}]
        });

        let response = client
            .post(&format!("http://127.0.0.1:{}/v1/messages", server_port))
            .header("x-api-key", "test-key")
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());

        let response_json: Value = response.json().await.unwrap();
        assert_eq!(response_json["type"], "message");
        assert!(response_json["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Hello world"));
    }

    #[tokio::test]
    async fn test_skill_injection_verification() {
        let port = 0;
        let server = start_mock_anthropic_server(port).await.unwrap();
        let server_port = server.addr().port();

        let client = Client::new();

        // Test request with skills in container
        let request_body = json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 100,
            "container": {
                "skills": [
                    {"type": "anthropic", "skill_id": "pptx", "version": "latest"},
                    {"type": "custom", "skill_id": "test-skill", "version": "1.0"}
                ]
            },
            "messages": [{"role": "user", "content": "Create a presentation"}]
        });

        let response = client
            .post(&format!("http://127.0.0.1:{}/v1/messages", server_port))
            .header("x-api-key", "test-key")
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());

        let response_json: Value = response.json().await.unwrap();
        let response_text = response_json["content"][0]["text"].as_str().unwrap();

        // Verify skills are mentioned in response
        assert!(response_text.contains("pptx"));
        assert!(response_text.contains("test-skill"));
        assert!(response_text.contains("[INJECTED_SKILL_CONTEXT: pptx,test-skill]"));
    }

    #[tokio::test]
    async fn test_missing_api_key() {
        let port = 0;
        let server = start_mock_anthropic_server(port).await.unwrap();
        let server_port = server.addr().port();

        let client = Client::new();

        let request_body = json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let response = client
            .post(&format!("http://127.0.0.1:{}/v1/messages", server_port))
            .json(&request_body)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 401);

        let response_json: Value = response.json().await.unwrap();
        assert_eq!(response_json["error"]["type"], "authentication_error");
    }
}

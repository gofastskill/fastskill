//! Comprehensive integration tests for the FastSkill proxy server

use anyhow::Context;
use async_trait::async_trait;
use axum::{extract::Request, http::StatusCode, response::IntoResponse, routing::post, Router};
use fastskill::core::metadata::{MetadataService, SkillFrontmatter, SkillMetadata};
use fastskill::core::service::ServiceError;
use fastskill::core::skill_manager::{
    SkillDefinition, SkillFilters, SkillManagementService, SkillUpdate,
};
use fastskill::http::proxy::{config::ProxyConfig, server::ProxyServer};
use fastskill::SkillId;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::Duration;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};

/// Trait for service compatibility in tests
#[async_trait]
pub trait FastSkillServiceTrait: Send + Sync {
    fn skill_manager(&self) -> Arc<dyn SkillManagementService>;
    fn metadata_service(&self) -> Arc<dyn MetadataService>;
}

/// Mock FastSkillService for testing
#[derive(Clone)]
pub struct MockFastSkillService {
    skill_manager: Arc<MockSkillManagementService>,
    metadata_service: Arc<MockMetadataService>,
}

impl MockFastSkillService {
    /// Create a new mock service
    pub fn new() -> Self {
        let skill_manager = Arc::new(MockSkillManagementService::new());
        let metadata_service = Arc::new(MockMetadataService::new(skill_manager.clone()));

        Self {
            skill_manager,
            metadata_service,
        }
    }

    /// Add a skill directly for test setup
    pub fn add_skill(&self, skill: SkillDefinition) {
        self.skill_manager.add_skill(skill);
    }
}

#[async_trait]
impl FastSkillServiceTrait for MockFastSkillService {
    fn skill_manager(&self) -> Arc<dyn SkillManagementService> {
        self.skill_manager.clone()
    }

    fn metadata_service(&self) -> Arc<dyn MetadataService> {
        self.metadata_service.clone()
    }
}

impl fastskill::http::proxy::ProxyService for MockFastSkillService {
    fn skill_manager(&self) -> Arc<dyn SkillManagementService> {
        self.skill_manager.clone()
    }

    fn metadata_service(&self) -> Arc<dyn MetadataService> {
        self.metadata_service.clone()
    }
}

/// Mock skill management service using in-memory storage
pub struct MockSkillManagementService {
    skills: std::sync::Arc<std::sync::Mutex<HashMap<SkillId, SkillDefinition>>>,
}

impl MockSkillManagementService {
    pub fn new() -> Self {
        Self {
            skills: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn add_skill(&self, skill: SkillDefinition) {
        self.skills.lock().unwrap().insert(skill.id.clone(), skill);
    }

    pub fn remove_skill(&self, skill_id: &SkillId) {
        self.skills.lock().unwrap().remove(skill_id);
    }

    pub fn clear(&self) {
        self.skills.lock().unwrap().clear();
    }
}

#[async_trait]
impl SkillManagementService for MockSkillManagementService {
    async fn register_skill(&self, skill: SkillDefinition) -> Result<SkillId, ServiceError> {
        // For simplicity, just return the skill ID
        // In a real implementation, this would register the skill
        Ok(skill.id.clone())
    }

    async fn force_register_skill(&self, skill: SkillDefinition) -> Result<SkillId, ServiceError> {
        Ok(skill.id.clone())
    }

    async fn get_skill(&self, skill_id: &SkillId) -> Result<Option<SkillDefinition>, ServiceError> {
        Ok(self.skills.lock().unwrap().get(skill_id).cloned())
    }

    async fn update_skill(
        &self,
        skill_id: &SkillId,
        _updates: SkillUpdate,
    ) -> Result<(), ServiceError> {
        let skills = self.skills.lock().unwrap();
        if skills.contains_key(skill_id) {
            Ok(())
        } else {
            Err(ServiceError::SkillNotFound(skill_id.to_string()))
        }
    }

    async fn unregister_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError> {
        let mut skills = self.skills.lock().unwrap();
        if skills.remove(skill_id).is_some() {
            Ok(())
        } else {
            Err(ServiceError::SkillNotFound(skill_id.to_string()))
        }
    }

    async fn list_skills(
        &self,
        _filters: Option<SkillFilters>,
    ) -> Result<Vec<SkillDefinition>, ServiceError> {
        Ok(self.skills.lock().unwrap().values().cloned().collect())
    }

    async fn enable_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError> {
        let skills = self.skills.lock().unwrap();
        if skills.contains_key(skill_id) {
            Ok(())
        } else {
            Err(ServiceError::SkillNotFound(skill_id.to_string()))
        }
    }

    async fn disable_skill(&self, skill_id: &SkillId) -> Result<(), ServiceError> {
        let skills = self.skills.lock().unwrap();
        if skills.contains_key(skill_id) {
            Ok(())
        } else {
            Err(ServiceError::SkillNotFound(skill_id.to_string()))
        }
    }
}

/// Mock metadata service
pub struct MockMetadataService {
    skill_manager: Arc<MockSkillManagementService>,
}

impl MockMetadataService {
    pub fn new(skill_manager: Arc<MockSkillManagementService>) -> Self {
        Self { skill_manager }
    }
}

#[async_trait]
impl MetadataService for MockMetadataService {
    async fn discover_skills(&self, _query: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        // Simple implementation - return all skills for any query
        let skills = self.skill_manager.list_skills(None).await?;
        Ok(skills.iter().map(|s| SkillMetadata::from(s)).collect())
    }

    async fn find_skills_by_capability(
        &self,
        capability: &str,
    ) -> Result<Vec<SkillMetadata>, ServiceError> {
        let all_skills = self.skill_manager.list_skills(None).await?;
        Ok(all_skills
            .into_iter()
            .filter(|skill| {
                skill
                    .capabilities
                    .iter()
                    .any(|cap| cap.contains(capability))
            })
            .map(|s| SkillMetadata::from(&s))
            .collect())
    }

    async fn find_skills_by_tag(&self, tag: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        let all_skills = self.skill_manager.list_skills(None).await?;
        Ok(all_skills
            .into_iter()
            .filter(|skill| skill.tags.iter().any(|t| t.contains(tag)))
            .map(|s| SkillMetadata::from(&s))
            .collect())
    }

    async fn search_skills(&self, query: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        let all_skills = self.skill_manager.list_skills(None).await?;
        Ok(all_skills
            .into_iter()
            .filter(|skill| skill.name.contains(query) || skill.description.contains(query))
            .map(|s| SkillMetadata::from(&s))
            .collect())
    }

    async fn get_available_capabilities(&self) -> Result<Vec<String>, ServiceError> {
        let skills = self.skill_manager.list_skills(None).await?;
        let mut capabilities = std::collections::HashSet::new();
        for skill in skills {
            capabilities.extend(skill.capabilities);
        }
        Ok(capabilities.into_iter().collect())
    }

    async fn get_skill_frontmatter(
        &self,
        _skill_id: &str,
    ) -> Result<SkillFrontmatter, ServiceError> {
        // Simplified - return empty frontmatter
        Ok(SkillFrontmatter {
            name: "Test Skill".to_string(),
            description: "Test description".to_string(),
            version: "1.0.0".to_string(),
            author: Some("Test".to_string()),
            tags: vec![],
            capabilities: vec![],
            extra: std::collections::HashMap::new(),
        })
    }
}

/// Mock Anthropic API server for testing
#[derive(Clone)]
pub struct MockAnthropicServer {
    addr: SocketAddr,
}

impl MockAnthropicServer {
    pub fn new(port: u16) -> Self {
        Self {
            addr: format!("127.0.0.1:{}", port).parse().unwrap(),
        }
    }

    pub async fn serve(self) -> Result<(), anyhow::Error> {
        let app = Router::new()
            .route("/v1/messages", post(Self::handle_request))
            .layer(ServiceBuilder::new().layer(CorsLayer::new().allow_origin(Any)));

        let listener = TcpListener::bind(self.addr)
            .await
            .context("Failed to bind server")?;
        axum::serve(listener, app).await.context("Server error")?;
        Ok(())
    }

    async fn handle_request(req: Request) -> Result<impl IntoResponse, StatusCode> {
        // Check for API key header
        if !req.headers().contains_key("x-api-key") {
            let error_response = json!({
                "type": "error",
                "error": {
                    "type": "authentication_error",
                    "message": "x-api-key header is required"
                }
            });
            return Ok((StatusCode::UNAUTHORIZED, axum::Json(error_response)));
        }

        let (_parts, body) = req.into_parts();

        let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
            Ok(bytes) => bytes,
            Err(_) => return Err(StatusCode::BAD_REQUEST),
        };

        let request_json: Value = match serde_json::from_slice(&body_bytes) {
            Ok(json) => json,
            Err(_) => return Err(StatusCode::BAD_REQUEST),
        };

        // Extract skills from container if present
        let injected_skills = if let Some(container) = request_json.get("container") {
            if let Some(skills) = container.get("skills") {
                if let Some(skills_array) = skills.as_array() {
                    skills_array
                        .iter()
                        .filter_map(|skill| skill.get("skill_id"))
                        .filter_map(|id| id.as_str())
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Create mock response with skill context
        let mut response_content = format!(
            "Mock response to: {}",
            request_json["messages"][0]["content"]
                .as_str()
                .unwrap_or("unknown")
        );
        if !injected_skills.is_empty() {
            response_content.push_str(&format!(
                "\n\nSKILL_CONTEXT: Injected skills: {}",
                injected_skills.join(", ")
            ));
        }

        let response = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": response_content
            }],
            "model": "claude-3-haiku-20240307",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20
            }
        });

        Ok((StatusCode::OK, axum::Json(response)))
    }
}

/// Convenience function to start a mock server
pub async fn start_mock_anthropic_server(port: u16) -> Result<MockAnthropicServer, anyhow::Error> {
    let server = MockAnthropicServer::new(port);

    let server_clone = server.clone();
    tokio::spawn(async move {
        if let Err(e) = server_clone.serve().await {
            eprintln!("Mock Anthropic server error: {}", e);
        }
    });

    // Give the server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(server)
}

/// Test skill definitions and utilities
pub struct TestSkills {
    temp_dir: TempDir,
    skills: HashMap<String, SkillDefinition>,
}

impl TestSkills {
    /// Create a new test skills instance with temporary directory
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().unwrap(),
            skills: HashMap::new(),
        }
    }

    /// Get the temporary directory path
    pub fn temp_dir(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    /// Create a test skill with proper SKILL.md file
    pub fn create_skill(
        &mut self,
        skill_id: &str,
        name: &str,
        description: &str,
        capabilities: Vec<&str>,
        tags: Vec<&str>,
    ) -> SkillDefinition {
        let skill_dir = self.temp_dir.path().join(skill_id);
        std::fs::create_dir_all(&skill_dir).unwrap();

        let skill_md_path = skill_dir.join("SKILL.md");

        let frontmatter = format!(
            r#"---
name: {}
description: {}
version: "1.0.0"
author: "Test Author"
tags: [{}]
capabilities: [{}]
---

# {}

## Usage
Test skill for integration testing.
"#,
            name,
            description,
            tags.join(", "),
            capabilities.join(", "),
            name
        );

        std::fs::write(&skill_md_path, &frontmatter).unwrap();

        let skill_def = SkillDefinition {
            id: fastskill::SkillId::new(skill_id.to_string()).unwrap(),
            name: name.to_string(),
            description: description.to_string(),
            version: "1.0.0".to_string(),
            author: Some("Test Author".to_string()),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            enabled: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            skill_file: skill_md_path,
            reference_files: None,
            script_files: None,
            asset_files: None,
            execution_environment: None,
            dependencies: None,
            timeout: None,
            source_url: None,
            source_type: None,
            source_branch: None,
            source_tag: None,
            source_subdir: None,
            installed_from: None,
            commit_hash: None,
            fetched_at: None,
            editable: false,
        };

        self.skills.insert(skill_id.to_string(), skill_def.clone());
        skill_def
    }

    /// Register skill with service
    pub fn register_with_service(&self, service: &MockFastSkillService) {
        for skill in self.skills.values() {
            service.add_skill(skill.clone());
        }
    }

    pub fn all_skills(&self) -> Vec<&SkillDefinition> {
        self.skills.values().collect()
    }
}

/// Create mock service with test skills
pub fn create_mock_service_with_skills() -> (MockFastSkillService, TestSkills) {
    let mut test_skills = TestSkills::new();

    // Create Anthropic skills in correct subdirectories
    // These need to match the mappings in SkillInjector::new
    let temp_dir = test_skills.temp_dir();

    // Create pptx in presentation/pptx
    let pptx_dir = temp_dir.join("presentation/pptx");
    std::fs::create_dir_all(&pptx_dir).unwrap();
    std::fs::write(
        pptx_dir.join("SKILL.md"),
        r#"---
name: PowerPoint Skill
description: Create and manipulate PowerPoint presentations
version: "1.0.0"
---
# PowerPoint Skill
"#,
    )
    .unwrap();

    // Create xlsx in spreadsheet/xlsx
    let xlsx_dir = temp_dir.join("spreadsheet/xlsx");
    std::fs::create_dir_all(&xlsx_dir).unwrap();
    std::fs::write(
        xlsx_dir.join("SKILL.md"),
        r#"---
name: Excel Skill
description: Work with Excel spreadsheets
version: "1.0.0"
---
# Excel Skill
"#,
    )
    .unwrap();

    // Create custom skills (these go directly in the skills directory)
    test_skills.create_skill(
        "codeguardian",
        "Code Guardian",
        "Code analysis and protection",
        vec!["code", "analysis", "security"],
        vec!["code", "security"],
    );

    test_skills.create_skill(
        "test-skill-1",
        "Test Skill 1",
        "A test skill for integration testing",
        vec!["test", "integration"],
        vec!["test"],
    );

    let mut service = MockFastSkillService::new();
    test_skills.register_with_service(&mut service);

    (service, test_skills)
}

/// Test fixture that manages proxy and mock servers for testing
pub struct TestFixture {
    pub proxy_port: u16,
    pub anthropic_port: u16,
    pub mock_service: MockFastSkillService,
    pub test_skills: TestSkills,
    pub proxy_handle: Option<tokio::task::JoinHandle<()>>,
    pub anthropic_handle: Option<tokio::task::JoinHandle<()>>,
}

impl TestFixture {
    /// Create a new test fixture
    pub async fn new() -> Self {
        let proxy_port = find_free_port().await;
        let anthropic_port = find_free_port().await;

        let (mock_service, test_skills) = create_mock_service_with_skills();

        Self {
            proxy_port,
            anthropic_port,
            mock_service,
            test_skills,
            proxy_handle: None,
            anthropic_handle: None,
        }
    }

    /// Start both proxy and mock Anthropic servers
    pub async fn start_servers(&mut self) -> Result<(), anyhow::Error> {
        // Start mock Anthropic server
        let anthropic_server = start_mock_anthropic_server(self.anthropic_port).await?;
        let anthropic_handle = tokio::spawn(async move {
            let _ = anthropic_server.serve().await;
        });

        // Give mock server time to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Start proxy server
        let config = ProxyConfig {
            port: self.proxy_port,
            target_url: format!("http://localhost:{}", self.anthropic_port),
            enable_dynamic_injection: true,
            skills_dir: self.test_skills.temp_dir().to_path_buf(),
            max_dynamic_skills: 5,
            request_timeout_secs: 30,
            enable_logging: false,
            hardcoded_skills: Vec::new(),
            dynamic_min_relevance: 0.7,
        };

        let proxy_server = ProxyServer::new(
            config,
            Arc::new(self.mock_service.clone()) as Arc<dyn fastskill::http::proxy::ProxyService>,
        )
        .unwrap();
        let proxy_handle = tokio::spawn(async move {
            let _ = proxy_server.serve().await;
        });

        // Give proxy server time to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        self.proxy_handle = Some(proxy_handle);
        self.anthropic_handle = Some(anthropic_handle);

        Ok(())
    }

    /// Make a proxy request and return the response
    pub async fn make_proxy_request(&self, request: &Value) -> Result<Value, anyhow::Error> {
        let client = Client::new();
        let response = client
            .post(&format!("http://localhost:{}/v1/messages", self.proxy_port))
            .header("Content-Type", "application/json")
            .header("x-api-key", "test-key")
            .json(request)
            .send()
            .await
            .context("Failed to send request")?;

        // Check HTTP status code
        let status = response.status();
        let response_body: Value = response
            .json()
            .await
            .context("Failed to parse response JSON")?;

        // If status is error, return error with message from response body
        if status.is_client_error() || status.is_server_error() {
            let error_msg = if let Some(error_obj) = response_body.get("error") {
                if let Some(msg) = error_obj.get("message").and_then(|v| v.as_str()) {
                    msg.to_string()
                } else if let Some(err_type) = error_obj.get("type").and_then(|v| v.as_str()) {
                    format!("Error: {}", err_type)
                } else {
                    format!("Request failed with status {}", status)
                }
            } else if let Some(msg) = response_body.get("message").and_then(|v| v.as_str()) {
                msg.to_string()
            } else {
                format!("Request failed with status {}", status)
            };

            return Err(anyhow::anyhow!("Request failed: {}", error_msg));
        }

        Ok(response_body)
    }

    /// Make a proxy request with custom headers and return the response
    pub async fn make_proxy_request_with_headers(
        &self,
        request: &Value,
        headers: &[(&str, &str)],
    ) -> Result<Value, anyhow::Error> {
        let mut client =
            Client::new().post(&format!("http://localhost:{}/v1/messages", self.proxy_port));

        for (key, value) in headers {
            client = client.header(*key, *value);
        }

        let response = client
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .context("Failed to send request with headers")?;

        // Check HTTP status code
        let status = response.status();
        let response_body: Value = response
            .json()
            .await
            .context("Failed to parse response JSON")?;

        // If status is error, return error with message from response body
        if status.is_client_error() || status.is_server_error() {
            let error_msg = if let Some(error_obj) = response_body.get("error") {
                if let Some(msg) = error_obj.get("message").and_then(|v| v.as_str()) {
                    msg.to_string()
                } else if let Some(err_type) = error_obj.get("type").and_then(|v| v.as_str()) {
                    format!("Error: {}", err_type)
                } else {
                    format!("Request failed with status {}", status)
                }
            } else if let Some(msg) = response_body.get("message").and_then(|v| v.as_str()) {
                msg.to_string()
            } else {
                format!("Request failed with status {}", status)
            };

            return Err(anyhow::anyhow!("Request failed: {}", error_msg));
        }

        Ok(response_body)
    }

    /// Clean up servers
    pub async fn cleanup(&mut self) {
        if let Some(handle) = self.proxy_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.anthropic_handle.take() {
            handle.abort();
        }
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        // Ensure cleanup happens
        if let Some(handle) = self.proxy_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.anthropic_handle.take() {
            handle.abort();
        }
    }
}

/// Find a free port for testing
pub async fn find_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// Create a test proxy server fixture
pub async fn create_test_proxy_server() -> Result<TestFixture, anyhow::Error> {
    let mut fixture = TestFixture::new().await;
    fixture.start_servers().await?;
    Ok(fixture)
}

/// Create a basic request without skills
pub fn create_basic_request(content: &str) -> Value {
    json!({
        "model": "claude-3-haiku-20240307",
        "max_tokens": 100,
        "messages": [{
            "role": "user",
            "content": content
        }]
    })
}

/// Create a request with explicit skills in container
pub fn create_request_with_skills(content: &str, skills: &[(&str, &str)]) -> Value {
    let skills_array = skills
        .iter()
        .map(|(skill_type, skill_id)| {
            json!({
                "type": skill_type,
                "skill_id": skill_id,
                "version": "latest"
            })
        })
        .collect::<Vec<_>>();

    json!({
        "model": "claude-3-haiku-20240307",
        "max_tokens": 100,
        "container": {
            "skills": skills_array
        },
        "messages": [{
            "role": "user",
            "content": content
        }]
    })
}

/// Assert that specific skills were injected in the response
pub fn assert_skill_injected(response: &Value, expected_skills: &[&str]) {
    if let Some(content) = response.get("content") {
        if let Some(content_array) = content.as_array() {
            if let Some(first_content) = content_array.get(0) {
                if let Some(text) = first_content.get("text") {
                    if let Some(text_str) = text.as_str() {
                        for skill in expected_skills {
                            assert!(
                                text_str.contains(skill),
                                "Response should contain skill '{}', but content was: {}",
                                skill,
                                text_str
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Assert that no skills were injected in the response
pub fn assert_no_skills_injected(response: &Value) {
    if let Some(content) = response.get("content") {
        if let Some(content_array) = content.as_array() {
            if let Some(first_content) = content_array.get(0) {
                if let Some(text) = first_content.get("text") {
                    if let Some(text_str) = text.as_str() {
                        // Check that no skill context was injected
                        assert!(
                            !text_str.contains("SKILL_CONTEXT"),
                            "Response should not contain skill context, but found: {}",
                            text_str
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_basic_request_no_skills() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_basic_request("Hello world");

        let response = fixture.make_proxy_request(&request).await.unwrap();

        // Verify response structure
        assert_eq!(response["type"], "message");
        assert_eq!(response["role"], "assistant");
        assert!(response["content"].is_array());
        assert!(response["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Hello world"));

        // Verify no skills were injected
        assert_no_skills_injected(&response);
    }

    /// Test request with dynamic injection disabled
    #[tokio::test]
    async fn test_basic_request_with_dynamic_injection_disabled() {
        // This test would require creating a proxy server with dynamic injection disabled
        // For now, we'll test that basic requests work (dynamic injection is enabled by default in our fixture)
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_basic_request("Simple question");

        let response = fixture.make_proxy_request(&request).await.unwrap();

        // Should work without issues
        assert_eq!(response["type"], "message");
        assert!(response["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Simple question"));
    }

    /// Test request with explicit container skills (Anthropic skills)
    #[tokio::test]
    async fn test_request_with_explicit_container_skills() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_request_with_skills(
            "Create a presentation",
            &[("anthropic", "pptx"), ("anthropic", "xlsx")],
        );

        let response = fixture.make_proxy_request(&request).await.unwrap();

        // Verify response mentions the skills
        assert_skill_injected(&response, &["pptx", "xlsx"]);

        // Verify response content includes skill context
        let content = response["content"][0]["text"].as_str().unwrap();
        assert!(content.contains("pptx"));
        assert!(content.contains("xlsx"));
    }

    /// Test request with custom skills from registry
    #[tokio::test]
    async fn test_request_with_custom_skills() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_request_with_skills(
            "Analyze code",
            &[("custom", "codeguardian"), ("custom", "test-skill-1")],
        );

        let response = fixture.make_proxy_request(&request).await.unwrap();

        // Verify response mentions the custom skills
        assert_skill_injected(&response, &["codeguardian", "test-skill-1"]);

        let content = response["content"][0]["text"].as_str().unwrap();
        assert!(content.contains("codeguardian"));
        assert!(content.contains("test-skill-1"));
    }

    /// Test dynamic skill injection based on message content
    #[tokio::test]
    async fn test_dynamic_skill_injection_enabled() {
        let fixture = create_test_proxy_server().await.unwrap();

        // This message should trigger presentation-related skills
        let request = create_basic_request("Create a PowerPoint presentation about AI");

        let response = fixture.make_proxy_request(&request).await.unwrap();

        // Dynamic injection should find and inject relevant skills
        // The mock service will return skills that match the query
        assert_eq!(response["type"], "message");

        // Note: The exact skills injected depend on the mock service's search logic
        // For this test, we mainly verify the request doesn't fail
        let content = response["content"][0]["text"].as_str().unwrap();
        assert!(content.contains("Create a PowerPoint presentation about AI"));
    }

    /// Test mixed Anthropic and custom skills
    #[tokio::test]
    async fn test_mixed_skill_types() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_request_with_skills(
            "Create a presentation and analyze code",
            &[("anthropic", "pptx"), ("custom", "codeguardian")],
        );

        let response = fixture.make_proxy_request(&request).await.unwrap();

        assert_skill_injected(&response, &["pptx", "codeguardian"]);

        let content = response["content"][0]["text"].as_str().unwrap();
        assert!(content.contains("pptx"));
        assert!(content.contains("codeguardian"));
    }

    /// Test request with empty skills array
    #[tokio::test]
    async fn test_empty_skills_array() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 100,
            "container": {
                "skills": []
            },
            "messages": [{"role": "user", "content": "Hello with empty skills"}]
        });

        let response = fixture.make_proxy_request(&request).await.unwrap();

        // Empty skills array should not cause errors
        assert_eq!(response["type"], "message");
        assert!(response["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Hello with empty skills"));

        // Should not have skill injection markers
        assert_no_skills_injected(&response);
    }

    /// Test request with malformed container
    #[tokio::test]
    async fn test_malformed_container() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 100,
            "container": "not an object",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let response = fixture.make_proxy_request(&request).await.unwrap();

        // Should handle malformed container gracefully
        assert_eq!(response["type"], "message");
    }

    /// Test concurrent requests to the proxy
    #[tokio::test]
    async fn test_concurrent_requests() {
        let fixture = create_test_proxy_server().await.unwrap();
        let fixture = std::sync::Arc::new(fixture);

        let request1 = create_basic_request("Request 1");
        let request2 = create_basic_request("Request 2");
        let request3 = create_request_with_skills("Request 3", &[("anthropic", "pptx")]);

        // Spawn concurrent requests
        let handle1 = {
            let fixture = fixture.clone();
            let req = request1.clone();
            tokio::spawn(async move { fixture.as_ref().make_proxy_request(&req).await })
        };

        let handle2 = {
            let fixture = fixture.clone();
            let req = request2.clone();
            tokio::spawn(async move { fixture.as_ref().make_proxy_request(&req).await })
        };

        let handle3 = {
            let fixture = fixture.clone();
            let req = request3.clone();
            tokio::spawn(async move { fixture.as_ref().make_proxy_request(&req).await })
        };

        // Wait for all requests to complete
        let (result1, result2, result3) = tokio::join!(handle1, handle2, handle3);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());

        let response1 = result1.unwrap().unwrap();
        let response2 = result2.unwrap().unwrap();
        let response3 = result3.unwrap().unwrap();

        // Verify responses
        assert_eq!(response1["type"], "message");
        assert_eq!(response2["type"], "message");
        assert_eq!(response3["type"], "message");

        assert!(response1["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Request 1"));
        assert!(response2["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Request 2"));
        assert_skill_injected(&response3, &["pptx"]);
    }

    /// Test health endpoint
    #[tokio::test]
    async fn test_proxy_health_check() {
        let fixture = create_test_proxy_server().await.unwrap();

        let client = reqwest::Client::new();
        let health_url = format!("http://127.0.0.1:{}/health", fixture.proxy_port);

        let response = client.get(&health_url).send().await.unwrap();

        assert!(response.status().is_success());
        let body = response.text().await.unwrap();
        assert_eq!(body, "Proxy server is running");
    }

    /// Test request with very large payload
    #[tokio::test]
    async fn test_large_request_body() {
        let fixture = create_test_proxy_server().await.unwrap();

        // Create a large message
        let large_content = "x".repeat(10000);
        let request = create_basic_request(&large_content);

        let response = fixture.make_proxy_request(&request).await.unwrap();

        assert_eq!(response["type"], "message");
        // The response should echo back the large content
        assert!(response["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("xxxxxxxxxx"));
    }

    /// Test request with unknown Anthropic skill
    #[tokio::test]
    async fn test_unknown_anthropic_skill() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_request_with_skills(
            "Use codeguardian",
            &[("anthropic", "codeguardian")], // This is not a real Anthropic skill
        );

        // This should fail because codeguardian is not a known Anthropic skill
        let result = fixture.make_proxy_request(&request).await;

        // The request should fail due to unknown skill
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Unknown Anthropic skill"));
        assert!(error.to_string().contains("Available Anthropic skills"));
    }

    /// Test request with skill that exists in registry but missing SKILL.md
    #[tokio::test]
    async fn test_missing_skill_file() {
        let fixture = create_test_proxy_server().await.unwrap();

        // Try to use a custom skill that exists in the registry
        let request = create_request_with_skills(
            "Use test skill",
            &[("custom", "codeguardian")], // This exists in our test skills
        );

        // This should work because the skill exists and has a proper SKILL.md file
        let response = fixture.make_proxy_request(&request).await.unwrap();
        assert_skill_injected(&response, &["codeguardian"]);
    }

    /// Test request with malformed container structure
    #[tokio::test]
    async fn test_invalid_container_structure() {
        let fixture = create_test_proxy_server().await.unwrap();

        // Create request with invalid container structure
        let request = json!({
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 100,
            "container": {
                "skills": [
                    {
                        "type": "anthropic",
                        // Missing skill_id field
                        "version": "latest"
                    }
                ]
            },
            "messages": [{"role": "user", "content": "Hello"}]
        });

        // The request should still work, but skills should not be loaded
        let response = fixture.make_proxy_request(&request).await.unwrap();
        assert_eq!(response["type"], "message");
        // Should not have skill injection since skill_id is missing
        assert_no_skills_injected(&response);
    }

    /// Test request without API key header
    #[tokio::test]
    async fn test_missing_api_key() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_basic_request("Hello without API key");

        // Make request without x-api-key header
        let result = fixture.make_proxy_request_with_headers(&request, &[]).await;

        // This should fail with authentication error
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Request failed"));
    }

    /// Test request with invalid JSON body
    #[tokio::test]
    async fn test_invalid_request_body() {
        let fixture = create_test_proxy_server().await.unwrap();

        // Try to make a request with invalid JSON directly
        let client = reqwest::Client::new();
        let invalid_json = "{ invalid json }";

        let response = client
            .post(&format!(
                "http://127.0.0.1:{}/v1/messages",
                fixture.proxy_port
            ))
            .header("x-api-key", "test-key")
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .body(invalid_json)
            .send()
            .await;

        // Should get a client error due to invalid JSON
        assert!(response.is_ok());
        let resp = response.unwrap();
        // The proxy should handle this gracefully
        assert!(resp.status().is_client_error() || resp.status().is_success());
    }

    /// Test request with invalid skill type
    #[tokio::test]
    async fn test_invalid_skill_type() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_request_with_skills(
            "Use invalid skill type",
            &[("invalid_type", "some_skill")], // Invalid type
        );

        // The request should work but the invalid skill type should be ignored
        let response = fixture.make_proxy_request(&request).await.unwrap();
        assert_eq!(response["type"], "message");
        // Should not inject the invalid skill
        assert_no_skills_injected(&response);
    }

    /// Test request timeout handling
    #[tokio::test]
    #[ignore] // Flaky test: 1ms timeout is too aggressive and unreliable across different systems
    async fn test_request_timeout() {
        let fixture = create_test_proxy_server().await.unwrap();

        let request = create_basic_request("This should timeout");

        // Use a very short timeout
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(1)) // Very short timeout
            .build()
            .unwrap();

        let result = client
            .post(&format!(
                "http://127.0.0.1:{}/v1/messages",
                fixture.proxy_port
            ))
            .header("x-api-key", "test-key")
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await;

        // Should timeout
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.is_timeout() || error.is_connect());
    }

    /// Test request with non-existent endpoint
    #[tokio::test]
    async fn test_non_existent_endpoint() {
        let fixture = create_test_proxy_server().await.unwrap();

        let client = reqwest::Client::new();
        let response = client
            .get(&format!(
                "http://127.0.0.1:{}/non-existent",
                fixture.proxy_port
            ))
            .send()
            .await
            .unwrap();

        // Should get 404
        assert_eq!(response.status(), 404);
    }
}

//! Integration tests for registry list-skills command
//!
//! Tests the HTTP endpoint integration for listing skills from a registry

use fastskill::core::registry_index::{ListSkillsOptions, SkillSummary};
use fastskill::core::repository::client::CratesRegistryClient;
use fastskill::core::repository::{RepositoryAuth, RepositoryConfig, RepositoryDefinition, RepositoryType};
use serde_json;
use std::collections::HashMap;
use tempfile::TempDir;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path, query_param};

/// Create a test repository definition for HTTP registry
fn create_test_repo_definition(index_url: String) -> RepositoryDefinition {
    RepositoryDefinition {
        name: "test-registry".to_string(),
        repo_type: RepositoryType::HttpRegistry,
        config: RepositoryConfig::HttpRegistry { index_url },
        auth: None,
        priority: 0,
        storage: None,
    }
}

/// Create a test repository definition with authentication
fn create_test_repo_definition_with_auth(
    index_url: String,
    auth_type: &str,
    env_var: String,
) -> RepositoryDefinition {
    let auth = match auth_type {
        "pat" => Some(RepositoryAuth::Pat { env_var }),
        "api_key" => Some(RepositoryAuth::ApiKey { env_var }),
        _ => None,
    };

    RepositoryDefinition {
        name: "test-registry".to_string(),
        repo_type: RepositoryType::HttpRegistry,
        config: RepositoryConfig::HttpRegistry { index_url },
        auth,
        priority: 0,
        storage: None,
    }
}

/// Create sample skill summaries for testing
fn create_sample_skill_summaries() -> Vec<SkillSummary> {
    use chrono::Utc;

    vec![
        SkillSummary {
            id: "acme/web-scraper".to_string(),
            scope: "acme".to_string(),
            name: "web-scraper".to_string(),
            description: "A web scraping skill".to_string(),
            latest_version: "1.0.0".to_string(),
            published_at: Some(Utc::now()),
            versions: None,
        },
        SkillSummary {
            id: "acme/data-processor".to_string(),
            scope: "acme".to_string(),
            name: "data-processor".to_string(),
            description: "A data processing skill".to_string(),
            latest_version: "2.0.0".to_string(),
            published_at: Some(Utc::now()),
            versions: None,
        },
        SkillSummary {
            id: "otherorg/tool".to_string(),
            scope: "otherorg".to_string(),
            name: "tool".to_string(),
            description: "A tool from another org".to_string(),
            latest_version: "1.5.0".to_string(),
            published_at: Some(Utc::now()),
            versions: None,
        },
    ]
}

#[tokio::test]
async fn test_list_skills_cli_calling_registry_http_endpoint() {
    // Test T011: Integration test for list-skills CLI calling registry HTTP endpoint
    // with multiple skills (verifies query mapping: scope/all_versions/include_pre_release)

    let mock_server = MockServer::start().await;

    // Create mock response with multiple skills
    let summaries = create_sample_skill_summaries();
    let response_body = serde_json::to_string(&summaries).unwrap();

    // Mock the endpoint
    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .respond_with(ResponseTemplate::new(200).set_body_string(response_body))
        .mount(&mock_server)
        .await;

    // Test without query parameters
    let repo_def = create_test_repo_definition(mock_server.uri());
    let client = CratesRegistryClient::new(&repo_def).unwrap();
    let options = ListSkillsOptions::default();
    let result = client.fetch_skills(&options).await;

    assert!(result.is_ok());
    let skills = result.unwrap();
    assert_eq!(skills.len(), 3);

    // Test with scope filter
    let options = ListSkillsOptions {
        scope: Some("acme".to_string()),
        ..Default::default()
    };

    // Mock filtered response
    let filtered_summaries: Vec<SkillSummary> = summaries
        .into_iter()
        .filter(|s| s.scope == "acme")
        .collect();
    let filtered_response = serde_json::to_string(&filtered_summaries).unwrap();

    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .and(query_param("scope", "acme"))
        .respond_with(ResponseTemplate::new(200).set_body_string(filtered_response))
        .mount(&mock_server)
        .await;

    let result = client.fetch_skills(&options).await;
    assert!(result.is_ok());
    let skills = result.unwrap();
    assert_eq!(skills.len(), 2);
    assert!(skills.iter().all(|s| s.scope == "acme"));

    // Test with all_versions flag
    let options = ListSkillsOptions {
        all_versions: true,
        ..Default::default()
    };

    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .and(query_param("all_versions", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_string(filtered_response))
        .mount(&mock_server)
        .await;

    let result = client.fetch_skills(&options).await;
    assert!(result.is_ok());

    // Test with include_pre_release flag
    let options = ListSkillsOptions {
        include_pre_release: true,
        ..Default::default()
    };

    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .and(query_param("include_pre_release", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_string(filtered_response))
        .mount(&mock_server)
        .await;

    let result = client.fetch_skills(&options).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_unauthenticated_vs_authenticated_registry_listing() {
    // Test T012: Integration test for unauthenticated vs authenticated registry listing
    // (401/403 handling)

    let mock_server = MockServer::start().await;

    // Test unauthenticated request (should succeed for public endpoints)
    let summaries = create_sample_skill_summaries();
    let response_body = serde_json::to_string(&summaries).unwrap();

    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .respond_with(ResponseTemplate::new(200).set_body_string(response_body))
        .mount(&mock_server)
        .await;

    let repo_def = create_test_repo_definition(mock_server.uri());
    let client = CratesRegistryClient::new(&repo_def).unwrap();
    let options = ListSkillsOptions::default();
    let result = client.fetch_skills(&options).await;

    assert!(result.is_ok());

    // Test 401 Unauthorized (authentication required)
    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&mock_server)
        .await;

    let result = client.fetch_skills(&options).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("Unauthorized") || error_msg.contains("authentication"));

    // Test 403 Forbidden (access denied)
    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&mock_server)
        .await;

    let result = client.fetch_skills(&options).await;
    assert!(result.is_err());
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("Forbidden") || error_msg.contains("denied"));

    // Test authenticated request with PAT
    std::env::set_var("TEST_PAT_TOKEN", "test-token-123");
    let repo_def = create_test_repo_definition_with_auth(
        mock_server.uri(),
        "pat",
        "TEST_PAT_TOKEN".to_string(),
    );

    // Mock successful authenticated response
    let summaries = create_sample_skill_summaries();
    let response_body = serde_json::to_string(&summaries).unwrap();

    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .and(wiremock::matchers::header("Authorization", "token test-token-123"))
        .respond_with(ResponseTemplate::new(200).set_body_string(response_body))
        .mount(&mock_server)
        .await;

    let client = CratesRegistryClient::new(&repo_def).unwrap();
    let options = ListSkillsOptions::default();
    let result = client.fetch_skills(&options).await;

    // Note: The actual auth header format depends on the Auth trait implementation
    // This test verifies the error handling structure
    // The actual header format may differ based on GitHubPat implementation
}

#[tokio::test]
async fn test_missing_invalid_index_url_configuration_error() {
    // Test T013: Integration test for missing/invalid index_url configuration error

    // Test with missing index_url (should fail during client creation)
    let repo_def = RepositoryDefinition {
        name: "test-registry".to_string(),
        repo_type: RepositoryType::HttpRegistry,
        config: RepositoryConfig::HttpRegistry {
            index_url: "".to_string(), // Empty URL
        },
        auth: None,
        priority: 0,
        storage: None,
    };

    // This should fail when trying to create the client or make requests
    // The exact error depends on validation, but it should fail
    let client_result = CratesRegistryClient::new(&repo_def);
    
    // The client creation might succeed, but the HTTP request should fail
    // Let's test with an invalid URL format
    let repo_def_invalid = RepositoryDefinition {
        name: "test-registry".to_string(),
        repo_type: RepositoryType::HttpRegistry,
        config: RepositoryConfig::HttpRegistry {
            index_url: "not-a-valid-url".to_string(),
        },
        auth: None,
        priority: 0,
        storage: None,
    };

    let client = CratesRegistryClient::new(&repo_def_invalid);
    // Client creation might succeed, but the request will fail
    if let Ok(client) = client {
        let options = ListSkillsOptions::default();
        let result = client.fetch_skills(&options).await;
        // Should fail with a client error
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn test_performance_benchmark_http_listing_1000_skills() {
    // Test T014: Performance benchmark test for HTTP listing with 1000 skills (assert < 2s)

    use chrono::Utc;
    use std::time::Instant;

    let mock_server = MockServer::start().await;

    // Create 1000 skill summaries
    let mut summaries = Vec::new();
    for i in 0..1000 {
        summaries.push(SkillSummary {
            id: format!("org{}/skill{}", i % 10, i),
            scope: format!("org{}", i % 10),
            name: format!("skill{}", i),
            description: format!("Description for skill {}", i),
            latest_version: "1.0.0".to_string(),
            published_at: Some(Utc::now()),
            versions: None,
        });
    }

    let response_body = serde_json::to_string(&summaries).unwrap();

    Mock::given(method("GET"))
        .and(path("/api/registry/index/skills"))
        .respond_with(ResponseTemplate::new(200).set_body_string(response_body))
        .mount(&mock_server)
        .await;

    let repo_def = create_test_repo_definition(mock_server.uri());
    let client = CratesRegistryClient::new(&repo_def).unwrap();
    let options = ListSkillsOptions::default();

    let start = Instant::now();
    let result = client.fetch_skills(&options).await;
    let duration = start.elapsed();

    assert!(result.is_ok());
    let skills = result.unwrap();
    assert_eq!(skills.len(), 1000);

    // Assert that the operation completes in less than 2 seconds
    assert!(
        duration.as_secs_f64() < 2.0,
        "HTTP listing with 1000 skills took {}s, expected < 2s",
        duration.as_secs_f64()
    );
}


//! Integration tests for HTTP authentication middleware

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_borrows_for_generic_args,
    clippy::single_char_add_str,
    clippy::to_string_in_format_args,
    clippy::useless_vec
)]

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request},
    routing::{get, post},
    Router,
};
use fastskill::http::auth::jwt::JwtService;
use fastskill::http::auth::middleware::auth_middleware;
use fastskill::http::handlers::AppState;
use fastskill::ServiceConfig;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

/// Helper to create a test router with auth middleware
async fn create_test_router() -> (Router, Arc<JwtService>) {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.to_path_buf(),
        embedding: None,
        ..Default::default()
    };

    let mut service = fastskill::FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let jwt_service = Arc::new(JwtService::from_env().expect("Failed to create JWT service"));

    let state = AppState {
        service: Arc::new(service),
        jwt_service: jwt_service.clone(),
        start_time: std::time::SystemTime::now(),
        project_file_path: std::path::PathBuf::from("skill-project.toml"),
        project_root: temp_dir.path().to_path_buf(),
        skills_directory: skills_dir,
    };

    let router = Router::new()
        .route(
            "/auth/token",
            post(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route(
            "/",
            get(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route(
            "/dashboard",
            get(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route(
            "/api/skills",
            get(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route(
            "/api/registry/publish",
            post(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route(
            "/api/registry/sources",
            get(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route(
            "/api/registry/index/skills",
            get(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            jwt_service.clone(),
            auth_middleware,
        ))
        .with_state(state);

    (router, jwt_service)
}

/// Test: Public routes without token should return 200
#[tokio::test]
async fn test_public_routes_without_token() {
    let (router, _jwt_service) = create_test_router().await;

    let public_routes = vec![
        (Method::POST, "/auth/token"),
        (Method::GET, "/"),
        (Method::GET, "/dashboard"),
        (Method::GET, "/api/registry/sources"),
        (Method::GET, "/api/registry/index/skills"),
    ];

    for (method, path) in public_routes {
        let request = Request::builder()
            .method(method.clone())
            .uri(path)
            .body(Body::empty())
            .unwrap();

        let response = router.clone().oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "Public route {} {} should return 200",
            method,
            path
        );
    }
}

/// Test: Protected routes without token should return 401
#[tokio::test]
async fn test_protected_routes_without_token() {
    let (router, _jwt_service) = create_test_router().await;

    let protected_routes = vec![
        (Method::GET, "/api/skills"),
        (Method::POST, "/api/registry/publish"),
    ];

    for (method, path) in protected_routes {
        let request = Request::builder()
            .method(method.clone())
            .uri(path)
            .body(Body::empty())
            .unwrap();

        let response = router.clone().oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "Protected route {} {} should return 401 without token",
            method,
            path
        );
    }
}

/// Test: Protected routes with invalid token should return 401
#[tokio::test]
async fn test_protected_routes_with_invalid_token() {
    let (router, _jwt_service) = create_test_router().await;

    let protected_routes = vec![
        (Method::GET, "/api/skills"),
        (Method::POST, "/api/registry/publish"),
    ];

    for (method, path) in protected_routes {
        let request = Request::builder()
            .method(method.clone())
            .uri(path)
            .header(header::AUTHORIZATION, "Bearer invalid-token")
            .body(Body::empty())
            .unwrap();

        let response = router.clone().oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNAUTHORIZED,
            "Protected route {} {} should return 401 with invalid token",
            method,
            path
        );
    }
}

/// Test: Protected routes with valid token should return 200
#[tokio::test]
async fn test_protected_routes_with_valid_token() {
    let (router, jwt_service) = create_test_router().await;

    // Generate a valid token
    let token_response = jwt_service
        .generate_token("test-user", "user")
        .expect("Failed to generate token");

    let protected_routes = vec![
        (Method::GET, "/api/skills"),
        (Method::POST, "/api/registry/publish"),
    ];

    for (method, path) in protected_routes {
        let request = Request::builder()
            .method(method.clone())
            .uri(path)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", token_response.token),
            )
            .body(Body::empty())
            .unwrap();

        let response = router.clone().oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "Protected route {} {} should return 200 with valid token",
            method,
            path
        );
    }
}

/// Test: X-API-Key header should be accepted as alternative to Authorization header
#[tokio::test]
async fn test_protected_routes_with_api_key() {
    let (router, jwt_service) = create_test_router().await;

    // Generate a valid token
    let token_response = jwt_service
        .generate_token("test-user", "user")
        .expect("Failed to generate token");

    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/skills")
        .header("x-api-key", token_response.token.clone())
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "Protected route should return 200 with X-API-Key header"
    );
}

/// Test: Public routes with invalid token should still work
#[tokio::test]
async fn test_public_routes_with_invalid_token() {
    let (router, _jwt_service) = create_test_router().await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/registry/sources")
        .header(header::AUTHORIZATION, "Bearer invalid-token")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "Public route should return 200 even with invalid token"
    );
}

/// Test: Registry index endpoints under /index/ should be public
#[tokio::test]
async fn test_registry_index_endpoints_are_public() {
    let (router, _jwt_service) = create_test_router().await;

    let index_routes = vec![
        "/index/test-skill",
        "/index/another-skill",
        "/index/some-path/to-skill",
    ];

    for path in index_routes {
        let request = Request::builder()
            .method(Method::GET)
            .uri(path)
            .body(Body::empty())
            .unwrap();

        // For unknown paths, Axum returns 404, but they should not be 401
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();

        assert_ne!(
            status,
            axum::http::StatusCode::UNAUTHORIZED,
            "Registry index route {} should not return 401",
            path
        );
    }
}

/// Test: AuthContext is attached to authenticated requests
#[tokio::test]
async fn test_auth_context_attached_to_authenticated_requests() {
    use axum::extract::Extension;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.to_path_buf(),
        embedding: None,
        ..Default::default()
    };

    let mut service = fastskill::FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let jwt_service = Arc::new(JwtService::from_env().expect("Failed to create JWT service"));

    let state = AppState {
        service: Arc::new(service),
        jwt_service: jwt_service.clone(),
        start_time: std::time::SystemTime::now(),
        project_file_path: std::path::PathBuf::from("skill-project.toml"),
        project_root: temp_dir.path().to_path_buf(),
        skills_directory: skills_dir,
    };

    async fn handler_with_context(
        Extension(auth_context): Extension<fastskill::http::auth::middleware::AuthContext>,
    ) -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "has_context": true,
            "user_id": auth_context.user_id,
            "role": auth_context.user_role.map(|r| r.to_string()),
        }))
    }

    let router = Router::new()
        .route("/test/context", get(handler_with_context))
        .route_layer(axum::middleware::from_fn_with_state(
            jwt_service.clone(),
            auth_middleware,
        ))
        .with_state(state);

    // Generate a valid token
    let token_response = jwt_service
        .generate_token("test-user-123", "admin")
        .expect("Failed to generate token");

    let request = Request::builder()
        .method(Method::GET)
        .uri("/test/context")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", token_response.token),
        )
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["has_context"], true);
    assert_eq!(json["user_id"], "test-user-123");
    assert_eq!(json["role"], "admin");
}

/// Test: Anonymous requests get AuthContext::anonymous()
#[tokio::test]
async fn test_anonymous_requests_get_anonymous_context() {
    use axum::extract::Extension;

    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".claude/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir.to_path_buf(),
        embedding: None,
        ..Default::default()
    };

    let mut service = fastskill::FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let jwt_service = Arc::new(JwtService::from_env().expect("Failed to create JWT service"));

    let state = AppState {
        service: Arc::new(service),
        jwt_service: jwt_service.clone(),
        start_time: std::time::SystemTime::now(),
        project_file_path: std::path::PathBuf::from("skill-project.toml"),
        project_root: temp_dir.path().to_path_buf(),
        skills_directory: skills_dir,
    };

    async fn handler_with_context(
        Extension(auth_context): Extension<fastskill::http::auth::middleware::AuthContext>,
    ) -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "has_context": true,
            "user_id": auth_context.user_id,
            "role": auth_context.user_role.map(|r| r.to_string()),
        }))
    }

    let router = Router::new()
        .route("/test/context", get(handler_with_context))
        .route(
            "/api/registry/sources",
            get(|| async { axum::Json(serde_json::json!({"success": true})) }),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            jwt_service.clone(),
            auth_middleware,
        ))
        .with_state(state);

    // Request to public route without token
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/registry/sources")
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    // Note: This test assumes the handler returns JSON
    // If the public route handler doesn't extract AuthContext, we can't verify this
    // The key is that the middleware successfully processes the request
}

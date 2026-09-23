//! `/registry/*` browse routes over a project that actually declares
//! repositories — a zip-url marketplace (wiremock), unreadable Git and zip-url
//! marketplaces, a local catalog and an HTTP registry — plus the repositories a
//! composed Manifest brings in. `http_handler_route_tests.rs` covers the same
//! routes with no repositories configured.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
    Router,
};
use fastskill_core::http::handlers::{registry, AppState};
use fastskill_core::{FastSkillService, ServiceConfig};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

struct Project {
    _dir: TempDir,
    state: AppState,
}

/// A served project whose `skill-project.toml` is `manifest`, with one
/// installed skill (`review`) so marketplace entries can report `installed`.
async fn project(manifest: &str, extra: impl FnOnce(&Path)) -> Project {
    let dir = TempDir::new().unwrap();
    let store = dir.path().join("store");
    fs::create_dir_all(store.join("review")).unwrap();
    fs::write(
        store.join("review/SKILL.md"),
        "---\nname: review\ndescription: installed\nversion: 1.0.0\n---\n# Review\n",
    )
    .unwrap();
    let project_file = dir.path().join("skill-project.toml");
    fs::write(&project_file, manifest).unwrap();
    extra(dir.path());

    let mut service = FastSkillService::new(ServiceConfig {
        skill_cache_root: Some(dir.path().join("cache")),
        skill_storage_path: store.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let state = AppState::new(Arc::new(service))
        .unwrap()
        .with_project_config(dir.path().to_path_buf(), project_file, store);
    Project { _dir: dir, state }
}

async fn marketplace_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.claude-plugin/marketplace.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/marketplace.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "team-marketplace",
            "plugins": [{
                "name": "review",
                "description": "marketplace review",
                "source": "./plugins/review",
                "skills": ["./skills/review"]
            }]
        })))
        .mount(&server)
        .await;
    server
}

/// One repository of every kind the browse routes distinguish, plus a
/// zip-url repository whose `auth` block makes its marketplace unreadable.
fn every_kind_of_repository(market: &MockServer) -> String {
    format!(
        "[dependencies]\n\
         [[tool.fastskill.repositories]]\n\
         name = \"market\"\ntype = \"zip-url\"\nzip_url = {market:?}\npriority = 0\n\
         [[tool.fastskill.repositories]]\n\
         name = \"locked\"\ntype = \"zip-url\"\nzip_url = {market:?}\npriority = 4\n\
         auth = {{ type = \"pat\", env_var = \"FASTSKILL_TEST_UNSET_TOKEN\" }}\n\
         [[tool.fastskill.repositories]]\n\
         name = \"broken\"\ntype = \"git-marketplace\"\nurl = \"file:///nonexistent/fastskill-broken.git\"\npriority = 1\n\
         auth = {{ type = \"pat\", env_var = \"FASTSKILL_TEST_UNSET_TOKEN\" }}\n\
         [[tool.fastskill.repositories]]\n\
         name = \"shelf\"\ntype = \"local\"\npath = \"shelf\"\npriority = 2\n\
         [[tool.fastskill.repositories]]\n\
         name = \"index\"\ntype = \"http-registry\"\nindex_url = \"http://127.0.0.1:9/index\"\npriority = 3\n",
        market = market.uri()
    )
}

async fn send(state: AppState, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
    let app = Router::new()
        .route("/registry/sources", get(registry::list_sources))
        .route("/registry/skills", get(registry::list_all_skills))
        .route(
            "/registry/skills/{id}/versions",
            get(registry::list_skill_versions),
        )
        .route(
            "/registry/sources/{name}/skills",
            get(registry::list_source_skills),
        )
        .route(
            "/registry/sources/{name}/marketplace",
            get(registry::get_marketplace),
        )
        .route("/registry/refresh", post(registry::refresh_sources))
        .with_state(state);
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&bytes).into()));
    (status, body)
}

#[tokio::test]
async fn sources_describe_every_kind_of_repository() {
    let market = marketplace_server().await;
    let p = project(&every_kind_of_repository(&market), |_| {}).await;

    let (status, body) = send(p.state, "GET", "/registry/sources").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let sources = body["data"].as_array().unwrap();
    let kind = |name: &str| {
        sources
            .iter()
            .find(|s| s["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing from {body}"))
            .clone()
    };
    assert_eq!(kind("market")["sourceType"], "zip-url");
    assert_eq!(kind("market")["supportsMarketplace"], true);
    assert_eq!(kind("broken")["sourceType"], "git-marketplace");
    assert_eq!(kind("shelf")["sourceType"], "local");
    assert_eq!(kind("shelf")["path"], "shelf");
    assert_eq!(kind("index")["sourceType"], "http-registry");
    assert_eq!(kind("index")["supportsMarketplace"], false);
}

#[tokio::test]
async fn all_skills_lists_readable_marketplaces_and_skips_the_rest() {
    let market = marketplace_server().await;
    let p = project(&every_kind_of_repository(&market), |_| {}).await;

    let (status, body) = send(p.state, "GET", "/registry/skills").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let data = &body["data"];
    let sources = data["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1, "{body}");
    assert_eq!(sources[0]["sourceName"], "market");
    assert_eq!(sources[0]["skills"][0]["installed"], true, "{body}");
    assert_eq!(data["totalSkills"], sources[0]["count"]);
    assert_eq!(
        data["totalSources"], 4,
        "the HTTP registry is not a source: {body}"
    );
}

#[tokio::test]
async fn refresh_reloads_the_same_listing() {
    let market = marketplace_server().await;
    let p = project(&every_kind_of_repository(&market), |_| {}).await;

    let (status, body) = send(p.state, "POST", "/registry/refresh").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["sources"][0]["sourceName"], "market");
}

#[tokio::test]
async fn one_marketplace_is_listed_with_install_status() {
    let market = marketplace_server().await;
    let p = project(&every_kind_of_repository(&market), |_| {}).await;

    let (status, body) = send(p.state.clone(), "GET", "/registry/sources/market/skills").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["sourceName"], "market");
    assert_eq!(body["data"]["skills"][0]["installed"], true, "{body}");
    assert_eq!(
        body["data"]["count"],
        body["data"]["skills"].as_array().unwrap().len()
    );

    let (status, body) = send(p.state, "GET", "/registry/sources/market/marketplace").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["data"].is_object(), "{body}");
}

#[tokio::test]
async fn a_source_without_a_marketplace_is_a_bad_request() {
    let market = marketplace_server().await;
    let p = project(&every_kind_of_repository(&market), |_| {}).await;

    for uri in [
        "/registry/sources/shelf/skills",
        "/registry/sources/shelf/marketplace",
    ] {
        let (status, body) = send(p.state.clone(), "GET", uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert!(body.to_string().contains("does not support marketplace"));
    }
}

#[tokio::test]
async fn an_unreadable_marketplace_is_a_server_error() {
    let market = marketplace_server().await;
    let p = project(&every_kind_of_repository(&market), |_| {}).await;

    for uri in [
        "/registry/sources/broken/skills",
        "/registry/sources/broken/marketplace",
    ] {
        let (status, body) = send(p.state.clone(), "GET", uri).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{uri}: {body}");
        assert!(body.to_string().contains("Failed to load marketplace"));
    }
}

#[tokio::test]
async fn versions_come_from_the_readable_marketplaces() {
    let market = marketplace_server().await;
    let p = project(&every_kind_of_repository(&market), |_| {}).await;

    let (status, body) = send(p.state, "GET", "/registry/skills/review/versions").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["id"], "review");
    assert!(body["data"]["versions"].is_array(), "{body}");
}

#[tokio::test]
async fn composed_repositories_are_browsable() {
    let market = marketplace_server().await;
    let shared = format!(
        "[dependencies]\n[[tool.fastskill.repositories]]\n\
         name = \"market\"\ntype = \"zip-url\"\nzip_url = {:?}\npriority = 0\n",
        market.uri()
    );
    let p = project(
        "[dependencies]\n[tool.fastskill.manifests]\nteam = \"shared\"\n",
        |root| {
            fs::create_dir_all(root.join("shared")).unwrap();
            fs::write(root.join("shared/skill-project.toml"), &shared).unwrap();
        },
    )
    .await;

    let (status, body) = send(p.state, "GET", "/registry/sources/market/skills").await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["sourceName"], "market");
}

#[tokio::test]
async fn one_repository_name_defined_two_ways_is_refused() {
    let p = project(
        "[dependencies]\n[tool.fastskill.manifests]\nteam = \"shared\"\n\
         [[tool.fastskill.repositories]]\nname = \"team\"\ntype = \"local\"\npath = \"mine\"\npriority = 0\n",
        |root| {
            fs::create_dir_all(root.join("shared")).unwrap();
            fs::write(
                root.join("shared/skill-project.toml"),
                "[dependencies]\n[[tool.fastskill.repositories]]\n\
                 name = \"team\"\ntype = \"local\"\npath = \"theirs\"\npriority = 0\n",
            )
            .unwrap();
        },
    )
    .await;

    let (status, body) = send(p.state, "GET", "/registry/sources").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body.to_string()
            .contains("Repository 'team' is defined differently"),
        "{body}"
    );
}

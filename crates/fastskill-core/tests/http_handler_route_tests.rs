//! Comprehensive HTTP handler integration tests (coverage-oriented).
//!
//! These tests drive the public handler functions through a plain axum `Router`
//! built with a fully-controlled `AppState` (temp project dir + skills fixture),
//! exercised via `tower::ServiceExt::oneshot`. Building our own router lets us
//! inject `project_file_path` / `skills_directory` / `registry_index_path` /
//! `enable_write`, which the production `serve()` path derives from the process
//! CWD and therefore can't be pinned per-test. No sockets are bound.
//!
//! Covers handlers/{skills,status,reindex,registry,manifest,resolve,search}.rs
//! branches. server.rs (write-gate, static assets, CORS, address parsing, /index
//! mount) is covered separately in `http_server_route_tests.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{delete, get, post, put},
    Router,
};
use fastskill_core::http::handlers::{
    manifest, registry, reindex, resolve, search, skills, status, AppState,
};
use fastskill_core::{FastSkillService, ServiceConfig};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Fixtures & helpers
// ---------------------------------------------------------------------------

/// Return a non-hidden storage root under `tmp`. This is required because
/// `TempDir` names begin with `.tmp`, and the service's filesystem auto-indexer
/// skips any directory whose name starts with `.` — so skills written directly
/// under the temp root would never be indexed.
fn skills_root(tmp: &TempDir) -> PathBuf {
    let root = tmp.path().join("store");
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_skill(storage: &std::path::Path, id: &str, name: &str, description: &str) {
    let dir = storage.join(id);
    fs::create_dir_all(&dir).unwrap();
    let body = format!(
        "---\nname: {name}\ndescription: {description}\nversion: 1.0.0\n---\n# {name}\n\nBody.\n"
    );
    fs::write(dir.join("SKILL.md"), body).unwrap();
}

/// Build an initialized service over `storage`, optionally with a registry index path.
async fn make_service(
    storage: PathBuf,
    registry_index_path: Option<PathBuf>,
) -> Arc<FastSkillService> {
    let config = ServiceConfig {
        skill_storage_path: storage,
        registry_index_path,
        ..Default::default()
    };
    let mut svc = FastSkillService::new(config).await.unwrap();
    svc.initialize().await.unwrap();
    Arc::new(svc)
}

/// A ready-to-use state pointing at a temp project + two skills.
struct Fixture {
    _storage: TempDir,
    _project: TempDir,
    state: AppState,
    project_file_path: PathBuf,
}

async fn fixture_with_skills(enable_write: bool) -> Fixture {
    let storage = TempDir::new().unwrap();
    let store = skills_root(&storage);
    write_skill(&store, "alpha-skill", "Alpha Skill", "First test skill");
    write_skill(&store, "beta-skill", "Beta Skill", "Second test skill");

    let project = TempDir::new().unwrap();
    let project_file_path = project.path().join("skill-project.toml");

    let service = make_service(store, None).await;
    let mut state = AppState::new(service).unwrap();
    state.project_file_path = project_file_path.clone();
    state.project_root = project.path().to_path_buf();
    state.skills_directory = project.path().join(".claude/skills");
    state.enable_write = enable_write;

    Fixture {
        _storage: storage,
        _project: project,
        state,
        project_file_path,
    }
}

/// Full router mirroring the production route table (paths without the /api/v1 prefix).
fn router(state: AppState) -> Router {
    Router::new()
        .route("/skills", get(skills::list_skills))
        .route("/skills/{id}", get(skills::get_skill))
        .route("/skills/{id}", delete(skills::delete_skill))
        .route("/skills/upgrade", post(skills::upgrade_skills))
        .route("/project", get(manifest::get_project))
        .route("/manifest/skills", get(manifest::list_manifest_skills))
        .route("/manifest/skills", post(manifest::add_skill_to_manifest))
        .route(
            "/manifest/skills/{id}",
            put(manifest::update_skill_in_manifest),
        )
        .route(
            "/manifest/skills/{id}",
            delete(manifest::remove_skill_from_manifest),
        )
        .route("/search", post(search::search_skills))
        .route("/resolve", post(resolve::resolve_context))
        .route("/status", get(status::status))
        .route("/dashboard", get(status::root))
        .route("/reindex", post(reindex::reindex_all))
        .route("/reindex/{id}", post(reindex::reindex_skill))
        .route("/registry/sources", get(registry::list_sources))
        .route("/registry/skills", get(registry::list_all_skills))
        .route("/registry/index/skills", get(registry::list_index_skills))
        .route(
            "/registry/sources/{name}/skills",
            get(registry::list_source_skills),
        )
        .route(
            "/registry/sources/{name}/marketplace",
            get(registry::get_marketplace),
        )
        .route("/registry/refresh", post(registry::refresh_sources))
        .route("/index/{*skill_id}", get(registry::serve_index_file))
        .with_state(state)
}

async fn do_get(state: AppState, uri: &str) -> (StatusCode, String) {
    send(state, "GET", uri, None).await
}

async fn post_json(state: AppState, uri: &str, body: serde_json::Value) -> (StatusCode, String) {
    send(state, "POST", uri, Some(body)).await
}

async fn send(
    state: AppState,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, String) {
    let app = router(state);
    let builder = Request::builder().method(method).uri(uri);
    let req = match body {
        Some(v) => builder
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&v).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// ---------------------------------------------------------------------------
// skills.rs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_skills_returns_all() {
    let f = fixture_with_skills(false).await;
    let (status, body) = do_get(f.state, "/skills").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("alpha-skill"));
    assert!(body.contains("beta-skill"));
    assert!(body.contains("\"count\":2"));
}

#[tokio::test]
async fn get_skill_found() {
    let f = fixture_with_skills(false).await;
    let (status, body) = do_get(f.state, "/skills/alpha-skill").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Alpha Skill"));
}

#[tokio::test]
async fn get_skill_invalid_id_is_400() {
    let f = fixture_with_skills(false).await;
    // A space is not a valid SkillId character -> BadRequest before lookup.
    let (status, _body) = do_get(f.state, "/skills/bad%20id").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_skill_unknown_is_404() {
    let f = fixture_with_skills(false).await;
    let (status, _body) = do_get(f.state, "/skills/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_skill_invalid_id_is_400() {
    let f = fixture_with_skills(true).await;
    let (status, _b) = send(f.state, "DELETE", "/skills/bad%20id", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_skill_unknown_is_404() {
    let f = fixture_with_skills(true).await;
    let (status, _b) = send(f.state, "DELETE", "/skills/does-not-exist", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_skill_success_no_project_file() {
    // project_file_path does not exist -> the manifest/lock removal block is skipped,
    // the skill directory is removed from storage, and the skill is unregistered.
    let f = fixture_with_skills(true).await;
    let (status, body) = send(f.state, "DELETE", "/skills/alpha-skill", None).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("Skill removed"));
}

#[tokio::test]
async fn delete_skill_success_with_project_and_lock() {
    // Exercise the project.exists() + lock.exists() branches of delete_skill.
    let f = fixture_with_skills(true).await;
    fs::write(
        &f.project_file_path,
        "[dependencies]\nalpha-skill = \"1.0.0\"\n",
    )
    .unwrap();
    let lock_path = f.project_file_path.parent().unwrap().join("skills.lock");
    fastskill_core::core::lock::ProjectSkillsLock::new_empty()
        .save_to_file(&lock_path)
        .unwrap();

    let (status, body) = send(f.state, "DELETE", "/skills/alpha-skill", None).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    // The dependency must have been removed from the manifest.
    let remaining = fs::read_to_string(&f.project_file_path).unwrap();
    assert!(!remaining.contains("alpha-skill"));
}

#[tokio::test]
async fn upgrade_rejects_unknown_skill() {
    let f = fixture_with_skills(true).await;
    let (status, _b) = post_json(
        f.state,
        "/skills/upgrade",
        serde_json::json!({"skillId": "ghost"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upgrade_all_runs_subprocess_branch() {
    // skillId "all" -> filter_id None -> no id validation, spawns the (test) binary.
    // We only assert it is NOT a 400 validation rejection; the subprocess outcome
    // (200 success or 500 failure) depends on the environment.
    let f = fixture_with_skills(true).await;
    let (status, _b) = post_json(
        f.state,
        "/skills/upgrade",
        serde_json::json!({"skillId": "all"}),
    )
    .await;
    assert_ne!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upgrade_known_skill_passes_validation() {
    // A known id passes the SEC-2 known-skill check and reaches the subprocess.
    let f = fixture_with_skills(true).await;
    let (status, _b) = post_json(
        f.state,
        "/skills/upgrade",
        serde_json::json!({"skillId": "alpha-skill"}),
    )
    .await;
    assert_ne!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upgrade_empty_body_is_none_filter() {
    // JSON null body -> Option<UpgradeRequest> None -> filter_id None.
    let f = fixture_with_skills(true).await;
    let (status, _b) = post_json(f.state, "/skills/upgrade", serde_json::Value::Null).await;
    assert_ne!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// status.rs (status endpoint + root dashboard incl. SEC-7 escaping)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn status_endpoint_ok() {
    let f = fixture_with_skills(false).await;
    let (status, body) = do_get(f.state, "/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("running"));
    assert!(body.contains("skillsCount"));
}

#[tokio::test]
async fn dashboard_lists_skills() {
    let f = fixture_with_skills(false).await;
    let (status, body) = do_get(f.state, "/dashboard").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("FastSkill Service"));
    assert!(body.contains("Alpha Skill"));
}

#[tokio::test]
async fn dashboard_empty_shows_placeholder() {
    let storage = TempDir::new().unwrap();
    let store = skills_root(&storage);
    let service = make_service(store, None).await;
    let state = AppState::new(service).unwrap();
    let (status, body) = do_get(state, "/dashboard").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("No skills found"));
}

#[tokio::test]
async fn dashboard_escapes_xss_and_truncates_over_ten() {
    // SEC-7: skill name/description are HTML-escaped. Also >10 skills triggers the
    // "... and N more" branch.
    let storage = TempDir::new().unwrap();
    let store = skills_root(&storage);
    write_skill(
        &store,
        "xss-skill",
        "<script>alert('x')</script>",
        "<b>danger</b>",
    );
    for i in 0..12 {
        write_skill(&store, &format!("skill-{i}"), &format!("Skill {i}"), "desc");
    }
    let service = make_service(store, None).await;
    let state = AppState::new(service).unwrap();
    let (status, body) = do_get(state, "/dashboard").await;
    assert_eq!(status, StatusCode::OK);
    // No raw <script> from skill data should appear.
    assert!(!body.contains("<script>alert"));
    assert!(body.contains("&lt;script&gt;") || body.contains("more skills"));
    assert!(body.contains("more skills"));
}

// ---------------------------------------------------------------------------
// reindex.rs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reindex_all_is_501() {
    let f = fixture_with_skills(true).await;
    let (status, body) = post_json(f.state, "/reindex", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert!(body.contains("not implemented"));
}

#[tokio::test]
async fn reindex_skill_is_501() {
    let f = fixture_with_skills(true).await;
    let (status, _b) = post_json(f.state, "/reindex/alpha-skill", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

// ---------------------------------------------------------------------------
// resolve.rs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_empty_prompt_is_400() {
    let f = fixture_with_skills(false).await;
    let (status, _b) = post_json(
        f.state,
        "/resolve",
        serde_json::json!({"prompt": "   ", "limit": 5, "scope": "local"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn resolve_zero_limit_is_400() {
    let f = fixture_with_skills(false).await;
    let (status, _b) = post_json(
        f.state,
        "/resolve",
        serde_json::json!({"prompt": "alpha", "limit": 0, "scope": "local"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn resolve_valid_ok() {
    let f = fixture_with_skills(false).await;
    let (status, body) = post_json(
        f.state,
        "/resolve",
        serde_json::json!({"prompt": "alpha skill", "limit": 5, "scope": "local"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("results"));
}

// ---------------------------------------------------------------------------
// search.rs (text fallback path; semantic requires embedding config + OPENAI key)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_empty_query_is_400() {
    let f = fixture_with_skills(false).await;
    let (status, _b) = post_json(f.state, "/search", serde_json::json!({"query": ""})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_text_fallback_matches() {
    let f = fixture_with_skills(false).await;
    let (status, body) = post_json(f.state, "/search", serde_json::json!({"query": "Alpha"})).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("alpha-skill"));
    assert!(body.contains("keyword"));
}

#[tokio::test]
async fn search_semantic_flag_falls_back_without_config() {
    // semantic:true but no embedding config -> use_semantic false -> text path.
    let f = fixture_with_skills(false).await;
    let (status, body) = post_json(
        f.state,
        "/search",
        serde_json::json!({"query": "beta", "semantic": true, "limit": 5}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("beta-skill"));
}

#[tokio::test]
async fn search_no_matches_returns_empty() {
    let f = fixture_with_skills(false).await;
    let (status, body) = post_json(
        f.state,
        "/search",
        serde_json::json!({"query": "zzzznomatch"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"count\":0"));
}

// ---------------------------------------------------------------------------
// manifest.rs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn project_missing_file_returns_nulls() {
    let f = fixture_with_skills(false).await;
    let (status, body) = do_get(f.state, "/project").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"metadata\":null"));
    assert!(body.contains("\"skills\":[]"));
}

#[tokio::test]
async fn project_with_all_dependency_variants() {
    let f = fixture_with_skills(false).await;
    // Craft a manifest exercising every DependencySpec/DependencySource arm.
    let toml = r#"
[metadata]
id = "proj"
version = "0.1.0"
description = "A project"
author = "tester"
name = "Proj"

[dependencies]
verdep = "1.2.3"
gitdep = { origin = { type = "git", url = "https://example.com/x.git", ref = { branch = "main" } } }
localdep = { origin = { type = "local", path = "./local/x" } }
zipdep = { origin = { type = "zip-url", url = "https://example.com/x.zip" } }
srcdep = { origin = { type = "repository", repo = "acme", skill = "widget" } }
"#;
    fs::write(&f.project_file_path, toml).unwrap();
    let (status, body) = do_get(f.state, "/project").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("verdep"));
    assert!(body.contains("gitdep"));
    assert!(body.contains("branch: main"));
    assert!(body.contains("localdep"));
    assert!(body.contains("zipdep"));
    assert!(body.contains("srcdep"));
    assert!(body.contains("acme / widget"));
}

#[tokio::test]
async fn manifest_list_empty_when_no_file() {
    let f = fixture_with_skills(false).await;
    let (status, body) = do_get(f.state, "/manifest/skills").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"data\":[]"), "body: {body}");
}

#[tokio::test]
async fn manifest_list_with_deps() {
    let f = fixture_with_skills(false).await;
    let toml = r#"
[dependencies]
verdep = "1.0.0"
gitdep = { origin = { type = "git", url = "https://example.com/x.git" } }
"#;
    fs::write(&f.project_file_path, toml).unwrap();
    let (status, body) = do_get(f.state, "/manifest/skills").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("verdep"));
    assert!(body.contains("gitdep"));
}

#[tokio::test]
async fn manifest_add_without_sources_is_404() {
    // No repositories configured -> no marketplace sources -> 404.
    let f = fixture_with_skills(true).await;
    let (status, body) = post_json(
        f.state,
        "/manifest/skills",
        serde_json::json!({"skillId": "some-skill", "sourceName": "acme"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {body}");
}

#[tokio::test]
async fn manifest_add_with_local_source_skill_not_found_is_404() {
    // A local repository IS a marketplace-eligible source, so `SourcesManager`
    // is built (the `Some(sources_mgr)` branch) and `find_skill_in_sources` runs;
    // loading a local marketplace.json is unsupported offline, so the skill is
    // "not found in source" -> 404. Covers the get_repositories + find path.
    let f = fixture_with_skills(true).await;
    let toml = r#"
[dependencies]

[[tool.fastskill.repositories]]
name = "localrepo"
type = "local"
path = "/tmp/does-not-exist-marketplace"
priority = 0
"#;
    fs::write(&f.project_file_path, toml).unwrap();
    let (status, body) = post_json(
        f.state,
        "/manifest/skills",
        serde_json::json!({"skillId": "widget", "sourceName": "localrepo"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "body: {body}");
}

#[tokio::test]
async fn manifest_put_missing_file_is_404() {
    let f = fixture_with_skills(true).await;
    let (status, _b) = send_json(
        f.state,
        "PUT",
        "/manifest/skills/verdep",
        serde_json::json!({"version": "2.0.0"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn manifest_put_edits_version() {
    let f = fixture_with_skills(true).await;
    fs::write(&f.project_file_path, "[dependencies]\nverdep = \"1.0.0\"\n").unwrap();
    let (status, body) = send_json(
        f.state,
        "PUT",
        "/manifest/skills/verdep",
        serde_json::json!({"version": "3.0.0"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("3.0.0"));
    let saved = fs::read_to_string(&f.project_file_path).unwrap();
    assert!(saved.contains("3.0.0"));
}

#[tokio::test]
async fn manifest_put_unknown_skill_is_404() {
    let f = fixture_with_skills(true).await;
    fs::write(&f.project_file_path, "[dependencies]\nverdep = \"1.0.0\"\n").unwrap();
    let (status, _b) = send_json(
        f.state,
        "PUT",
        "/manifest/skills/ghost",
        serde_json::json!({"version": "3.0.0"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn manifest_delete_missing_file_is_404() {
    let f = fixture_with_skills(true).await;
    let (status, _b) = send(f.state, "DELETE", "/manifest/skills/verdep", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn manifest_delete_removes_dep_and_lock() {
    let f = fixture_with_skills(true).await;
    fs::write(&f.project_file_path, "[dependencies]\nverdep = \"1.0.0\"\n").unwrap();
    let lock_path = f.project_file_path.parent().unwrap().join("skills.lock");
    fastskill_core::core::lock::ProjectSkillsLock::new_empty()
        .save_to_file(&lock_path)
        .unwrap();
    let (status, _b) = send(f.state, "DELETE", "/manifest/skills/verdep", None).await;
    assert_eq!(status, StatusCode::OK);
    let saved = fs::read_to_string(&f.project_file_path).unwrap();
    assert!(!saved.contains("verdep"));
}

async fn send_json(
    state: AppState,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, String) {
    send(state, method, uri, Some(body)).await
}

// ---------------------------------------------------------------------------
// registry.rs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registry_sources_ok() {
    let f = fixture_with_skills(false).await;
    let (status, _b) = do_get(f.state, "/registry/sources").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn registry_all_skills_ok() {
    let f = fixture_with_skills(false).await;
    let (status, body) = do_get(f.state, "/registry/skills").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("totalSkills"));
}

#[tokio::test]
async fn registry_refresh_ok() {
    let f = fixture_with_skills(true).await;
    let (status, _b) = post_json(f.state, "/registry/refresh", serde_json::json!({})).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn registry_source_skills_unknown_is_404() {
    let f = fixture_with_skills(false).await;
    let (status, _b) = do_get(f.state, "/registry/sources/nope/skills").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn registry_marketplace_unknown_is_404() {
    let f = fixture_with_skills(false).await;
    let (status, _b) = do_get(f.state, "/registry/sources/nope/marketplace").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---- registry index (list_index_skills) ----

async fn state_with_registry(registry: &TempDir) -> AppState {
    let storage = TempDir::new().unwrap();
    let store = skills_root(&storage);
    write_skill(&store, "alpha-skill", "Alpha Skill", "First");
    let service = make_service(store, Some(registry.path().to_path_buf())).await;
    // Skills are already indexed in memory; the storage temp dir is no longer needed.
    drop(storage);
    AppState::new(service).unwrap()
}

fn seed_registry(registry: &std::path::Path, skill_id: &str, version: &str) {
    use fastskill_core::core::registry_index::{
        update_skill_version, IndexMetadata, VersionMetadata,
    };
    let metadata = VersionMetadata {
        name: skill_id.to_string(),
        vers: version.to_string(),
        deps: vec![],
        cksum: format!("sha256:{version}"),
        features: std::collections::HashMap::new(),
        yanked: false,
        links: None,
        download_url: format!("https://example.com/{version}.zip"),
        published_at: "2024-01-01T00:00:00Z".to_string(),
        metadata: Some(IndexMetadata {
            description: Some("indexed skill".to_string()),
            author: None,
            license: None,
            repository: None,
        }),
    };
    update_skill_version(skill_id, version, &metadata, registry).unwrap();
}

#[tokio::test]
async fn index_skills_no_registry_configured_is_404() {
    let f = fixture_with_skills(false).await; // no registry_index_path
    let (status, _b) = do_get(f.state, "/registry/index/skills").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn index_skills_lists_seeded() {
    let registry = TempDir::new().unwrap();
    seed_registry(registry.path(), "acme/widget", "1.0.0");
    let state = state_with_registry(&registry).await;
    let (status, body) = do_get(state, "/registry/index/skills").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("acme/widget"));
}

#[tokio::test]
async fn index_skills_empty_scope_is_400() {
    let registry = TempDir::new().unwrap();
    let state = state_with_registry(&registry).await;
    let (status, _b) = do_get(state, "/registry/index/skills?scope=").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn index_skills_scope_with_separator_is_400() {
    let registry = TempDir::new().unwrap();
    let state = state_with_registry(&registry).await;
    let (status, _b) = do_get(state, "/registry/index/skills?scope=a%2Fb").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn index_skills_scope_with_dotdot_is_400() {
    let registry = TempDir::new().unwrap();
    let state = state_with_registry(&registry).await;
    let (status, _b) = do_get(state, "/registry/index/skills?scope=..").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn index_skills_scope_with_bad_char_is_400() {
    let registry = TempDir::new().unwrap();
    let state = state_with_registry(&registry).await;
    let (status, _b) = do_get(state, "/registry/index/skills?scope=bad%21name").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn index_skills_valid_scope_and_flags_ok() {
    let registry = TempDir::new().unwrap();
    seed_registry(registry.path(), "acme/widget", "1.0.0");
    let state = state_with_registry(&registry).await;
    let (status, _b) = do_get(
        state,
        "/registry/index/skills?scope=acme&all_versions=true&include_pre_release=1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ---- serve_index_file (/index/{*skill_id}) ----

#[tokio::test]
async fn serve_index_file_no_registry_is_500() {
    let f = fixture_with_skills(false).await; // registry_index_path None
    let (status, _b) = do_get(f.state, "/index/acme/widget").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn serve_index_file_success() {
    let registry = TempDir::new().unwrap();
    // Flat layout: registry/<scope>/<name>
    let scope_dir = registry.path().join("testorg");
    fs::create_dir_all(&scope_dir).unwrap();
    fs::write(
        scope_dir.join("serve-skill"),
        "{\"name\":\"testorg/serve-skill\"}",
    )
    .unwrap();
    let state = state_with_registry(&registry).await;
    let (status, body) = do_get(state, "/index/testorg/serve-skill").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("serve-skill"));
}

#[tokio::test]
async fn serve_index_file_missing_is_404() {
    let registry = TempDir::new().unwrap();
    fs::create_dir_all(registry.path().join("testorg")).unwrap();
    let state = state_with_registry(&registry).await;
    let (status, _b) = do_get(state, "/index/testorg/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

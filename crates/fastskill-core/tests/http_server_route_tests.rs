//! Server-level HTTP tests for `http/server.rs`.
//!
//! Two groups:
//!  1. Spawned-server (reqwest) tests for pieces only reachable through the real
//!     `serve()` wiring: embedded static assets, the root dashboard fallback, and
//!     the `/index` registry mount.
//!  2. Direct unit tests for the public `build_cors_layer` (all origin/header
//!     branches incl. the SEC-10 wildcard guard) and for address normalization /
//!     parsing via the `FastSkillServer` constructors.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::core::service::HttpServerConfig;
use fastskill_core::http::server::{build_cors_layer, FastSkillServer};
use fastskill_core::{FastSkillService, ServiceConfig};
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn free_port() -> Option<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(port)
}

fn wait_for_port(port: u16, secs: u64) -> bool {
    let start = Instant::now();
    while start.elapsed().as_secs() < secs {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

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

// ---------------------------------------------------------------------------
// Spawned-server tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn static_assets_and_dashboard_and_index_mount() {
    let storage = TempDir::new().unwrap();
    // one skill so the dashboard renders a list item. Use a non-hidden subdir:
    // the auto-indexer skips directories starting with '.', and TempDir names do.
    let store = storage.path().join("store");
    let skill_dir = store.join("demo-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Demo\ndescription: A demo skill\nversion: 1.0.0\n---\n# Demo\n",
    )
    .unwrap();

    // registry index (flat layout) for the /index mount
    let registry = TempDir::new().unwrap();
    let scope_dir = registry.path().join("testorg");
    fs::create_dir_all(&scope_dir).unwrap();
    fs::write(
        scope_dir.join("serve-skill"),
        "{\"name\":\"testorg/serve-skill\"}",
    )
    .unwrap();

    let Some(port) = free_port() else {
        return;
    };
    let service = make_service(store, Some(registry.path().to_path_buf())).await;
    let server = FastSkillServer::new(service, "127.0.0.1", port).enable_write(false);
    let handle = tokio::spawn(async move {
        let _ = server.serve().await;
    });
    assert!(wait_for_port(port, 10), "server failed to start");

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();

    // Embedded static assets (serve_embedded_static: every content-type arm).
    for (path, ctype) in [
        ("/", "text/html"),
        ("/index.html", "text/html"),
        ("/app.js", "javascript"),
        ("/styles.css", "text/css"),
    ] {
        let resp = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK, "GET {path}");
        let got_ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(got_ct.contains(ctype), "GET {path} content-type={got_ct}");
    }

    // Root dashboard fallback (status::root).
    let dash = client
        .get(format!("{base}/dashboard"))
        .send()
        .await
        .unwrap();
    assert_eq!(dash.status(), reqwest::StatusCode::OK);
    let dash_body = dash.text().await.unwrap();
    assert!(dash_body.contains("FastSkill Service"));
    assert!(dash_body.contains("Demo"));

    // /index mount -> serve_index_file success.
    let idx = client
        .get(format!("{base}/index/testorg/serve-skill"))
        .send()
        .await
        .unwrap();
    assert_eq!(idx.status(), reqwest::StatusCode::OK);
    let idx_ct = idx
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(idx_ct.contains("application/json"));

    // /index mount -> missing file 404.
    let missing = client
        .get(format!("{base}/index/testorg/ghost"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);

    handle.abort();
}

// ---------------------------------------------------------------------------
// build_cors_layer branch coverage
// ---------------------------------------------------------------------------

#[test]
fn cors_wildcard_origin_falls_back_to_deny_all() {
    // SEC-10: "*" combined with credentials is a foot-gun -> deny all.
    let config = ServiceConfig {
        http_server: Some(HttpServerConfig {
            allowed_origins: vec!["*".to_string()],
            allowed_headers: vec!["Content-Type".to_string()],
        }),
        ..Default::default()
    };
    let _ = build_cors_layer(&config);
}

#[test]
fn cors_invalid_origin_value_falls_back() {
    let config = ServiceConfig {
        http_server: Some(HttpServerConfig {
            // Newline is not a valid header value -> parse_origins errors.
            allowed_origins: vec!["https://ok.com".to_string(), "bad\norigin".to_string()],
            allowed_headers: vec!["Content-Type".to_string()],
        }),
        ..Default::default()
    };
    let _ = build_cors_layer(&config);
}

#[test]
fn cors_invalid_header_name_uses_fallback_headers() {
    let config = ServiceConfig {
        http_server: Some(HttpServerConfig {
            allowed_origins: vec!["https://ok.com".to_string()],
            // Space/newline is not a valid header name -> parse_headers errors.
            allowed_headers: vec!["bad header\n".to_string()],
        }),
        ..Default::default()
    };
    let _ = build_cors_layer(&config);
}

#[test]
fn cors_valid_origins_and_headers_credentialed() {
    let config = ServiceConfig {
        http_server: Some(HttpServerConfig {
            allowed_origins: vec!["https://a.com".to_string(), "https://b.com".to_string()],
            allowed_headers: vec!["X-Custom".to_string(), "Authorization".to_string()],
        }),
        ..Default::default()
    };
    let _ = build_cors_layer(&config);
}

// ---------------------------------------------------------------------------
// Address normalization / parsing via constructors
// ---------------------------------------------------------------------------

async fn any_service() -> Arc<FastSkillService> {
    let storage = TempDir::new().unwrap();
    let svc = make_service(storage.path().to_path_buf(), None).await;
    drop(storage);
    svc
}

#[tokio::test]
async fn address_normalization_ipv4_and_localhost() {
    let svc = any_service().await;
    let s = FastSkillServer::new(Arc::clone(&svc), "localhost", 8080);
    assert_eq!(s.addr().to_string(), "127.0.0.1:8080");

    let s2 = FastSkillServer::new(Arc::clone(&svc), "0.0.0.0", 9090).enable_write(true);
    assert_eq!(s2.addr().to_string(), "0.0.0.0:9090");
}

#[tokio::test]
async fn address_normalization_ipv6_variants() {
    let svc = any_service().await;

    // "::1" loopback -> bracketed IPv6.
    let s = FastSkillServer::from_ref(&svc, "::1", 8081);
    assert_eq!(s.addr().to_string(), "[::1]:8081");

    // "::" all interfaces.
    let s2 = FastSkillServer::from_ref(&svc, "::", 8082).enable_write(false);
    assert_eq!(s2.addr().to_string(), "[::]:8082");

    // Bracketed forms normalize the same way.
    let s3 = FastSkillServer::new(Arc::clone(&svc), "[::1]", 8083);
    assert_eq!(s3.addr().to_string(), "[::1]:8083");
}

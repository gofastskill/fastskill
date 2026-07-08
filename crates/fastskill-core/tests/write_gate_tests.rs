//! Integration tests for the WRITE-GATE (ADR-0003 / spec 002).
//!
//! `serve` is read-only by default: mutating routes are always registered but
//! wrapped in a gate middleware that returns 403 unless the server was started
//! with writes enabled. Read routes are always available.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::http::server::FastSkillServer;
use fastskill_core::{FastSkillService, ServiceConfig};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn free_port() -> Option<u16> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            let port = listener.local_addr().ok()?.port();
            drop(listener);
            Some(port)
        }
        Err(_) => None,
    }
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

async fn make_service(dir: &TempDir) -> Arc<FastSkillService> {
    let config = ServiceConfig {
        skill_storage_path: dir.path().to_path_buf(),
        ..Default::default()
    };
    let mut svc = FastSkillService::new(config).await.unwrap();
    svc.initialize().await.unwrap();
    Arc::new(svc)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_route_returns_403_when_write_disabled() {
    let dir = TempDir::new().unwrap();
    let Some(port) = free_port() else {
        return;
    };
    let service = make_service(&dir).await;
    let server = FastSkillServer::new_with_write(service, "127.0.0.1", port, false);
    let handle = tokio::spawn(async move {
        let _ = server.serve().await;
    });
    assert!(wait_for_port(port, 10), "server failed to start");

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/api/v1/reindex"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("POST /api/v1/reindex");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::FORBIDDEN,
        "write route must be 403 when writes are disabled"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("--enable-write"),
        "403 body should point at --enable-write, got: {body}"
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_route_not_forbidden_when_write_enabled() {
    let dir = TempDir::new().unwrap();
    let Some(port) = free_port() else {
        return;
    };
    let service = make_service(&dir).await;
    let server = FastSkillServer::new_with_write(service, "127.0.0.1", port, true);
    let handle = tokio::spawn(async move {
        let _ = server.serve().await;
    });
    assert!(wait_for_port(port, 10), "server failed to start");

    let client = reqwest::Client::new();
    // reindex is write-gated; with writes enabled it must NOT be 403.
    // (PARTIAL-6: the HTTP reindex path is honestly 501, not a fake 200.)
    let resp = client
        .post(format!("http://127.0.0.1:{port}/api/v1/reindex"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("POST /api/v1/reindex");
    assert_ne!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_IMPLEMENTED);

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_route_available_when_write_disabled() {
    let dir = TempDir::new().unwrap();
    let Some(port) = free_port() else {
        return;
    };
    let service = make_service(&dir).await;
    let server = FastSkillServer::new_with_write(service, "127.0.0.1", port, false);
    let handle = tokio::spawn(async move {
        let _ = server.serve().await;
    });
    assert!(wait_for_port(port, 10), "server failed to start");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/api/v1/skills"))
        .send()
        .await
        .expect("GET /api/v1/skills");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "read route must be available even with writes disabled"
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upgrade_rejects_unknown_skill_id() {
    // SEC-2: an unknown/attacker-controlled skillId must be rejected with 400
    // BEFORE any subprocess is spawned. The skill store is empty here, so any
    // id (including a leading-dash one) is unknown.
    let dir = TempDir::new().unwrap();
    let Some(port) = free_port() else {
        return;
    };
    let service = make_service(&dir).await;
    let server = FastSkillServer::new_with_write(service, "127.0.0.1", port, true);
    let handle = tokio::spawn(async move {
        let _ = server.serve().await;
    });
    assert!(wait_for_port(port, 10), "server failed to start");

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/api/v1/skills/upgrade"))
        .json(&serde_json::json!({"skillId": "-rf-nonexistent"}))
        .send()
        .await
        .expect("POST /api/v1/skills/upgrade");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::BAD_REQUEST,
        "unknown skillId must be rejected with 400"
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removed_create_and_update_routes_are_not_mounted() {
    // PARTIAL-1: POST /skills (create) and PUT /skills/{id} (update) were removed.
    let dir = TempDir::new().unwrap();
    let Some(port) = free_port() else {
        return;
    };
    // Even with writes enabled, these routes must not exist at all.
    let service = make_service(&dir).await;
    let server = FastSkillServer::new_with_write(service, "127.0.0.1", port, true);
    let handle = tokio::spawn(async move {
        let _ = server.serve().await;
    });
    assert!(wait_for_port(port, 10), "server failed to start");

    let client = reqwest::Client::new();
    let create = client
        .post(format!("http://127.0.0.1:{port}/api/v1/skills"))
        .json(&serde_json::json!({"name": "x", "description": "y"}))
        .send()
        .await
        .expect("POST /api/v1/skills");
    assert_eq!(
        create.status(),
        reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "POST /skills should be gone (405), got {}",
        create.status()
    );

    let update = client
        .put(format!("http://127.0.0.1:{port}/api/v1/skills/some-id"))
        .json(&serde_json::json!({"name": "x", "description": "y"}))
        .send()
        .await
        .expect("PUT /api/v1/skills/{id}");
    assert_eq!(
        update.status(),
        reqwest::StatusCode::METHOD_NOT_ALLOWED,
        "PUT /skills/{{id}} should be gone (405), got {}",
        update.status()
    );

    handle.abort();
}

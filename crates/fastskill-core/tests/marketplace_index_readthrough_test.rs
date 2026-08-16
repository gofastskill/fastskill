//! Integration tests for spec 008 "marketplace-cache-reconciliation":
//! `SourcesManager::fetch_and_cache_marketplace` reads through to the on-disk
//! index cache (`SkillCache::read_source_index`) on an in-memory miss,
//! instead of always hitting the network — closing the same offline gap
//! `Origin::Git`/`Origin::Repository` already have (`install::resolve_git_sha`,
//! `install::resolve_repository_version`) for the marketplace-listing path.
//!
//! "Zero HTTP calls" is proven via
//! `core::sources::manager::MARKETPLACE_FETCH_INVOCATIONS`, a counting seam
//! kept for exactly this purpose (mirrors `storage::git::CLONE_INVOCATIONS` /
//! `registry::client::DOWNLOAD_INVOCATIONS` / `install::ZIP_URL_BODY_BYTES_DOWNLOADED`).
//! The `wiremock` `MockServer` fixture used where a live fetch *is* expected
//! is a loopback-bound server, not "the network" — same posture as
//! `zip_url_cache_test.rs`/`registry_content_cache_test.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::core::cache::{SkillCache, SourceIndex, SourceIndexEntry};
use fastskill_core::core::sources::manager::MARKETPLACE_FETCH_INVOCATIONS;
use fastskill_core::core::sources::{SourceConfig, SourcesManager};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// An address nothing listens on: a connection attempt fails fast (refused)
/// without touching the real network — mirrors the same technique
/// `zip_url_cache_test.rs` uses for its offline-fallback tests.
const UNREACHABLE_BASE_URL: &str = "http://127.0.0.1:1";

fn sources_toml_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("sources.toml")
}

/// A minimal, valid Claude Code `marketplace.json` advertising `count`
/// skills (`skill-a`, `skill-b`, ...), each with a non-empty id/name/
/// description/version so `SourcesManager`'s own validation accepts it.
fn claude_marketplace_json(count: usize) -> serde_json::Value {
    let plugins: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            let letter = (b'a' + i as u8) as char;
            serde_json::json!({
                "name": format!("plugin-{letter}"),
                "description": format!("Plugin {letter}"),
                "source": "./",
                "skills": [format!("./skill-{letter}")],
            })
        })
        .collect();
    serde_json::json!({
        "name": "test-marketplace",
        "owner": {"name": "Test Owner"},
        "metadata": {"description": "test marketplace", "version": "1.0.0"},
        "plugins": plugins,
    })
}

// ── FR-1: disk-first bypass, zero HTTP calls ────────────────────────────────

/// A cold `SourcesManager` (empty in-memory cache) with a `SkillCache`
/// carrying a previously-refreshed on-disk index for the source resolves the
/// listing straight from disk: zero HTTP calls, and the returned data
/// matches what was recorded.
#[tokio::test]
async fn cold_sources_manager_resolves_listing_from_disk_index_with_zero_http_calls() {
    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());
    cache
        .write_source_index(
            "acme",
            &SourceIndex {
                fetched_at: chrono::Utc::now(),
                entries: vec![SourceIndexEntry {
                    skill: "alpha".to_string(),
                    versions: vec!["1.0.0".to_string()],
                    name: "Alpha".to_string(),
                    description: "The alpha skill".to_string(),
                }],
            },
        )
        .unwrap();

    let sources_dir = TempDir::new().unwrap();
    let mut manager = SourcesManager::new(sources_toml_path(&sources_dir)).with_skill_cache(cache);
    manager.load().unwrap();
    manager
        .add_source(
            "acme".to_string(),
            SourceConfig::ZipUrl {
                // Never touched if FR-1's disk-first bypass works: nothing
                // listens here, so any real request would fail fast.
                base_url: UNREACHABLE_BASE_URL.to_string(),
                auth: None,
            },
        )
        .unwrap();

    let baseline = MARKETPLACE_FETCH_INVOCATIONS.load(Ordering::SeqCst);
    let marketplace = manager
        .get_marketplace_json("acme")
        .await
        .expect("a disk-index hit must resolve without touching the network");
    let after = MARKETPLACE_FETCH_INVOCATIONS.load(Ordering::SeqCst);

    assert_eq!(
        after, baseline,
        "resolving from the on-disk index must make zero HTTP calls"
    );
    assert_eq!(marketplace.skills.len(), 1);
    let skill = &marketplace.skills[0];
    assert_eq!(skill.id, "alpha");
    assert_eq!(skill.name, "Alpha");
    assert_eq!(skill.description, "The alpha skill");
    assert_eq!(skill.version, "1.0.0");
}

/// With no `SkillCache` configured at all (today's pre-spec-008 behavior,
/// still the default for e.g. `SourcesManager::new`), an in-memory miss goes
/// straight to the network exactly as before — spec 008 must not change
/// behavior for a manager that never opted in.
#[tokio::test]
async fn without_a_skill_cache_configured_behavior_is_unchanged_network_on_miss() {
    let sources_dir = TempDir::new().unwrap();
    let mut manager = SourcesManager::new(sources_toml_path(&sources_dir));
    manager.load().unwrap();
    manager
        .add_source(
            "acme".to_string(),
            SourceConfig::ZipUrl {
                base_url: UNREACHABLE_BASE_URL.to_string(),
                auth: None,
            },
        )
        .unwrap();

    let err = manager
        .get_marketplace_json("acme")
        .await
        .expect_err("no cache configured and nothing reachable must fail, not silently succeed");
    assert!(err.to_string().to_lowercase().contains("failed to fetch"));
}

// ── FR-5: within-operation dedup is preserved ───────────────────────────────

/// Resolving N skills from one marketplace source, in one operation, performs
/// exactly one live fetch — the in-memory `marketplace_cache` collapses the
/// rest, per spec 008's "Correction" (this is the load-bearing behavior the
/// first draft nearly deleted).
#[tokio::test]
async fn resolving_n_skills_from_one_marketplace_performs_exactly_one_fetch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.claude-plugin/marketplace.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(claude_marketplace_json(3)))
        .mount(&server)
        .await;

    let sources_dir = TempDir::new().unwrap();
    let mut manager = SourcesManager::new(sources_toml_path(&sources_dir));
    manager.load().unwrap();
    manager
        .add_source(
            "acme".to_string(),
            SourceConfig::ZipUrl {
                base_url: server.uri(),
                auth: None,
            },
        )
        .unwrap();

    use fastskill_core::core::resolver::{ConflictStrategy, PackageResolver};
    let mut resolver = PackageResolver::new(Arc::new(manager));

    let baseline = MARKETPLACE_FETCH_INVOCATIONS.load(Ordering::SeqCst);
    resolver
        .build_index()
        .await
        .expect("build_index must succeed");
    let after_build = MARKETPLACE_FETCH_INVOCATIONS.load(Ordering::SeqCst);
    assert_eq!(
        after_build - baseline,
        1,
        "one source, one build_index() call, must fetch exactly once"
    );

    for id in ["skill-a", "skill-b", "skill-c"] {
        resolver
            .resolve_skill(id, None, None, ConflictStrategy::Priority)
            .unwrap_or_else(|e| panic!("resolving '{id}' must succeed: {e}"));
    }
    let after_resolves = MARKETPLACE_FETCH_INVOCATIONS.load(Ordering::SeqCst);
    assert_eq!(
        after_resolves, after_build,
        "resolving already-indexed skills must not perform additional fetches"
    );
}

// ── FR-2: a live fetch refreshes the on-disk index ──────────────────────────

/// A successful live fetch (in-memory and on-disk both cold) writes the
/// on-disk index for the source, so a later cold process can resolve the
/// same listing offline (FR-2: the two layers must not drift).
#[tokio::test]
async fn a_successful_live_fetch_refreshes_the_on_disk_index() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.claude-plugin/marketplace.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(claude_marketplace_json(1)))
        .mount(&server)
        .await;

    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());
    assert!(
        cache.read_source_index("acme").unwrap().is_none(),
        "precondition: nothing on disk yet"
    );

    let sources_dir = TempDir::new().unwrap();
    let mut manager =
        SourcesManager::new(sources_toml_path(&sources_dir)).with_skill_cache(cache.clone());
    manager.load().unwrap();
    manager
        .add_source(
            "acme".to_string(),
            SourceConfig::ZipUrl {
                base_url: server.uri(),
                auth: None,
            },
        )
        .unwrap();

    manager
        .get_marketplace_json("acme")
        .await
        .expect("live fetch must succeed");

    let idx = cache
        .read_source_index("acme")
        .unwrap()
        .expect("FR-2: a successful live fetch must write the on-disk index");
    assert_eq!(idx.entries.len(), 1);
    assert_eq!(idx.entries[0].skill, "skill-a");
    assert_eq!(idx.entries[0].versions, vec!["1.0.0".to_string()]);
    assert_eq!(idx.entries[0].name, "skill-a");
    assert_eq!(idx.entries[0].description, "Plugin a");
}

// ── FR-3: network-failure fallback names the recorded fetch time ───────────

/// Captures every `tracing` event's formatted message emitted while alive.
#[derive(Clone, Default)]
struct CapturedLog(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl CapturedLog {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// FR-3 / offline install: a refreshed on-disk index lets an otherwise
/// offline `get_marketplace_json` succeed, and — since this is the same
/// disk-first branch FR-1 uses (both live locations are never even
/// reachable here) — the warning naming the recorded fetch time is emitted
/// regardless of whether the network was actually attempted or bypassed
/// outright, matching the git/zip-url fallback shape
/// (`install::resolve_git_sha` / `install::fetch_zip_url_cached`).
#[tokio::test]
async fn offline_resolution_of_a_refreshed_index_succeeds_and_warns_with_the_recorded_time() {
    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());
    let recorded_at = chrono::Utc::now();
    cache
        .write_source_index(
            "acme",
            &SourceIndex {
                fetched_at: recorded_at,
                entries: vec![SourceIndexEntry {
                    skill: "alpha".to_string(),
                    versions: vec!["1.0.0".to_string()],
                    name: "Alpha".to_string(),
                    description: "The alpha skill".to_string(),
                }],
            },
        )
        .unwrap();

    let sources_dir = TempDir::new().unwrap();
    let mut manager = SourcesManager::new(sources_toml_path(&sources_dir)).with_skill_cache(cache);
    manager.load().unwrap();
    manager
        .add_source(
            "acme".to_string(),
            SourceConfig::ZipUrl {
                base_url: UNREACHABLE_BASE_URL.to_string(),
                auth: None,
            },
        )
        .unwrap();

    let log = CapturedLog::default();
    let make_writer = {
        let log = log.clone();
        move || log.clone()
    };
    let subscriber = tracing_subscriber::fmt()
        .with_writer(make_writer)
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let marketplace = manager
        .get_marketplace_json("acme")
        .await
        .expect("offline install of a skill listed in a refreshed index must succeed");
    assert_eq!(marketplace.skills.len(), 1);
    assert_eq!(marketplace.skills[0].id, "alpha");

    drop(_guard);
    let captured = log.text();
    let expected_time = recorded_at.to_string();
    assert!(
        captured.contains(&expected_time),
        "the warning must name the recorded fetch time ({expected_time}); captured: {captured}"
    );
    assert!(
        captured.to_lowercase().contains("warn"),
        "the fallback must be logged at warn level; captured: {captured}"
    );
}

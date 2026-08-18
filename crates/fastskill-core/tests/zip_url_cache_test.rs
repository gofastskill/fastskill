//! Integration tests for the zip-url content cache (spec 007 "zip-url
//! caching"): unlike git/registry/local, a bare URL carries no identity
//! before the fetch, so `Origin::ZipUrl` must resolve staleness via an HTTP
//! conditional request (`ETag`/`Last-Modified`) rather than a pre-fetch
//! identity check.
//!
//! The fixture is a `wiremock` `MockServer` bound to a loopback port -- this
//! is not "the network" (no DNS, no external host, nothing flaky in CI), the
//! same posture `registry_content_cache_test.rs` takes. "Zero bytes
//! downloaded on a 304" is proven via
//! `core::install::ZIP_URL_BODY_BYTES_DOWNLOADED`, a counting seam kept for
//! exactly this purpose (mirrors `storage::git::CLONE_INVOCATIONS` /
//! `registry::client::DOWNLOAD_INVOCATIONS`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::core::cache::{CacheIdentity, ContentSourceKind, SkillCache, ZipValidator};
use fastskill_core::core::install::ZIP_URL_BODY_BYTES_DOWNLOADED;
use fastskill_core::core::{AddMode, Origin};
use fastskill_core::{FastSkillService, ServiceConfig};
use sha2::Digest;
use std::path::Path;
use std::sync::atomic::Ordering;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SKILL_ID: &str = "zip-cached-skill";

fn skill_md(marker: &str) -> String {
    format!(
        "---\nname: {SKILL_ID}\nversion: \"1.0.0\"\ndescription: {marker}\n---\nBody: {marker}\n"
    )
}

/// Build a `.zip` archive containing a single valid `SKILL.md`, with `marker`
/// baked into the content so different builds are byte-distinguishable.
fn build_zip(marker: &str) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = zip::ZipWriter::new(cursor);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file(format!("{SKILL_ID}/SKILL.md"), opts)
            .unwrap();
        writer.write_all(skill_md(marker).as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buf
}

/// A minimal fastskill project directory: `skill-project.toml` + an empty
/// skills directory, ready for `add_from_origin`.
fn setup_project(root: &Path) {
    std::fs::write(
        root.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".claude/skills")).unwrap();
}

async fn make_service(project_root: &Path, storage: &Path, cache_root: &Path) -> FastSkillService {
    let config = ServiceConfig {
        skill_storage_path: storage.to_path_buf(),
        skill_cache_root: Some(cache_root.to_path_buf()),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();
    service.with_project_root(project_root.to_path_buf())
}

/// Install `url` into a brand-new project + storage dir, sharing `cache_root`
/// with any other install in the same test. Returns the installed
/// `SKILL.md` contents.
async fn install_into_fresh_project(cache_root: &Path, url: &str) -> String {
    let project = tempfile::TempDir::new().unwrap();
    setup_project(project.path());
    let storage = tempfile::TempDir::new().unwrap();
    let service = make_service(project.path(), storage.path(), cache_root).await;

    let origin = Origin::ZipUrl {
        url: url.to_string(),
    };
    service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("zip-url install should succeed");

    std::fs::read_to_string(storage.path().join(SKILL_ID).join("SKILL.md")).unwrap()
}

// ── tests ────────────────────────────────────────────────────────────────

/// Two installs of the same URL: the first is a real download (200 + ETag),
/// the second -- from a different project, sharing the cache -- gets a 304
/// and must download **zero bytes** of body, still installing byte-identical
/// content (spec 007 design + FR-3).
#[tokio::test]
async fn second_install_gets_a_304_and_downloads_zero_bytes() {
    let server = MockServer::start().await;
    let zip_bytes = build_zip("v1");
    let etag = "\"etag-v1\"";

    // A conditional request carrying the etag this server issued -> 304,
    // no body. Higher priority so it wins whenever both mocks match.
    Mock::given(method("GET"))
        .and(path("/pkg.zip"))
        .and(header("If-None-Match", etag))
        .respond_with(ResponseTemplate::new(304))
        .with_priority(1)
        .mount(&server)
        .await;
    // Any other GET (i.e. no matching If-None-Match) -> a full 200 download.
    Mock::given(method("GET"))
        .and(path("/pkg.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(zip_bytes.clone())
                .insert_header("ETag", etag),
        )
        .mount(&server)
        .await;

    let cache_root = tempfile::TempDir::new().unwrap();
    let url = format!("{}/pkg.zip", server.uri());

    let baseline = ZIP_URL_BODY_BYTES_DOWNLOADED.load(Ordering::SeqCst);
    let content_a = install_into_fresh_project(cache_root.path(), &url).await;
    let after_a = ZIP_URL_BODY_BYTES_DOWNLOADED.load(Ordering::SeqCst);
    assert_eq!(
        after_a - baseline,
        zip_bytes.len(),
        "first install must download the full archive body"
    );

    let content_b = install_into_fresh_project(cache_root.path(), &url).await;
    let after_b = ZIP_URL_BODY_BYTES_DOWNLOADED.load(Ordering::SeqCst);
    assert_eq!(
        after_b, after_a,
        "second install must be a 304 that downloads zero additional bytes of body"
    );

    assert_eq!(
        content_a, content_b,
        "the 304 path must still install byte-identical content"
    );
    assert_eq!(content_a, skill_md("v1"));
}

/// A conditional request that gets a fresh `200` (the server's content
/// actually changed) re-downloads and updates the recorded validator +
/// content hash, rather than reusing the stale cached entry.
#[tokio::test]
async fn conditional_200_redownloads_and_updates_recorded_hash() {
    let server = MockServer::start().await;
    let zip_v1 = build_zip("v1");
    let zip_v2 = build_zip("v2");
    let etag_v1 = "\"etag-v1\"";
    let etag_v2 = "\"etag-v2\"";

    // A conditional request carrying the *old* etag -> the server reports a
    // fresh 200 with new content and a new etag (content changed).
    Mock::given(method("GET"))
        .and(path("/pkg.zip"))
        .and(header("If-None-Match", etag_v1))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(zip_v2.clone())
                .insert_header("ETag", etag_v2),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    // Unconditional request -> the original content.
    Mock::given(method("GET"))
        .and(path("/pkg.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(zip_v1.clone())
                .insert_header("ETag", etag_v1),
        )
        .mount(&server)
        .await;

    let cache_root = tempfile::TempDir::new().unwrap();
    let url = format!("{}/pkg.zip", server.uri());

    let baseline = ZIP_URL_BODY_BYTES_DOWNLOADED.load(Ordering::SeqCst);
    let content_a = install_into_fresh_project(cache_root.path(), &url).await;
    assert_eq!(content_a, skill_md("v1"));

    let content_b = install_into_fresh_project(cache_root.path(), &url).await;
    assert_eq!(
        content_b,
        skill_md("v2"),
        "a fresh 200 on the conditional request must re-download the new content"
    );

    let downloaded = ZIP_URL_BODY_BYTES_DOWNLOADED.load(Ordering::SeqCst) - baseline;
    assert_eq!(
        downloaded,
        zip_v1.len() + zip_v2.len(),
        "both installs must have downloaded a full body"
    );

    let cache = SkillCache::at_root(cache_root.path());
    let validators = cache.read_zip_validators().unwrap();
    let recorded = validators.get(&url).expect("validator must be recorded");
    let expected_hash = fastskill_core::utils::to_hex_lower(&sha2::Sha256::digest(&zip_v2));
    assert_eq!(
        recorded.content_hash, expected_hash,
        "the recorded hash must be updated to the new content's hash"
    );
    assert_eq!(recorded.etag.as_deref(), Some(etag_v2));
}

/// A server that sends no `ETag`/`Last-Modified` at all still dedups:
/// identical archive bytes served from two different URLs are stored once in
/// the content cache (download-then-hash fallback; no fetch-time saving, but
/// still dedup -- see spec 007's design notes).
#[tokio::test]
async fn no_validator_server_still_dedups_identical_bytes_to_one_entry() {
    let server = MockServer::start().await;
    let zip_bytes = build_zip("shared");

    Mock::given(method("GET"))
        .and(path("/a.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_bytes.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/b.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_bytes.clone()))
        .mount(&server)
        .await;

    let cache_root = tempfile::TempDir::new().unwrap();

    let content_a =
        install_into_fresh_project(cache_root.path(), &format!("{}/a.zip", server.uri())).await;
    let content_b =
        install_into_fresh_project(cache_root.path(), &format!("{}/b.zip", server.uri())).await;
    assert_eq!(content_a, content_b);

    let cache = SkillCache::at_root(cache_root.path());
    let stats = cache.stats().unwrap();
    assert_eq!(
        stats.zip.entry_count, 1,
        "identical bytes from two different no-validator URLs must dedup to one stored entry"
    );

    // Two distinct URL entries in the validators index, though -- each URL's
    // own validator is recorded independently.
    let validators = cache.read_zip_validators().unwrap();
    assert_eq!(validators.len(), 2);
}

/// FR-4: a `304` whose recorded content hash is no longer in the content
/// cache (e.g. after `cache clean`) must fall back to an unconditional
/// download rather than failing.
#[tokio::test]
async fn a_304_with_evicted_content_falls_back_to_a_full_download() {
    let server = MockServer::start().await;
    let zip_bytes = build_zip("v1");
    let etag = "\"etag-v1\"";

    Mock::given(method("GET"))
        .and(path("/pkg.zip"))
        .and(header("If-None-Match", etag))
        .respond_with(ResponseTemplate::new(304))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/pkg.zip"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(zip_bytes.clone())
                .insert_header("ETag", etag),
        )
        .mount(&server)
        .await;

    let cache_root = tempfile::TempDir::new().unwrap();
    let url = format!("{}/pkg.zip", server.uri());

    // First install: real download, records the validator + publishes the
    // content cache entry.
    let content_a = install_into_fresh_project(cache_root.path(), &url).await;
    assert_eq!(content_a, skill_md("v1"));

    // Evict the content cache (mirrors `fastskill cache clean --source
    // zip`), leaving the validators index untouched.
    let cache = SkillCache::at_root(cache_root.path());
    let cleaned = cache.clean(Some(ContentSourceKind::Zip)).unwrap();
    assert_eq!(cleaned.entries_removed, 1);
    assert_eq!(cache.stats().unwrap().zip.entry_count, 0);
    assert!(
        cache.read_zip_validators().unwrap().get(&url).is_some(),
        "clean must never touch the validators index"
    );

    // Second install: the server still reports 304 for the conditional
    // request (nothing changed server-side), but the content is gone locally
    // -- must fall back to a full download rather than failing.
    let baseline = ZIP_URL_BODY_BYTES_DOWNLOADED.load(Ordering::SeqCst);
    let content_b = install_into_fresh_project(cache_root.path(), &url).await;
    assert_eq!(content_b, skill_md("v1"));
    assert_eq!(
        ZIP_URL_BODY_BYTES_DOWNLOADED.load(Ordering::SeqCst) - baseline,
        zip_bytes.len(),
        "a 304 with evicted content must fall back to a full, unconditional download"
    );
    assert_eq!(cache.stats().unwrap().zip.entry_count, 1);
}

/// FR-5: offline / transport failure with a recorded, still-cached hash
/// proceeds with a warning rather than failing (mirrors the git ref-
/// resolution fallback shipped in PR #227).
#[tokio::test]
async fn offline_install_with_a_cached_hash_succeeds() {
    // Nothing listens here; the request fails fast without touching the real
    // network.
    let unreachable_url = "http://127.0.0.1:1/pkg.zip";

    let cache_root = tempfile::TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());

    let zip_bytes = build_zip("v1");
    let content_hash = fastskill_core::utils::to_hex_lower(&sha2::Sha256::digest(&zip_bytes));

    // Prime the content cache with the previously-fetched, already-extracted
    // skill content under its recorded hash.
    let content_dir = tempfile::TempDir::new().unwrap();
    std::fs::write(content_dir.path().join("SKILL.md"), skill_md("v1")).unwrap();
    cache
        .put(
            &CacheIdentity::ZipUrl {
                content_hash: content_hash.clone(),
            },
            content_dir.path(),
        )
        .unwrap();

    // Prime the validators index as if a prior online install had recorded
    // this resolution.
    let mut validators = cache.read_zip_validators().unwrap();
    validators.insert(
        unreachable_url,
        ZipValidator {
            etag: Some("\"etag-v1\"".to_string()),
            last_modified: None,
            content_hash: content_hash.clone(),
            fetched_at: chrono::Utc::now(),
        },
    );
    cache.write_zip_validators(&validators).unwrap();

    let project = tempfile::TempDir::new().unwrap();
    setup_project(project.path());
    let storage = tempfile::TempDir::new().unwrap();
    let service = make_service(project.path(), storage.path(), cache_root.path()).await;

    let origin = Origin::ZipUrl {
        url: unreachable_url.to_string(),
    };
    let outcome = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("offline install with a cached hash should succeed");
    assert_eq!(outcome.id, SKILL_ID);

    let installed =
        std::fs::read_to_string(storage.path().join(SKILL_ID).join("SKILL.md")).unwrap();
    assert_eq!(installed, skill_md("v1"));
}

/// With no prior resolution recorded, an unreachable URL fails the install
/// (no silent staleness, no panic) -- the transport error is surfaced as-is.
#[tokio::test]
async fn offline_install_with_nothing_cached_fails_with_the_transport_error() {
    let unreachable_url = "http://127.0.0.1:1/pkg.zip";
    let cache_root = tempfile::TempDir::new().unwrap();
    let project = tempfile::TempDir::new().unwrap();
    setup_project(project.path());
    let storage = tempfile::TempDir::new().unwrap();
    let service = make_service(project.path(), storage.path(), cache_root.path()).await;

    let origin = Origin::ZipUrl {
        url: unreachable_url.to_string(),
    };
    let result = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await;
    assert!(
        result.is_err(),
        "install must fail without a cached hash to fall back to"
    );
}

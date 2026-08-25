//! Integration tests for the registry content cache (PRD 006 "Local Skill
//! Cache", US-003): registry installs resolve to a concrete version and use
//! the content cache, so a repeated install of the same pinned version --
//! even across different projects -- downloads at most once per machine, and
//! `newest` resolution never depends on a live listing call.
//!
//! The fixture is a `wiremock` `MockServer` bound to a loopback port -- this
//! is not "the network" (no DNS, no external host, nothing flaky in CI), the
//! same posture `git_content_cache_test.rs` takes with a local `git daemon`
//! for the analogous git case. "Exactly one download" is proven via
//! `core::registry::client::DOWNLOAD_INVOCATIONS`, a counting seam kept for
//! exactly this purpose (mirrors `storage::git::CLONE_INVOCATIONS`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::core::cache::{CacheIdentity, SkillCache, SourceIndex, SourceIndexEntry};
use fastskill_core::core::registry::client::{IndexEntry, DOWNLOAD_INVOCATIONS};
use fastskill_core::core::repository::{
    RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use fastskill_core::core::version::VersionConstraint;
use fastskill_core::core::{AddMode, Origin};
use fastskill_core::{FastSkillService, ServiceConfig};
use sha2::Digest;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static DOWNLOAD_COUNTER_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const SKILL_ID: &str = "widget";
const REPO_NAME: &str = "myreg";

fn skill_md(version: &str) -> String {
    format!(
        "---\nname: {SKILL_ID}\nversion: \"{version}\"\ndescription: A registry skill\n---\nBody\n"
    )
}

fn build_zip(version: &str) -> Vec<u8> {
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
        writer.write_all(skill_md(version).as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buf
}

/// Mount an index entry + download endpoint for `version` on `server`.
async fn mount_version(server: &MockServer, version: &str) {
    let zip_bytes = build_zip(version);
    let cksum = format!(
        "sha256:{}",
        fastskill_core::utils::to_hex_lower(&sha2::Sha256::digest(&zip_bytes))
    );
    let entry = IndexEntry {
        name: SKILL_ID.to_string(),
        vers: version.to_string(),
        deps: vec![],
        cksum,
        features: std::collections::HashMap::new(),
        yanked: false,
        links: None,
        download_url: format!("{}/dl/{version}", server.uri()),
        metadata: None,
    };

    Mock::given(method("GET"))
        .and(path(format!("/index/{SKILL_ID}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(serde_json::to_string(&entry).unwrap()),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/dl/{version}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_bytes))
        .mount(server)
        .await;
}

fn repo_manager(index_url: String) -> RepositoryManager {
    RepositoryManager::from_definitions(vec![RepositoryDefinition {
        name: REPO_NAME.to_string(),
        repo_type: RepositoryType::HttpRegistry,
        priority: 0,
        config: RepositoryConfig::HttpRegistry { index_url },
        auth: None,
        storage: None,
    }])
}

/// A minimal fastskill project directory: `skill-project.toml` + an empty
/// skills directory, ready for `add_from_origin`.
fn setup_project(root: &Path) -> std::path::PathBuf {
    std::fs::write(
        root.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let skills_dir = root.join(".claude/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    skills_dir
}

async fn make_service(
    project_root: &Path,
    storage: &Path,
    cache_root: &Path,
    manager: RepositoryManager,
) -> FastSkillService {
    let config = ServiceConfig {
        skill_storage_path: storage.to_path_buf(),
        skill_cache_root: Some(cache_root.to_path_buf()),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();
    service
        .with_project_root(project_root.to_path_buf())
        .with_repository_manager(Arc::new(manager))
}

// ── tests ────────────────────────────────────────────────────────────────

/// Two installs of the same pinned version, into two different projects,
/// must perform exactly one download -- the second is served from the
/// shared content cache (US-003 acceptance criterion).
#[tokio::test]
async fn two_installs_of_the_same_pinned_version_download_exactly_once() {
    let _counter_guard = DOWNLOAD_COUNTER_MUTEX.lock().await;
    let server = MockServer::start().await;
    mount_version(&server, "1.0.0").await;

    let shared_cache = TempDir::new().unwrap();
    let origin = Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: Some(VersionConstraint::parse("1.0.0").unwrap()),
    };

    let baseline = DOWNLOAD_INVOCATIONS.load(Ordering::SeqCst);

    // Project A: fresh download (cache miss).
    let project_a = TempDir::new().unwrap();
    let skills_a = setup_project(project_a.path());
    let storage_a = TempDir::new().unwrap();
    let manager_a = repo_manager(format!("{}/index", server.uri()));
    let service_a = make_service(
        project_a.path(),
        storage_a.path(),
        shared_cache.path(),
        manager_a,
    )
    .await;
    let outcome_a = service_a
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("project A install should succeed");
    assert_eq!(outcome_a.resolved.version, "1.0.0");
    let _ = skills_a;

    // Project B: same pinned version, different project -- must be a cache hit.
    let project_b = TempDir::new().unwrap();
    let skills_b = setup_project(project_b.path());
    let storage_b = TempDir::new().unwrap();
    let manager_b = repo_manager(format!("{}/index", server.uri()));
    let service_b = make_service(
        project_b.path(),
        storage_b.path(),
        shared_cache.path(),
        manager_b,
    )
    .await;
    let outcome_b = service_b
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("project B install should succeed");
    assert_eq!(outcome_b.resolved.version, "1.0.0");
    let _ = skills_b;

    let downloads = DOWNLOAD_INVOCATIONS.load(Ordering::SeqCst) - baseline;
    assert_eq!(
        downloads, 1,
        "two installs of the same pinned version must perform exactly one download"
    );

    // Both projects ended up with byte-identical installed content (FR-8).
    let content_a =
        std::fs::read_to_string(storage_a.path().join(SKILL_ID).join("SKILL.md")).unwrap();
    let content_b =
        std::fs::read_to_string(storage_b.path().join(SKILL_ID).join("SKILL.md")).unwrap();
    assert_eq!(content_a, content_b);
    assert_eq!(content_a, skill_md("1.0.0"));
}

/// A pinned version already published in the content cache installs with no
/// network at all -- the registry is unreachable and is never contacted.
#[tokio::test]
async fn offline_install_of_a_pinned_cached_version_needs_no_network() {
    let _counter_guard = DOWNLOAD_COUNTER_MUTEX.lock().await;
    // Nothing listens here; any HTTP request fails fast.
    let unreachable_index_url = "http://127.0.0.1:1/index".to_string();

    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());
    let identity = CacheIdentity::Registry {
        source: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: "1.0.0".to_string(),
    };
    let content_dir = TempDir::new().unwrap();
    std::fs::write(content_dir.path().join("SKILL.md"), skill_md("1.0.0")).unwrap();
    cache.put(&identity, content_dir.path()).unwrap();

    let project = TempDir::new().unwrap();
    let skills_dir = setup_project(project.path());
    let storage = TempDir::new().unwrap();
    let manager = repo_manager(unreachable_index_url);
    let service = make_service(project.path(), storage.path(), cache_root.path(), manager).await;

    let baseline = DOWNLOAD_INVOCATIONS.load(Ordering::SeqCst);
    let origin = Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: Some(VersionConstraint::parse("1.0.0").unwrap()),
    };
    let outcome = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("offline install of a pinned, cached version should succeed");

    assert_eq!(outcome.resolved.version, "1.0.0");
    assert_eq!(
        DOWNLOAD_INVOCATIONS.load(Ordering::SeqCst) - baseline,
        0,
        "a pinned, cached version must install without any network download"
    );
    let installed =
        std::fs::read_to_string(storage.path().join(SKILL_ID).join("SKILL.md")).unwrap();
    assert_eq!(installed, skill_md("1.0.0"));
    let _ = skills_dir;
}

/// `newest` (`version: None`) with no cached index fails with an actionable
/// error naming `repos refresh`, rather than falling back to a live listing
/// call or failing with an opaque error.
#[tokio::test]
async fn newest_without_a_fresh_index_fails_naming_repos_refresh() {
    let _counter_guard = DOWNLOAD_COUNTER_MUTEX.lock().await;
    // No mocks mounted at all: a live listing call would fail loudly (connection
    // refused / no matching route), so a passing assertion here cannot be
    // accidentally satisfied by an unintended network fallback.
    let server = MockServer::start().await;

    let project = TempDir::new().unwrap();
    setup_project(project.path());
    let storage = TempDir::new().unwrap();
    let cache_root = TempDir::new().unwrap();
    let manager = repo_manager(format!("{}/index", server.uri()));
    let service = make_service(project.path(), storage.path(), cache_root.path(), manager).await;

    let origin = Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: None,
    };
    let err = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect_err("`newest` with no cached index must fail");

    let message = err.to_string();
    assert!(
        message.contains("repos refresh"),
        "error must name `repos refresh` as the fix, got: {message}"
    );
}

/// `newest` resolves to a concrete version via the on-disk index (populated
/// as if by a prior `repos refresh`), then follows the ordinary
/// cached-content path for that resolved version.
#[tokio::test]
async fn newest_resolves_via_cached_index_then_downloads_the_resolved_version() {
    let _counter_guard = DOWNLOAD_COUNTER_MUTEX.lock().await;
    let server = MockServer::start().await;
    mount_version(&server, "2.0.0").await;

    let project = TempDir::new().unwrap();
    setup_project(project.path());
    let storage = TempDir::new().unwrap();
    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());
    cache
        .write_source_index(
            REPO_NAME,
            &SourceIndex {
                fetched_at: chrono::Utc::now(),
                entries: vec![SourceIndexEntry {
                    skill: SKILL_ID.to_string(),
                    versions: vec!["1.0.0".to_string(), "2.0.0".to_string()],
                    name: SKILL_ID.to_string(),
                    description: String::new(),
                }],
            },
        )
        .unwrap();

    let manager = repo_manager(format!("{}/index", server.uri()));
    let service = make_service(project.path(), storage.path(), cache_root.path(), manager).await;

    let origin = Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: None,
    };
    let outcome = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("newest install should resolve via the cached index and succeed");

    assert_eq!(
        outcome.resolved.version, "2.0.0",
        "must resolve to the newest version recorded in the cached index"
    );
}

/// `preflight` (called by `update` before every re-fetch, per `update.rs` /
/// `skills.rs`) persists the live listing it already fetched into the
/// on-disk index. This is what makes "`update` implicitly refreshes just the
/// sources it touches" (PRD 006 US-003, "Resolved Defaults") true: an
/// unpinned (`newest`) registry origin an `update` re-fetches must not
/// require a separate, prior `repos refresh` even though `add_from_origin`'s
/// own version resolution never calls the network directly.
#[tokio::test]
async fn update_flow_preflight_implicitly_refreshes_the_index_for_newest() {
    let _counter_guard = DOWNLOAD_COUNTER_MUTEX.lock().await;
    let server = MockServer::start().await;
    mount_version(&server, "1.0.0").await;

    let project = TempDir::new().unwrap();
    setup_project(project.path());
    let storage = TempDir::new().unwrap();
    let cache_root = TempDir::new().unwrap();
    let manager = repo_manager(format!("{}/index", server.uri()));
    let service = make_service(project.path(), storage.path(), cache_root.path(), manager).await;

    let origin = Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: None,
    };

    // No index cache exists yet -- mirrors a project whose dependency was
    // added before any `repos refresh` ever ran.
    let cache = SkillCache::at_root(cache_root.path());
    assert!(cache.read_source_index(REPO_NAME).unwrap().is_none());

    let preflight = service
        .preflight(&origin)
        .await
        .expect("preflight should succeed via its own live listing call");
    assert!(matches!(
        preflight,
        fastskill_core::core::UpdatePreflight::Updatable
    ));

    // preflight's live call must have persisted an index entry...
    let idx = cache
        .read_source_index(REPO_NAME)
        .unwrap()
        .expect("preflight must persist a listing it already fetched live");
    assert_eq!(idx.entries[0].skill, SKILL_ID);

    // ...so this `Update` re-fetch resolves `newest` through the index,
    // exactly as `update.rs` / `skills.rs` chain the two calls.
    let outcome = service
        .add_from_origin(origin, AddMode::Update, vec![])
        .await
        .expect("update re-fetch must succeed without a separate `repos refresh`");
    assert_eq!(outcome.resolved.version, "1.0.0");
}

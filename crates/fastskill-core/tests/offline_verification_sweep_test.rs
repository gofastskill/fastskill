//! Offline verification sweep (PRD 006 "Local Skill Cache", US-007): for
//! each of the three v1 cached origin kinds -- git (pinned/tag resolution),
//! registry (pinned version), and local -- populate the content cache while
//! "online" via the same real fixtures the per-story tests use (a local
//! `git daemon`, a `wiremock` HTTP registry), then make network egress to
//! the *original* source actually fail (kill the daemon; point the registry
//! client at an unreachable loopback port that nothing ever listens on,
//! `127.0.0.1:1`; delete the local source directory outright) and re-run
//! `add_from_origin` into a *different* project sharing the same on-disk
//! cache. Every one of those re-runs must succeed purely from cache and
//! produce byte-identical installed content to the original online install
//! (FR-8).
//!
//! The same suite also proves the two paths that must fail loudly instead
//! of silently serving stale data or panicking: a registry `newest`
//! resolution with no cached index (actionable error naming `repos
//! refresh`), and a git ref with no prior resolution (actionable error, not
//! a panic).
//!
//! "Network made to fail" is always either a real process that has been
//! killed (the git daemon, via `Drop`) or a fixed, reserved loopback address
//! nothing listens on (`127.0.0.1:1`) -- deterministic and fast, never
//! dependent on the environment actually lacking internet access. This
//! mirrors `git_content_cache_test.rs` / `registry_content_cache_test.rs`
//! (US-002/US-003), whose fixtures and counting seams
//! (`storage::git::CLONE_INVOCATIONS`, `core::registry::client::DOWNLOAD_INVOCATIONS`,
//! `core::install::LOCAL_COPY_INVOCATIONS`) this file reuses.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{git_command, run_git, GitDaemonFixture};
use fastskill_core::core::install::LOCAL_COPY_INVOCATIONS;
use fastskill_core::core::registry::client::{IndexEntry, DOWNLOAD_INVOCATIONS};
use fastskill_core::core::repository::{
    RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use fastskill_core::core::version::VersionConstraint;
use fastskill_core::core::{AddMode, GitRef, Origin};
use fastskill_core::storage::git::CLONE_INVOCATIONS;
use fastskill_core::{FastSkillService, ServiceConfig};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A URL nothing is listening on. Port 1 is a reserved, unprivileged-unbindable
/// port, so a connection attempt fails immediately (connection refused) rather
/// than timing out -- deterministic and fast, no dependence on the test
/// environment actually lacking internet access.
const UNREACHABLE_GIT_URL: &str = "git://127.0.0.1:1/does-not-exist.git";
const UNREACHABLE_INDEX_URL: &str = "http://127.0.0.1:1/index";

// ── shared fixtures ──────────────────────────────────────────────────────
//
// `GitDaemonFixture` (incl. `kill_and_wait`, used below to prove the daemon
// is genuinely dead before the offline re-run) and its supporting helpers
// live in `tests/common/mod.rs`, shared with `git_content_cache_test.rs` and
// `repo_index_refresh_git_test.rs`.

const GIT_SKILL_MD: &str =
    "---\nname: offline-git-skill\nversion: \"1.0.0\"\ndescription: A test skill\n---\nBody\n";

/// Seed a bare repo at `base/<repo_name>.git` with a single commit tagged
/// `tag_name`. Returns the tagged commit's SHA.
fn seed_bare_repo_with_tag(base: &Path, repo_name: &str, tag_name: &str) -> String {
    let bare_path = base.join(format!("{repo_name}.git"));
    run_git(
        base,
        &["init", "--bare", "--quiet", bare_path.to_str().unwrap()],
    );

    let work = base.join(format!("{repo_name}-work"));
    std::fs::create_dir_all(&work).unwrap();
    run_git(&work, &["init", "--quiet"]);
    run_git(&work, &["config", "user.email", "test@example.com"]);
    run_git(&work, &["config", "user.name", "test"]);
    std::fs::write(work.join("SKILL.md"), GIT_SKILL_MD).unwrap();
    run_git(&work, &["add", "-A"]);
    run_git(&work, &["commit", "--quiet", "-m", "init"]);
    run_git(&work, &["tag", tag_name]);
    run_git(&work, &["branch", "-M", "main"]);
    run_git(
        &work,
        &["remote", "add", "origin", bare_path.to_str().unwrap()],
    );
    run_git(&work, &["push", "--quiet", "origin", "main"]);
    run_git(&work, &["push", "--quiet", "origin", tag_name]);

    let output = git_command()
        .args(["rev-parse", "HEAD"])
        .current_dir(&work)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

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

async fn make_service(storage: &Path, cache_root: &Path) -> FastSkillService {
    let config = ServiceConfig {
        skill_storage_path: storage.to_path_buf(),
        skill_cache_root: Some(cache_root.to_path_buf()),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();
    service
}

async fn make_service_with_repos(
    project_root: &Path,
    storage: &Path,
    cache_root: &Path,
    manager: RepositoryManager,
) -> FastSkillService {
    make_service(storage, cache_root)
        .await
        .with_project_root(project_root.to_path_buf())
        .with_repository_manager(Arc::new(manager))
}

fn read_installed_skill_md(storage: &Path, skill_id: &str) -> String {
    std::fs::read_to_string(storage.join(skill_id).join("SKILL.md"))
        .unwrap_or_else(|e| panic!("expected installed SKILL.md for '{skill_id}': {e}"))
}

// ── US-007: git, pinned (tag) resolution, fully offline after the daemon dies ──

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn git_tag_install_succeeds_offline_after_daemon_dies_and_is_byte_identical() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let daemon_base = TempDir::new().unwrap();
    let expected_sha = seed_bare_repo_with_tag(daemon_base.path(), "repo", "v1.0.0");
    let daemon = GitDaemonFixture::start(daemon_base);
    let repo_url = daemon.repo_url("repo");

    let shared_cache = TempDir::new().unwrap();
    let origin = Origin::Git {
        url: repo_url,
        r#ref: GitRef::Tag("v1.0.0".to_string()),
        subdir: None,
    };

    let baseline_clones = CLONE_INVOCATIONS.load(Ordering::SeqCst);

    // "Online": project A installs while the daemon is alive, populating the
    // content cache and recording the tag's resolution.
    let project_a = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    std::env::set_current_dir(project_a.path()).unwrap();
    let _guard_a = fastskill_core::test_utils::DirGuard(original_dir);
    setup_project(project_a.path());
    let storage_a = TempDir::new().unwrap();
    let service_a = make_service(storage_a.path(), shared_cache.path()).await;
    let outcome_a = service_a
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("online install should succeed");
    assert_eq!(
        outcome_a.resolved.commit_hash.as_deref(),
        Some(expected_sha.as_str())
    );

    // Network egress made to fail: kill the daemon and confirm the port has
    // actually stopped accepting connections before proceeding.
    daemon.kill_and_wait();

    // "Offline": project B, same tag, same shared cache -- the daemon is
    // dead, so `ls_remote` cannot possibly succeed live; this must fall back
    // to the recorded resolution and install purely from the content cache.
    let project_b = TempDir::new().unwrap();
    std::env::set_current_dir(project_b.path()).unwrap();
    setup_project(project_b.path());
    let storage_b = TempDir::new().unwrap();
    let service_b = make_service(storage_b.path(), shared_cache.path()).await;
    let outcome_b = service_b
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("offline install from cache should succeed after the daemon is gone");
    assert_eq!(
        outcome_b.resolved.commit_hash.as_deref(),
        Some(expected_sha.as_str())
    );

    let clones_performed = CLONE_INVOCATIONS.load(Ordering::SeqCst) - baseline_clones;
    assert_eq!(
        clones_performed, 1,
        "the offline re-run must not have cloned again"
    );

    // FR-8: byte-identical installed content.
    let content_a = read_installed_skill_md(storage_a.path(), "offline-git-skill");
    let content_b = read_installed_skill_md(storage_b.path(), "offline-git-skill");
    assert_eq!(content_a, content_b);
    assert_eq!(content_a, GIT_SKILL_MD);
}

// ── US-007: registry, pinned version, fully offline against an unreachable index ──

const SKILL_ID: &str = "widget";
const REPO_NAME: &str = "myreg";

fn skill_md(version: &str) -> String {
    format!(
        "---\nname: {SKILL_ID}\nversion: \"{version}\"\ndescription: A registry skill\n---\nBody\n"
    )
}

fn build_zip(version: &str) -> Vec<u8> {
    use std::io::Write;
    use zip::write::FileOptions;
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = zip::ZipWriter::new(cursor);
        let opts = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file(format!("{SKILL_ID}/SKILL.md"), opts)
            .unwrap();
        writer.write_all(skill_md(version).as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buf
}

async fn mount_version(server: &MockServer, version: &str) {
    let zip_bytes = build_zip(version);
    use sha2::Digest;
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
        .and(wm_path(format!("/index/{SKILL_ID}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(serde_json::to_string(&entry).unwrap()),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/dl/{version}")))
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

#[tokio::test]
async fn registry_pinned_version_install_succeeds_offline_and_is_byte_identical() {
    let server = MockServer::start().await;
    mount_version(&server, "1.0.0").await;

    let shared_cache = TempDir::new().unwrap();
    let origin = Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: Some(VersionConstraint::parse("1.0.0").unwrap()),
    };

    let baseline = DOWNLOAD_INVOCATIONS.load(Ordering::SeqCst);

    // "Online": project A downloads from the real (mock) registry.
    let project_a = TempDir::new().unwrap();
    setup_project(project_a.path());
    let storage_a = TempDir::new().unwrap();
    let manager_a = repo_manager(format!("{}/index", server.uri()));
    let service_a = make_service_with_repos(
        project_a.path(),
        storage_a.path(),
        shared_cache.path(),
        manager_a,
    )
    .await;
    let outcome_a = service_a
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("online install should succeed");
    assert_eq!(outcome_a.resolved.version, "1.0.0");

    // "Offline": project B is wired to a registry index that nothing is
    // listening on -- any live call fails fast. Same shared cache root, same
    // pinned version.
    let project_b = TempDir::new().unwrap();
    setup_project(project_b.path());
    let storage_b = TempDir::new().unwrap();
    let manager_b = repo_manager(UNREACHABLE_INDEX_URL.to_string());
    let service_b = make_service_with_repos(
        project_b.path(),
        storage_b.path(),
        shared_cache.path(),
        manager_b,
    )
    .await;
    let outcome_b = service_b
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("offline install of a pinned, cached version should succeed");
    assert_eq!(outcome_b.resolved.version, "1.0.0");

    let downloads = DOWNLOAD_INVOCATIONS.load(Ordering::SeqCst) - baseline;
    assert_eq!(
        downloads, 1,
        "the offline re-run must not have downloaded again"
    );

    let content_a = read_installed_skill_md(storage_a.path(), SKILL_ID);
    let content_b = read_installed_skill_md(storage_b.path(), SKILL_ID);
    assert_eq!(content_a, content_b);
    assert_eq!(content_a, skill_md("1.0.0"));
}

// ── US-007: local, source removed entirely between the two installs ───────

const LOCAL_SKILL_ID: &str = "offline-local-skill";

fn write_local_skill(dir: &Path) {
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {LOCAL_SKILL_ID}\nversion: \"1.0.0\"\ndescription: local skill\n---\nBody\n"
        ),
    )
    .unwrap();
}

/// Local origins never touch the network at all, so there is nothing to
/// "make fail" the way git/registry have a remote to sever. The closest
/// meaningful analogue -- and the one this test proves -- is that the
/// *original source path* can vanish entirely (deleted, unmounted, a
/// different machine) once its content has been cached, and a second
/// install of byte-identical content at a *different* path still resolves
/// to the same cached identity and completes without re-copying anything
/// from a source ([`LOCAL_COPY_INVOCATIONS`] delta of zero).
#[tokio::test]
async fn local_install_succeeds_after_original_source_is_removed_and_is_byte_identical() {
    let shared_cache = TempDir::new().unwrap();

    let source_a = TempDir::new().unwrap();
    write_local_skill(source_a.path());

    let baseline = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst);

    // "Online": project A installs from `source_a`, populating the content
    // cache under `source_a`'s tree-hash.
    let project_a = TempDir::new().unwrap();
    setup_project(project_a.path());
    let storage_a = TempDir::new().unwrap();
    let service_a = make_service(storage_a.path(), shared_cache.path())
        .await
        .with_project_root(project_a.path().to_path_buf());
    let outcome_a = service_a
        .add_from_origin(
            Origin::Local {
                path: source_a.path().to_path_buf(),
                editable: false,
            },
            AddMode::Fresh,
            vec![],
        )
        .await
        .expect("online install should succeed");
    assert_eq!(outcome_a.resolved.version, "1.0.0");

    // A byte-identical copy of the source at a *different* path, made before
    // the original is removed -- the tree-hash identity depends only on
    // relative paths and content, never on where the tree lives.
    let source_b = TempDir::new().unwrap();
    write_local_skill(source_b.path());

    // The original source is now gone entirely -- project B's install must
    // not need it.
    drop(source_a);

    let project_b = TempDir::new().unwrap();
    setup_project(project_b.path());
    let storage_b = TempDir::new().unwrap();
    let service_b = make_service(storage_b.path(), shared_cache.path())
        .await
        .with_project_root(project_b.path().to_path_buf());
    let outcome_b = service_b
        .add_from_origin(
            Origin::Local {
                path: source_b.path().to_path_buf(),
                editable: false,
            },
            AddMode::Fresh,
            vec![],
        )
        .await
        .expect("second install of byte-identical content should succeed from cache");
    assert_eq!(outcome_b.resolved.version, "1.0.0");

    let copies_performed = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline;
    assert_eq!(
        copies_performed, 1,
        "the second install must have been served from the content cache, not re-copied"
    );

    let content_a = read_installed_skill_md(storage_a.path(), LOCAL_SKILL_ID);
    let content_b = read_installed_skill_md(storage_b.path(), LOCAL_SKILL_ID);
    assert_eq!(content_a, content_b);
}

// ── US-007: `update`/`newest`-resolution fail loudly instead of silently ──

/// `newest` (`version: None`) with no cached index, against an unreachable
/// registry, fails with an actionable error naming `repos refresh` -- never
/// a panic, never silently serving stale/no data.
#[tokio::test]
async fn newest_resolution_offline_with_no_index_fails_naming_repos_refresh() {
    let project = TempDir::new().unwrap();
    setup_project(project.path());
    let storage = TempDir::new().unwrap();
    let cache_root = TempDir::new().unwrap();
    let manager = repo_manager(UNREACHABLE_INDEX_URL.to_string());
    let service =
        make_service_with_repos(project.path(), storage.path(), cache_root.path(), manager).await;

    let origin = Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: SKILL_ID.to_string(),
        version: None,
    };
    let err = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect_err("`newest` offline with no cached index must fail, not panic or go stale");

    let message = err.to_string();
    assert!(
        message.contains("repos refresh"),
        "error must name `repos refresh` as the fix, got: {message}"
    );
}

/// A git branch `update` offline (unreachable remote) with no prior
/// resolution recorded fails with a real error -- not a panic, not a
/// silent no-op reusing whatever happened to be installed before.
#[tokio::test]
async fn git_branch_update_offline_with_no_prior_resolution_fails() {
    let cache_root = TempDir::new().unwrap();
    let storage = TempDir::new().unwrap();
    let service = make_service(storage.path(), cache_root.path()).await;

    let origin = Origin::Git {
        url: UNREACHABLE_GIT_URL.to_string(),
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };
    let result = service
        .add_from_origin(origin, AddMode::Update, vec![])
        .await;
    assert!(
        result.is_err(),
        "an offline git update with no prior resolution must fail, not panic or go stale"
    );
}

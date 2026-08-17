//! Integration tests for the git content cache (PRD 006 "Local Skill Cache",
//! US-002): git installs resolve their ref to a SHA and use the content
//! cache, so a repeated install of the same commit — even across different
//! projects — clones at most once per machine.
//!
//! The primary fixture is a real `git daemon` serving a bare repo over
//! `git://127.0.0.1` on a loopback port. This is not "the network": no DNS,
//! no external host, nothing that can be flaky in CI — it is the only way to
//! exercise the real `ls_remote`/`clone_repository` code paths without a
//! local filesystem path, which SEC-11 refuses outright
//! (`protocol.file.allow=never`; see `storage::git::build_clone_args`).
//! "Exactly one clone" is proven via `storage::git::CLONE_INVOCATIONS`, a
//! counting seam kept for exactly this purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{git_command, run_git, GitDaemonFixture};
use fastskill_core::core::cache::{CacheIdentity, GitResolutions, SkillCache};
use fastskill_core::core::{AddMode, GitRef, Origin};
use fastskill_core::storage::git::CLONE_INVOCATIONS;
use fastskill_core::test_utils::{DirGuard, DIR_MUTEX};
use fastskill_core::{FastSkillService, ServiceConfig};
use std::path::Path;
use std::sync::atomic::Ordering;
use tempfile::TempDir;

const VALID_SKILL_MD: &str =
    "---\nname: cached-git-skill\nversion: \"1.0.0\"\ndescription: A test skill\n---\nBody\n";

// ── git daemon fixture ──────────────────────────────────────────────────────
//
// `GitDaemonFixture` and its supporting helpers live in `tests/common/mod.rs`,
// shared with `offline_verification_sweep_test.rs` and
// `repo_index_refresh_git_test.rs`.

/// Seed a bare repo at `base/<repo_name>.git` with a single commit containing
/// `SKILL.md` on `branch`. Returns the seeded commit's SHA.
fn seed_bare_repo(base: &Path, repo_name: &str, branch: &str) -> String {
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
    std::fs::write(work.join("SKILL.md"), VALID_SKILL_MD).unwrap();
    run_git(&work, &["add", "-A"]);
    run_git(&work, &["commit", "--quiet", "-m", "init"]);
    run_git(&work, &["branch", "-M", branch]);
    run_git(
        &work,
        &["remote", "add", "origin", bare_path.to_str().unwrap()],
    );
    run_git(&work, &["push", "--quiet", "origin", branch]);

    let output = git_command()
        .args(["rev-parse", "HEAD"])
        .current_dir(&work)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

// ── project fixture ──────────────────────────────────────────────────────

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

/// Every regular file under `root`, as a `/`-normalized path relative to
/// `root`, sorted. Used to assert a cache entry's contents exactly, without
/// caring about directory-read order.
fn list_files_relative(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let rel = path.strip_prefix(base).unwrap();
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

// ── tests ────────────────────────────────────────────────────────────────

/// Two installs of the same git ref, into two different project directories,
/// must perform exactly one `git clone` — the second is served from the
/// shared content cache (PRD 006, US-002 acceptance criterion).
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn two_installs_of_the_same_ref_clone_exactly_once() {
    let _lock = DIR_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let daemon_base = TempDir::new().unwrap();
    let expected_sha = seed_bare_repo(daemon_base.path(), "repo", "main");
    let daemon = GitDaemonFixture::start(daemon_base);
    let repo_url = daemon.repo_url("repo");

    let shared_cache = TempDir::new().unwrap();
    let origin = Origin::Git {
        url: repo_url,
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };

    let baseline_clones = CLONE_INVOCATIONS.load(Ordering::SeqCst);

    // Project A: fresh clone (cache miss).
    let project_a = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    std::env::set_current_dir(project_a.path()).unwrap();
    let _guard_a = DirGuard(original_dir);
    let skills_a = setup_project(project_a.path());
    let storage_a = TempDir::new().unwrap();
    let service_a = make_service(storage_a.path(), shared_cache.path()).await;
    let outcome_a = service_a
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("project A install should succeed");
    assert_eq!(
        outcome_a.resolved.commit_hash.as_deref(),
        Some(expected_sha.as_str())
    );
    let _ = skills_a;

    // Project B: same ref, different project dir — must be a cache hit.
    let project_b = TempDir::new().unwrap();
    std::env::set_current_dir(project_b.path()).unwrap();
    let skills_b = setup_project(project_b.path());
    let storage_b = TempDir::new().unwrap();
    let service_b = make_service(storage_b.path(), shared_cache.path()).await;
    let outcome_b = service_b
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("project B install should succeed");
    assert_eq!(
        outcome_b.resolved.commit_hash.as_deref(),
        Some(expected_sha.as_str())
    );
    let _ = skills_b;

    let clones_performed = CLONE_INVOCATIONS.load(Ordering::SeqCst) - baseline_clones;
    assert_eq!(
        clones_performed, 1,
        "two installs of the same ref must perform exactly one clone"
    );

    // Both projects ended up with byte-identical installed content (FR-8).
    let content_a =
        std::fs::read_to_string(storage_a.path().join("cached-git-skill").join("SKILL.md"))
            .unwrap();
    let content_b =
        std::fs::read_to_string(storage_b.path().join("cached-git-skill").join("SKILL.md"))
            .unwrap();
    assert_eq!(content_a, content_b);
    assert_eq!(content_a, VALID_SKILL_MD);
}

/// Bugfix regression (cache bloat): the content cache must store only skill
/// files, never the clone's own `.git` metadata. `.git` is only ever read
/// transiently, to resolve the commit the clone just landed on
/// (`git rev-parse HEAD`) — a cache entry is only ever copied back out as
/// skill *files* (never re-treated as a git repository), so caching `.git`
/// is pure bloat that scales with the source repo's full history, not the
/// (typically tiny) skill payload it actually serves. Also proves a
/// cache-hit install still reproduces byte-identical skill content to the
/// original clone-path install, and that the cache-hit's own on-disk footprint
/// (what `cache info` sizes) matches the pared-down entry, not a `.git`-laden
/// one.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn git_cache_entry_excludes_git_metadata_and_stays_byte_identical_on_a_hit() {
    let _lock = DIR_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let daemon_base = TempDir::new().unwrap();
    let expected_sha = seed_bare_repo(daemon_base.path(), "repo", "main");
    let daemon = GitDaemonFixture::start(daemon_base);
    let repo_url = daemon.repo_url("repo");

    let shared_cache = TempDir::new().unwrap();
    let origin = Origin::Git {
        url: repo_url,
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };

    // Project A: fresh clone (cache miss) — exercises the `fetch_git` "miss"
    // branch that strips `.git` before `cache.put`.
    let project_a = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    std::env::set_current_dir(project_a.path()).unwrap();
    let _guard_a = DirGuard(original_dir);
    setup_project(project_a.path());
    let storage_a = TempDir::new().unwrap();
    let service_a = make_service(storage_a.path(), shared_cache.path()).await;
    service_a
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("project A install should succeed");

    // Inspect the published cache entry directly: it must hold exactly the
    // skill's files, nothing named `.git` anywhere inside it.
    let cache = SkillCache::at_root(shared_cache.path());
    let cached = cache
        .get(&CacheIdentity::Git {
            sha: expected_sha.clone(),
        })
        .expect("cache entry should exist after a miss+put");

    assert!(
        !cached.path.join(".git").exists(),
        "published cache entry must not contain a .git directory"
    );
    let entry_files = list_files_relative(&cached.path);
    assert_eq!(
        entry_files,
        vec!["SKILL.md".to_string()],
        "cache entry must contain exactly the skill's files, no git metadata"
    );

    // Project B: same ref, different project directory — must be a cache hit
    // (exercises the `fetch_git` "hit" branch's `copy_dir_recursive`).
    let project_b = TempDir::new().unwrap();
    std::env::set_current_dir(project_b.path()).unwrap();
    setup_project(project_b.path());
    let storage_b = TempDir::new().unwrap();
    let service_b = make_service(storage_b.path(), shared_cache.path()).await;
    service_b
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("project B (cache-hit) install should succeed");

    // Byte-identical skill content between the clone-path and cache-hit-path
    // installs (FR-8): read the same file both ways and compare raw bytes.
    let content_a =
        std::fs::read(storage_a.path().join("cached-git-skill").join("SKILL.md")).unwrap();
    let content_b =
        std::fs::read(storage_b.path().join("cached-git-skill").join("SKILL.md")).unwrap();
    assert_eq!(
        content_a, content_b,
        "a cache-hit install must be byte-identical to the original clone-path install"
    );
    assert_eq!(content_a, VALID_SKILL_MD.as_bytes());

    // The cache-hit install must not have materialized a `.git` either,
    // proving the cache entry it copied from was itself already clean.
    assert!(
        !storage_b
            .path()
            .join("cached-git-skill")
            .join(".git")
            .exists(),
        "a cache-hit install must not carry a .git directory into the installed skill"
    );
}

/// When `ls_remote` fails (offline / unreachable remote) but a previous
/// resolution for the same `url`+`ref` is recorded in the index cache, install
/// proceeds from the cached content with a warning rather than failing.
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn offline_install_falls_back_to_previously_resolved_sha() {
    let _lock = DIR_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    // A URL nothing is listening on (port 1 is reserved/unprivileged-unbindable),
    // so `ls_remote` fails fast without ever touching the real network.
    let unreachable_url = "git://127.0.0.1:1/does-not-exist.git";
    let sha = "1111111111111111111111111111111111111a";

    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());

    // Prime the content cache with the "previously fetched" skill content.
    let content_dir = TempDir::new().unwrap();
    std::fs::write(content_dir.path().join("SKILL.md"), VALID_SKILL_MD).unwrap();
    cache
        .put(
            &CacheIdentity::Git {
                sha: sha.to_string(),
            },
            content_dir.path(),
        )
        .unwrap();

    // Prime the git-resolutions index as if a prior online install had
    // resolved this exact url+ref. The key format (`branch:<name>`) mirrors
    // `install.rs`'s private `git_ref_cache_key` encoding.
    let mut resolutions = GitResolutions::default();
    resolutions.insert(
        unreachable_url,
        "branch:main",
        sha.to_string(),
        chrono::Utc::now(),
    );
    cache.write_git_resolutions(&resolutions).unwrap();

    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    std::env::set_current_dir(project.path()).unwrap();
    let _guard = DirGuard(original_dir);
    setup_project(project.path());
    let storage = TempDir::new().unwrap();
    let service = make_service(storage.path(), cache_root.path()).await;

    let origin = Origin::Git {
        url: unreachable_url.to_string(),
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };
    let outcome = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("offline install with a cached resolution should succeed");

    assert_eq!(outcome.resolved.commit_hash.as_deref(), Some(sha));
    let installed =
        std::fs::read_to_string(storage.path().join("cached-git-skill").join("SKILL.md")).unwrap();
    assert_eq!(installed, VALID_SKILL_MD);
}

/// With no prior resolution recorded, an unreachable remote fails the install
/// (no silent staleness, no panic).
#[tokio::test]
async fn offline_install_with_no_prior_resolution_fails() {
    let unreachable_url = "git://127.0.0.1:1/does-not-exist.git";
    let cache_root = TempDir::new().unwrap();
    let storage = TempDir::new().unwrap();
    let service = make_service(storage.path(), cache_root.path()).await;

    let origin = Origin::Git {
        url: unreachable_url.to_string(),
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };
    let result = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await;
    assert!(
        result.is_err(),
        "install must fail without a cached resolution"
    );
}

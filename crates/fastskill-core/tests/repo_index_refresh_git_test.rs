//! Integration test for git-source index refresh (PRD 006 "Local Skill
//! Cache", US-005): `RepositoryManager::refresh_index` resolves a
//! `GitMarketplace` repository's configured branch to its current commit SHA
//! via `ls_remote` and records it in the git-resolutions index cache.
//!
//! Mirrors `git_content_cache_test.rs`'s fixture (US-002): a real `git
//! daemon` serving a bare repo over `git://127.0.0.1` on a loopback port —
//! not "the network", nothing DNS- or host-dependent, the only way to
//! exercise the real `ls_remote` path without a `file://` URL (SEC-11 refuses
//! those outright).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::core::cache::SkillCache;
use fastskill_core::core::repository::{
    RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const SKILL_MD: &str =
    "---\nname: repo-skill\nversion: \"1.0.0\"\ndescription: A test skill\n---\nBody\n";

// ── git daemon fixture (mirrors git_content_cache_test.rs) ─────────────────

struct GitDaemonFixture {
    child: Child,
    port: u16,
    _base: TempDir,
}

impl GitDaemonFixture {
    fn start(base: TempDir) -> Self {
        // `free_loopback_port` can only *suggest* a port: it binds :0, reads the
        // number, then drops the listener so `git daemon` can take it. Under a
        // parallel test run another process can win that gap, and the daemon
        // then dies immediately with "address already in use". Retry on a fresh
        // port instead of failing the test for an unlucky race.
        const MAX_ATTEMPTS: u32 = 10;
        for attempt in 1..=MAX_ATTEMPTS {
            let port = free_loopback_port();
            let mut child = git_command()
                .args([
                    "daemon",
                    "--reuseaddr",
                    "--export-all",
                    "--listen=127.0.0.1",
                    &format!("--port={port}"),
                    &format!("--base-path={}", base.path().display()),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to start `git daemon`; is git installed with daemon support?");

            if wait_for_port(port, &mut child) {
                return Self {
                    child,
                    port,
                    _base: base,
                };
            }

            // Did not come up: reap this attempt before trying another port.
            let _ = child.kill();
            let _ = child.wait();
            assert!(
                attempt < MAX_ATTEMPTS,
                "git daemon failed to start listening after {MAX_ATTEMPTS} attempts"
            );
        }
        unreachable!("loop either returns or asserts on the final attempt")
    }

    fn repo_url(&self, repo_name: &str) -> String {
        format!("git://127.0.0.1:{}/{repo_name}", self.port)
    }
}

impl Drop for GitDaemonFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `git` exports these to its own subprocesses (hooks, `rebase --exec`) to pin
/// them to the invoking repository. A test run *from* such a context — most
/// obviously this repo's own `pre-push` hook — would otherwise inherit them,
/// and every `git` below would retarget the real repo instead of the scratch
/// one, firing the real hooks. Always spawn git through this.
fn git_command() -> Command {
    let mut cmd = Command::new("git");
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_PREFIX",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

fn free_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind ephemeral port");
    listener
        .local_addr()
        .expect("failed to read local addr")
        .port()
}

/// Poll until `port` accepts connections. Returns `false` (rather than
/// panicking) if the daemon exits first or the deadline passes, so the caller
/// can retry on a different port.
fn wait_for_port(port: u16, child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        // A daemon that already exited is never going to listen -- fail this
        // attempt immediately instead of burning the whole deadline.
        if matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run_git(dir: &Path, args: &[&str]) {
    let output = git_command()
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to execute `git {}`: {e}", args.join(" ")));
    assert!(
        output.status.success(),
        "`git {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Seed a bare repo at `base/<repo_name>.git` with a single commit on
/// `branch`, including a root `.claude-plugin/marketplace.json` so the repo
/// is also listable as a marketplace source. Returns the seeded commit's SHA.
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
    std::fs::write(work.join("SKILL.md"), SKILL_MD).unwrap();
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

// ── tests ────────────────────────────────────────────────────────────────

/// Refreshing a `GitMarketplace` source resolves its configured branch to the
/// remote's current commit SHA and records that resolution in the
/// git-resolutions index — the same cache `install::fetch_git` reads for
/// offline resolution (US-002's acceptance criterion this unblocks) — even
/// though the fixture repo has no marketplace.json (so the listing half of
/// the refresh fails: `list_skills()` fetches marketplace.json over HTTP(S),
/// a completely separate path from the git-protocol `ls_remote` this test
/// exercises). That the resolution is recorded regardless proves the two
/// steps are independent, per `refresh_index`'s doc comment.
#[tokio::test]
async fn refresh_index_records_git_ref_resolution() {
    let daemon_base = TempDir::new().unwrap();
    let expected_sha = seed_bare_repo(daemon_base.path(), "repo", "main");
    let daemon = GitDaemonFixture::start(daemon_base);
    let repo_url = daemon.repo_url("repo");

    let mut manager = RepositoryManager::new(std::path::PathBuf::new());
    manager
        .add_repository(
            "git-src".to_string(),
            RepositoryDefinition {
                name: "git-src".to_string(),
                repo_type: RepositoryType::GitMarketplace,
                priority: 0,
                config: RepositoryConfig::GitMarketplace {
                    url: repo_url.clone(),
                    branch: Some("main".to_string()),
                    tag: None,
                },
                auth: None,
                storage: None,
            },
        )
        .unwrap();

    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());

    let result = manager.refresh_index(&cache, "git-src").await;
    assert!(
        result.is_err(),
        "the listing step must fail: nothing serves marketplace.json over HTTP for this fixture"
    );

    let resolutions = cache.read_git_resolutions().unwrap();
    let resolution = resolutions
        .get(&repo_url, "branch:main")
        .expect("ls_remote resolution must be recorded under url + branch:main");
    assert_eq!(resolution.sha, expected_sha);

    // Index-only per RFQ 004: no content is fetched or cached by a refresh.
    assert!(!cache_root.path().join("git").join(&expected_sha).exists());
}

/// An unreachable git remote fails the refresh with a clear error rather than
/// silently skipping the ref-resolution step.
#[tokio::test]
async fn refresh_index_fails_when_remote_is_unreachable() {
    let unreachable_url = "git://127.0.0.1:1/does-not-exist.git";

    let mut manager = RepositoryManager::new(std::path::PathBuf::new());
    manager
        .add_repository(
            "unreachable".to_string(),
            RepositoryDefinition {
                name: "unreachable".to_string(),
                repo_type: RepositoryType::GitMarketplace,
                priority: 0,
                config: RepositoryConfig::GitMarketplace {
                    url: unreachable_url.to_string(),
                    branch: Some("main".to_string()),
                    tag: None,
                },
                auth: None,
                storage: None,
            },
        )
        .unwrap();

    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());

    let result = manager.refresh_index(&cache, "unreachable").await;
    assert!(result.is_err(), "refresh must fail, not silently succeed");
    assert!(cache.read_source_index("unreachable").unwrap().is_none());
}

//! Integration tests for `storage::git_isolated::clone_isolated` against a
//! real `git daemon` on a loopback port (see `tests/common/mod.rs`). The
//! daemon speaks `git://`, so these tests allow that transport explicitly;
//! the default (HTTPS only) is proven to refuse it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{git_command, run_git, GitDaemonFixture};
use fastskill_core::storage::git_isolated::{clone_isolated, IsolatedCloneOptions};
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

/// Seed `base/<name>.git` with one commit on `main`, its default branch,
/// holding what `write` writes. Returns the commit's object ID.
fn seed(base: &Path, name: &str, write: impl FnOnce(&Path)) -> String {
    let bare = base.join(format!("{name}.git"));
    run_git(base, &["init", "--bare", "--quiet", bare.to_str().unwrap()]);
    run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let work = base.join(format!("{name}-work"));
    std::fs::create_dir_all(&work).unwrap();
    run_git(&work, &["init", "--quiet"]);
    run_git(&work, &["config", "user.email", "test@example.com"]);
    run_git(&work, &["config", "user.name", "test"]);
    write(&work);
    run_git(&work, &["add", "-A"]);
    run_git(&work, &["commit", "--quiet", "-m", "init"]);
    run_git(&work, &["branch", "-M", "main"]);
    run_git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    run_git(&work, &["push", "--quiet", "origin", "main"]);
    let output = git_command()
        .args(["rev-parse", "HEAD"])
        .current_dir(&work)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn skill(work: &Path) {
    std::fs::create_dir_all(work.join("skills/demo")).unwrap();
    std::fs::write(
        work.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: A demo skill\n---\nBody\n",
    )
    .unwrap();
}

fn over_git(parent: &Path) -> IsolatedCloneOptions {
    IsolatedCloneOptions {
        allowed_protocols: vec!["git".to_string()],
        parent_dir: Some(parent.to_path_buf()),
        ..IsolatedCloneOptions::default()
    }
}

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

#[tokio::test]
async fn clones_the_tree_at_one_commit_without_git_metadata() {
    let base = TempDir::new().unwrap();
    let commit = seed(base.path(), "repo", skill);
    let daemon = GitDaemonFixture::start(base);
    let parent = TempDir::new().unwrap();

    let clone = clone_isolated(
        &daemon.repo_url("repo"),
        Some("main"),
        &over_git(parent.path()),
    )
    .await
    .unwrap();

    assert_eq!(clone.commit(), commit);
    assert!(clone.path().join("skills/demo/SKILL.md").is_file());
    assert!(!clone.path().join(".git").exists());
    assert!(clone.path().starts_with(parent.path()));
    drop(clone);
    assert!(is_empty(parent.path()), "the clone is removed on drop");
}

#[tokio::test]
async fn clones_the_default_branch_when_none_is_named() {
    let base = TempDir::new().unwrap();
    let commit = seed(base.path(), "default", skill);
    let daemon = GitDaemonFixture::start(base);
    let parent = TempDir::new().unwrap();

    let clone = clone_isolated(&daemon.repo_url("default"), None, &over_git(parent.path()))
        .await
        .unwrap();
    assert_eq!(clone.commit(), commit);
}

#[tokio::test]
async fn the_default_options_refuse_every_transport_but_https() {
    let base = TempDir::new().unwrap();
    seed(base.path(), "plain", skill);
    let daemon = GitDaemonFixture::start(base);
    let parent = TempDir::new().unwrap();
    let options = IsolatedCloneOptions {
        parent_dir: Some(parent.path().to_path_buf()),
        ..IsolatedCloneOptions::default()
    };

    let error = clone_isolated(&daemon.repo_url("plain"), None, &options)
        .await
        .expect_err("git:// is not allowed by default");
    assert!(error.to_string().contains("Failed to clone"), "{error}");
    assert!(is_empty(parent.path()), "nothing is left on disk");
}

#[tokio::test]
async fn a_missing_branch_fails_and_leaves_nothing() {
    let base = TempDir::new().unwrap();
    seed(base.path(), "branchless", skill);
    let daemon = GitDaemonFixture::start(base);
    let parent = TempDir::new().unwrap();

    let error = clone_isolated(
        &daemon.repo_url("branchless"),
        Some("no-such-branch"),
        &over_git(parent.path()),
    )
    .await
    .expect_err("the branch does not exist");
    assert!(error.to_string().contains("Failed to clone"), "{error}");
    assert!(is_empty(parent.path()));
}

#[tokio::test]
async fn a_clone_over_the_byte_cap_is_stopped_and_removed() {
    let base = TempDir::new().unwrap();
    seed(base.path(), "big", |work| {
        // Incompressible, so the pack is as large as the file.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let noise: Vec<u8> = (0..(2 << 20))
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        std::fs::write(work.join("noise.bin"), noise).unwrap();
    });
    let daemon = GitDaemonFixture::start(base);
    let parent = TempDir::new().unwrap();
    let options = IsolatedCloneOptions {
        max_bytes: 256 * 1024,
        ..over_git(parent.path())
    };

    let error = clone_isolated(&daemon.repo_url("big"), None, &options)
        .await
        .expect_err("the repository is larger than the cap");
    assert!(error.to_string().contains("bytes"), "{error}");
    assert!(is_empty(parent.path()));
}

#[tokio::test]
async fn a_clone_over_the_file_cap_is_refused() {
    let base = TempDir::new().unwrap();
    seed(base.path(), "many", |work| {
        for i in 0..200 {
            std::fs::write(work.join(format!("f{i}.txt")), format!("{i}")).unwrap();
        }
    });
    let daemon = GitDaemonFixture::start(base);
    let parent = TempDir::new().unwrap();
    let options = IsolatedCloneOptions {
        max_files: 100,
        ..over_git(parent.path())
    };

    let error = clone_isolated(&daemon.repo_url("many"), None, &options)
        .await
        .expect_err("the repository holds more files than the cap");
    assert!(error.to_string().contains("files"), "{error}");
    assert!(is_empty(parent.path()));
}

#[tokio::test]
async fn a_server_that_never_answers_hits_the_deadline() {
    // Accepts the connection and then says nothing.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let parent = TempDir::new().unwrap();
    let options = IsolatedCloneOptions {
        timeout: Duration::from_secs(2),
        ..over_git(parent.path())
    };

    let started = std::time::Instant::now();
    let error = clone_isolated(&format!("git://127.0.0.1:{port}/silent"), None, &options)
        .await
        .expect_err("the server never answers");
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(is_empty(parent.path()));
    drop(listener);
}

#[cfg(unix)]
#[tokio::test]
async fn symbolic_links_stay_links_for_the_caller_to_judge() {
    let base = TempDir::new().unwrap();
    seed(base.path(), "links", |work| {
        skill(work);
        std::os::unix::fs::symlink("/etc/passwd", work.join("skills/demo/escape")).unwrap();
    });
    let daemon = GitDaemonFixture::start(base);
    let parent = TempDir::new().unwrap();

    let clone = clone_isolated(&daemon.repo_url("links"), None, &over_git(parent.path()))
        .await
        .unwrap();
    let escape = clone.path().join("skills/demo/escape");
    assert!(std::fs::symlink_metadata(&escape)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(
        fastskill_core::core::content_digest::content_digest(&clone.path().join("skills/demo"))
            .is_err(),
        "the content digest refuses the link"
    );
}

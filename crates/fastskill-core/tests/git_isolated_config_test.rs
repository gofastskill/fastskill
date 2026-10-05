//! `clone_isolated` ignores the host's git configuration. Its own binary,
//! because the test sets process environment variables, which no other test
//! may race with.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{git_command, run_git, GitDaemonFixture};
use fastskill_core::storage::git_isolated::{clone_isolated, IsolatedCloneOptions};
use tempfile::TempDir;

fn seed(base: &std::path::Path, name: &str) {
    let bare = base.join(format!("{name}.git"));
    run_git(base, &["init", "--bare", "--quiet", bare.to_str().unwrap()]);
    run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let work = base.join(format!("{name}-work"));
    std::fs::create_dir_all(&work).unwrap();
    run_git(&work, &["init", "--quiet"]);
    run_git(&work, &["config", "user.email", "test@example.com"]);
    run_git(&work, &["config", "user.name", "test"]);
    std::fs::write(
        work.join("SKILL.md"),
        "---\nname: real\ndescription: d\n---\n",
    )
    .unwrap();
    run_git(&work, &["add", "-A"]);
    run_git(&work, &["commit", "--quiet", "-m", "init"]);
    run_git(&work, &["branch", "-M", "main"]);
    run_git(&work, &["push", "--quiet", bare.to_str().unwrap(), "main"]);
}

#[tokio::test]
async fn host_git_configuration_does_not_apply() {
    let base = TempDir::new().unwrap();
    seed(base.path(), "real");
    let daemon = GitDaemonFixture::start(base);
    let real = daemon.repo_url("real");
    let decoy = daemon.repo_url("decoy-that-does-not-exist");

    // Configuration from the environment, as a host could carry it: rewrite
    // the decoy URL to the real repository.
    std::env::set_var("GIT_CONFIG_COUNT", "1");
    std::env::set_var("GIT_CONFIG_KEY_0", format!("url.{real}.insteadOf"));
    std::env::set_var("GIT_CONFIG_VALUE_0", &decoy);

    // Control: an ordinary git honours it.
    let control = TempDir::new().unwrap();
    let dest = control.path().join("clone");
    let status = git_command()
        .args(["clone", "--quiet", &decoy, dest.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success(), "the host configuration rewrites the URL");

    let parent = TempDir::new().unwrap();
    let result = clone_isolated(
        &decoy,
        None,
        &IsolatedCloneOptions {
            allowed_protocols: vec!["git".to_string()],
            parent_dir: Some(parent.path().to_path_buf()),
            ..IsolatedCloneOptions::default()
        },
    )
    .await;

    std::env::remove_var("GIT_CONFIG_COUNT");
    std::env::remove_var("GIT_CONFIG_KEY_0");
    std::env::remove_var("GIT_CONFIG_VALUE_0");
    assert!(result.is_err(), "the isolated clone ignores it");
}

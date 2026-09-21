//! Integration tests for listing a git source's marketplace catalog.
//!
//! A git source's `marketplace.json` is read from a shallow clone -- the
//! same `git clone` path skill installs use -- rather than an anonymous HTTP
//! GET of a `raw.githubusercontent.com` URL, which 404'd for private repos,
//! mangled SSH remotes, ignored a configured tag, and assumed a `main`
//! default branch. These tests serve bare repos from a local `git daemon`
//! (see `tests/common/mod.rs`), so they exercise the real clone path with no
//! HTTP server anywhere.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{run_git, GitDaemonFixture};
use fastskill_core::core::sources::{SourceConfig, SourcesManager};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A Claude Code catalog listing one skill, which lives at `skills/<dir>/`.
fn catalog(dir: &str) -> String {
    serde_json::json!({
        "name": "fixture",
        "metadata": {"version": "1.0.0"},
        "plugins": [{
            "name": "pack",
            "description": "fixture plugin",
            "source": "./",
            "skills": [format!("./skills/{dir}")]
        }]
    })
    .to_string()
}

/// A catalog at `catalog_path` listing a skill that lives at `skills/<dir>/`,
/// together with that skill's own files declaring `id`.
///
/// ADR-0014: the listing reads each skill's id and version from its own
/// `skill-project.toml`/`SKILL.md` at the cloned commit, so a catalog is only
/// a pointer -- a fixture that lists a path holding no skill describes a
/// repository that does not exist, and is now rejected as such.
fn repo_files(catalog_path: &str, dir: &str, id: &str) -> Vec<(String, String)> {
    vec![
        (catalog_path.to_string(), catalog(dir)),
        (
            format!("skills/{dir}/SKILL.md"),
            format!("---\nname: {id}\ndescription: {id} skill\nversion: 1.0.0\n---\n\n# {id}\n"),
        ),
        (
            format!("skills/{dir}/skill-project.toml"),
            format!("[metadata]\nid = \"{id}\"\nversion = \"1.0.0\"\n"),
        ),
    ]
}

/// Work tree for `repo_name` under `base`, pushing to a bare repo at
/// `base/<repo_name>.git` whose `HEAD` (the remote default branch) is
/// `default_branch`.
struct SeededRepo {
    work: PathBuf,
}

impl SeededRepo {
    fn init(base: &Path, repo_name: &str, default_branch: &str) -> Self {
        let bare = base.join(format!("{repo_name}.git"));
        run_git(base, &["init", "--bare", "--quiet", bare.to_str().unwrap()]);
        run_git(
            &bare,
            &[
                "symbolic-ref",
                "HEAD",
                &format!("refs/heads/{default_branch}"),
            ],
        );

        let work = base.join(format!("{repo_name}-work"));
        std::fs::create_dir_all(&work).unwrap();
        run_git(&work, &["init", "--quiet"]);
        run_git(&work, &["config", "user.email", "test@example.com"]);
        run_git(&work, &["config", "user.name", "test"]);
        run_git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        run_git(&work, &["checkout", "--quiet", "-b", default_branch]);
        Self { work }
    }

    /// Commit `files` (path, contents) on the current branch and push it.
    fn commit<S: AsRef<str>>(&self, files: &[(S, S)], branch: &str) {
        for (relative, contents) in files {
            let path = self.work.join(relative.as_ref());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents.as_ref()).unwrap();
        }
        run_git(&self.work, &["add", "-A"]);
        run_git(&self.work, &["commit", "--quiet", "-m", "update"]);
        run_git(&self.work, &["push", "--quiet", "origin", branch]);
    }

    fn tag(&self, name: &str) {
        run_git(&self.work, &["tag", name]);
        run_git(&self.work, &["push", "--quiet", "origin", name]);
    }
}

fn manager_with(url: &str, branch: Option<&str>, tag: Option<&str>) -> SourcesManager {
    let mut manager = SourcesManager::new(PathBuf::from("unused"));
    manager
        .add_source(
            "git-src".to_string(),
            SourceConfig::Git {
                url: url.to_string(),
                branch: branch.map(str::to_string),
                tag: tag.map(str::to_string),
                auth: None,
            },
        )
        .unwrap();
    manager
}

fn listed_ids(marketplace: &fastskill_core::core::sources::MarketplaceJson) -> Vec<&str> {
    marketplace.skills.iter().map(|s| s.id.as_str()).collect()
}

#[tokio::test]
async fn lists_catalog_from_claude_plugin_location_on_remote_default_branch() {
    let base = TempDir::new().unwrap();
    // A default branch that is not `main`: the listing used to assume `main`.
    let repo = SeededRepo::init(base.path(), "repo", "trunk");
    // The folder names deliberately differ from the declared ids: identity
    // comes from each skill's own files, never from the catalog's paths.
    let mut files = repo_files(
        ".claude-plugin/marketplace.json",
        "preferred-dir",
        "preferred",
    );
    files.extend(repo_files("marketplace.json", "root-dir", "root-fallback"));
    repo.commit(&files, "trunk");
    let daemon = GitDaemonFixture::start(base);

    let manager = manager_with(&daemon.repo_url("repo"), None, None);
    let marketplace = manager.get_marketplace_json("git-src").await.unwrap();
    assert_eq!(listed_ids(&marketplace), vec!["preferred"]);

    let skills = manager.get_available_skills().await.unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].id, "preferred");
    assert_eq!(skills[0].source_name, "git-src");
}

#[tokio::test]
async fn falls_back_to_root_catalog_and_honors_branch() {
    let base = TempDir::new().unwrap();
    let repo = SeededRepo::init(base.path(), "repo", "main");
    repo.commit(
        &repo_files("marketplace.json", "on-main", "on-main"),
        "main",
    );
    run_git(&repo.work, &["checkout", "--quiet", "-b", "release"]);
    repo.commit(
        &repo_files("marketplace.json", "on-release", "on-release"),
        "release",
    );
    let daemon = GitDaemonFixture::start(base);

    let manager = manager_with(&daemon.repo_url("repo"), Some("release"), None);
    let marketplace = manager.get_marketplace_json("git-src").await.unwrap();
    assert_eq!(listed_ids(&marketplace), vec!["on-release"]);
}

#[tokio::test]
async fn honors_configured_tag() {
    let base = TempDir::new().unwrap();
    let repo = SeededRepo::init(base.path(), "repo", "main");
    repo.commit(&repo_files("marketplace.json", "tagged", "tagged"), "main");
    repo.tag("v1.0.0");
    repo.commit(
        &repo_files("marketplace.json", "after-tag", "after-tag"),
        "main",
    );
    let daemon = GitDaemonFixture::start(base);

    let url = daemon.repo_url("repo");
    let manager = manager_with(&url, None, Some("v1.0.0"));
    let marketplace = manager.get_marketplace_json("git-src").await.unwrap();
    assert_eq!(listed_ids(&marketplace), vec!["tagged"]);
    // A non-GitHub host has no ref in its links, and `./` segments are dropped.
    assert_eq!(
        marketplace.skills[0].download_url,
        Some(format!("{url}/skills/tagged"))
    );
}

#[tokio::test]
async fn repo_without_catalog_reports_missing_catalog_not_http_404() {
    let base = TempDir::new().unwrap();
    let repo = SeededRepo::init(base.path(), "repo", "main");
    repo.commit(&[("README.md", "no catalog here")], "main");
    let daemon = GitDaemonFixture::start(base);

    let manager = manager_with(&daemon.repo_url("repo"), None, None);
    let err = manager
        .get_marketplace_json("git-src")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("has no marketplace catalog"), "{err}");
    assert!(err.contains(".claude-plugin/marketplace.json"), "{err}");
    assert!(!err.contains("HTTP"), "{err}");
}

#[tokio::test]
async fn unreachable_repo_surfaces_the_git_error() {
    let base = TempDir::new().unwrap();
    let daemon = GitDaemonFixture::start(base);

    let manager = manager_with(&daemon.repo_url("missing"), None, None);
    let err = manager
        .get_marketplace_json("git-src")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Failed to read marketplace.json"), "{err}");
    assert!(err.contains("missing"), "{err}");
}

#[tokio::test]
async fn second_listing_is_served_from_memory_without_cloning_again() {
    let base = TempDir::new().unwrap();
    let repo = SeededRepo::init(base.path(), "repo", "main");
    repo.commit(&repo_files("marketplace.json", "cached", "cached"), "main");
    let daemon = GitDaemonFixture::start(base);

    let manager = manager_with(&daemon.repo_url("repo"), None, None);
    manager.get_marketplace_json("git-src").await.unwrap();
    // With the daemon gone a second clone would fail outright, so a
    // successful second read proves it came from the in-memory cache.
    daemon.kill_and_wait();
    let cached = manager.get_marketplace_json("git-src").await.unwrap();
    assert_eq!(listed_ids(&cached), vec!["cached"]);
}

/// A catalog entry pointing at a path that holds no skill -- exactly what
/// `marketplace create` produced before it recorded real paths -- names the
/// problem instead of listing a skill that install would then fail to find.
#[tokio::test]
async fn entry_pointing_at_no_skill_is_reported_against_the_catalog() {
    let base = TempDir::new().unwrap();
    let repo = SeededRepo::init(base.path(), "repo", "main");
    repo.commit(&[("marketplace.json", &catalog("moved-away"))], "main");
    let daemon = GitDaemonFixture::start(base);

    let manager = manager_with(&daemon.repo_url("repo"), None, None);
    let err = manager
        .get_marketplace_json("git-src")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not name a skill"), "{err}");
    assert!(err.contains("./skills/moved-away"), "{err}");
}

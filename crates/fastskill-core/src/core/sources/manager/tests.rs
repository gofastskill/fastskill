//! Unit tests for [`super`] (`SourcesManager`).
//!
//! Split out of `manager.rs` to keep that file under the 1000-line
//! source-size gate (`scripts/check-source-size.sh`), which excludes
//! `tests.rs` by name. Same pattern as `core/cache/tests.rs`.

use super::*;

fn pat_auth() -> Option<SourceAuth> {
    Some(SourceAuth::Pat {
        env_var: "EXAMPLE_TOKEN".to_string(),
    })
}

fn zip_source_with_auth() -> SourceConfig {
    SourceConfig::ZipUrl {
        base_url: "http://127.0.0.1:1/skills.zip".to_string(),
        auth: pat_auth(),
    }
}

fn zip_source_without_auth() -> SourceConfig {
    SourceConfig::ZipUrl {
        base_url: "http://127.0.0.1:1/skills.zip".to_string(),
        auth: None,
    }
}

#[test]
fn reject_configured_zip_url_auth_rejects_when_configured() {
    let err = reject_configured_zip_url_auth("my-zip-source", &pat_auth())
        .expect_err("configured auth on a zip-url source must be rejected");
    assert_eq!(
        err.to_string(),
        "Zip URL error: Source 'my-zip-source' has `auth` configured, but zip-url sources \
         fetch via a plain HTTP GET and do not support an `auth` block -- fastskill does \
         not inject PAT/basic credentials into zip-url requests. Remove `auth` from this \
         source and use a pre-signed URL instead (e.g. an S3 or GCS presigned URL), which \
         embeds the credential in the URL itself and needs no separate `auth` configuration."
    );
}

#[test]
fn reject_configured_zip_url_auth_allows_when_absent() {
    assert!(reject_configured_zip_url_auth("my-zip-source", &None).is_ok());
}

/// Regression guard: this fix (the zip-url half of #273's git-auth fix)
/// must not change #273's git rejection message by even one character --
/// callers and tests elsewhere may assert on its exact text.
#[test]
fn reject_configured_git_auth_message_is_unchanged() {
    let err = reject_configured_git_auth("my-git-source", &pat_auth())
        .expect_err("configured auth on a git source must be rejected");
    assert_eq!(
        err.to_string(),
        "Git error: Source 'my-git-source' has `auth` configured, but git sources \
         authenticate via the system git credential helper or SSH agent, not via an \
         `auth` block -- fastskill does not inject PAT/basic credentials into git \
         operations. Remove `auth` from this source and either: (1) configure a git \
         credential helper (e.g. `git config credential.helper store`, or `gh auth \
         login`), or (2) use an SSH remote (e.g. `git@github.com:org/repo.git`) with a \
         key loaded in your SSH agent."
    );
}

#[test]
fn reject_configured_git_auth_allows_when_absent() {
    assert!(reject_configured_git_auth("my-git-source", &None).is_ok());
}

/// Call site 1: `get_skills_from_source`. Configured `auth` on a
/// zip-url source must be rejected before any network call is made --
/// the loopback base_url would otherwise fail with a connection-refused
/// `Network` error, not a `ZipUrl` one, so reaching the network at all
/// here would itself be a test failure.
#[tokio::test]
async fn get_skills_from_source_rejects_zip_url_auth() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_with_auth())
        .unwrap();
    let source_def = manager.get_source("zip-src").unwrap().clone();

    let err = manager
        .get_skills_from_source("zip-src", &source_def)
        .await
        .expect_err("configured auth must be rejected before any network call");
    assert!(matches!(err, SourcesError::ZipUrl(_)));
    assert!(err.to_string().contains("pre-signed URL"));
}

/// Call site 2: `get_marketplace_json`. Same guarantee as above, for
/// the other function that destructures `SourceConfig::ZipUrl` -- the
/// exact failure mode of the original bug was fixing only one of these.
#[tokio::test]
async fn get_marketplace_json_rejects_zip_url_auth() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_with_auth())
        .unwrap();

    let err = manager
        .get_marketplace_json("zip-src")
        .await
        .expect_err("configured auth must be rejected before any network call");
    assert!(matches!(err, SourcesError::ZipUrl(_)));
    assert!(err.to_string().contains("pre-signed URL"));
}

/// A zip-url source with no `auth` configured must proceed unaffected:
/// it should reach the network stage (and fail there, against a
/// deliberately unreachable loopback port) rather than being rejected
/// by the new auth gate.
#[tokio::test]
async fn get_skills_from_source_zip_url_without_auth_is_unaffected() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_without_auth())
        .unwrap();
    let source_def = manager.get_source("zip-src").unwrap().clone();

    let err = manager
        .get_skills_from_source("zip-src", &source_def)
        .await
        .expect_err("unreachable loopback URL should fail at the network stage");
    assert!(
        !matches!(err, SourcesError::ZipUrl(_)),
        "unexpected auth rejection for a source with no `auth` configured: {err}"
    );
}

/// Same as above for the `get_marketplace_json` call site.
#[tokio::test]
async fn get_marketplace_json_zip_url_without_auth_is_unaffected() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_without_auth())
        .unwrap();

    let err = manager
        .get_marketplace_json("zip-src")
        .await
        .expect_err("unreachable loopback URL should fail at the network stage");
    assert!(
        !matches!(err, SourcesError::ZipUrl(_)),
        "unexpected auth rejection for a source with no `auth` configured: {err}"
    );
}

//! Reading a git source's `marketplace.json` through git itself.
//!
//! A git source's catalog used to be fetched with an anonymous HTTP GET
//! against a `raw.githubusercontent.com` URL built from the repo URL. That
//! 404'd for every private repo (no credentials were ever sent), broke SSH
//! remotes (`git@github.com:org/repo.git` was spliced into the raw URL
//! verbatim), ignored a configured `tag`, and assumed a `main` default
//! branch. Skill *content* for git sources was already fetched with
//! `git clone` (`storage::git::clone_repository`), which picks up the system
//! credential helper and SSH agent -- so listing now does the same, and a
//! git source has exactly one credential model for both listing and install.

use super::SourcesError;
use crate::storage::git::{clone_repository, redact_url_credentials, scrub_inherited_git_env};

/// Catalog locations inside a repository, in lookup order: the Claude Code
/// standard location first, then the repository root.
pub(super) const CATALOG_PATHS: [&str; 2] = [".claude-plugin/marketplace.json", "marketplace.json"];

/// Where a source's `marketplace.json` is read from.
pub(super) enum CatalogLocation<'a> {
    /// Plain HTTP(S) GET of each candidate URL (zip-url sources).
    Http {
        claude_plugin_url: String,
        root_url: String,
        base_url: &'a str,
    },
    /// Shallow clone of the repository (git sources).
    Git {
        url: &'a str,
        branch: Option<&'a str>,
        tag: Option<&'a str>,
    },
}

impl CatalogLocation<'_> {
    /// Keys under which a listing from this location is cached in memory.
    pub(super) fn cache_keys(&self) -> Vec<String> {
        match self {
            Self::Http {
                claude_plugin_url,
                root_url,
                ..
            } => vec![claude_plugin_url.clone(), root_url.clone()],
            Self::Git { url, branch, tag } => vec![git_cache_key(url, *branch, *tag)],
        }
    }
}

/// In-memory cache key for a git catalog: the URL plus the configured ref,
/// so two sources pointing at different branches/tags of one repo never
/// share a listing.
pub(super) fn git_cache_key(url: &str, branch: Option<&str>, tag: Option<&str>) -> String {
    let reference = match (branch, tag) {
        (Some(branch), _) => format!("branch:{branch}"),
        (None, Some(tag)) => format!("tag:{tag}"),
        (None, None) => "HEAD".to_string(),
    };
    format!("git+{url}#{reference}")
}

/// A catalog read from a git checkout.
pub(super) struct GitCatalog {
    /// Raw bytes of the catalog file.
    pub(super) body: Vec<u8>,
    /// In-repo path the catalog was read from (one of [`CATALOG_PATHS`]).
    pub(super) found_at: &'static str,
    /// Commit the checkout is at, when `git rev-parse` could tell. Listing
    /// URLs are pinned to it so they name the revision the catalog came from.
    pub(super) commit: Option<String>,
}

/// Shallow-clone `url` at the configured ref (the remote's default branch
/// when neither is set) and return the first catalog file found.
pub(super) async fn read_git_catalog(
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> Result<GitCatalog, SourcesError> {
    let checkout = clone_repository(url, branch, tag, None)
        .await
        .map_err(|e| SourcesError::Git(format!("Failed to read marketplace.json: {e}")))?;

    for found_at in CATALOG_PATHS {
        let candidate = checkout.path().join(found_at);
        if candidate.is_file() {
            return Ok(GitCatalog {
                body: std::fs::read(&candidate)?,
                found_at,
                commit: head_commit(checkout.path()).await,
            });
        }
    }

    let reference = branch
        .map(|b| format!("branch '{b}'"))
        .or_else(|| tag.map(|t| format!("tag '{t}'")))
        .unwrap_or_else(|| "the default branch".to_string());
    Err(SourcesError::Git(format!(
        "Repository '{}' has no marketplace catalog on {reference}: expected {} or {}",
        redact_url_credentials(url),
        CATALOG_PATHS[0],
        CATALOG_PATHS[1]
    )))
}

/// `HEAD`'s commit in `repo_dir`, or `None` if git cannot say.
async fn head_commit(repo_dir: &std::path::Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["rev-parse", "HEAD"]).current_dir(repo_dir);
    // `GIT_DIR` beats `current_dir`; without this a run nested in another git
    // invocation would report the enclosing repository's HEAD.
    scrub_inherited_git_env(&mut cmd);
    let output = cmd.output().await.ok()?;
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !commit.is_empty()).then_some(commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_distinguishes_branch_tag_and_default_ref() {
        let url = "https://example.test/skills.git";
        assert_eq!(
            git_cache_key(url, Some("dev"), None),
            "git+https://example.test/skills.git#branch:dev"
        );
        assert_eq!(
            git_cache_key(url, None, Some("v1")),
            "git+https://example.test/skills.git#tag:v1"
        );
        assert_eq!(
            git_cache_key(url, None, None),
            "git+https://example.test/skills.git#HEAD"
        );
    }
}

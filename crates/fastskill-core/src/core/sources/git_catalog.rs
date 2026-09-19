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
use crate::storage::git::{clone_repository, redact_url_credentials};

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

/// Shallow-clone `url` at the configured ref (the remote's default branch
/// when neither is set) and return the raw bytes of the first catalog file
/// found, with the in-repo path it was read from.
pub(super) async fn read_git_catalog(
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> Result<(Vec<u8>, &'static str), SourcesError> {
    let checkout = clone_repository(url, branch, tag, None)
        .await
        .map_err(|e| SourcesError::Git(format!("Failed to read marketplace.json: {e}")))?;

    for relative in CATALOG_PATHS {
        let candidate = checkout.path().join(relative);
        if candidate.is_file() {
            return Ok((std::fs::read(&candidate)?, relative));
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

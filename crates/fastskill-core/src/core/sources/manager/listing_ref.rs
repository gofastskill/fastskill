//! Which git ref a git source's marketplace listing is read at.
//!
//! The listing (`marketplace.json` read from a shallow clone) and the download
//! (`git clone`) must agree on one revision. Both honor the configured
//! `branch`, then `tag`, then the remote's default branch, and listing URLs are
//! pinned to the commit the catalog was read at whenever git can report it.

/// The ref name used when no commit is known: the configured branch,
/// then the configured tag, then `HEAD` (which GitHub resolves to the
/// repository's default branch in both raw and tree URLs). Branch wins over tag
/// to mirror `storage::git::build_clone_args`.
pub(super) fn configured_ref<'a>(branch: Option<&'a str>, tag: Option<&'a str>) -> &'a str {
    branch.or(tag).unwrap_or("HEAD")
}

/// True for a `github.com` repository URL (as opposed to a raw-content URL or
/// another host), the only shape whose listing URL embeds a ref.
pub(super) fn is_github_repo_url(base_url: &str) -> bool {
    base_url.contains("github.com") && !base_url.contains("raw.githubusercontent.com")
}

/// `owner/repo` from a `github.com` repository URL.
pub(super) fn github_repo_path(base_url: &str) -> &str {
    base_url
        .trim_start_matches("https://github.com/")
        .trim_start_matches("http://github.com/")
        .trim_end_matches(".git")
        .trim_end_matches('/')
}

/// Drop `.` and empty segments so `./plugins/pack/skills/one` and
/// `plugins//pack/` do not leak into a URL as `/./` or `//`.
pub(super) fn normalize_repo_path(path: &str) -> String {
    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn configured_ref_prefers_branch_then_tag_then_head() {
        assert_eq!(configured_ref(None, None), "HEAD");
        assert_eq!(configured_ref(Some("develop"), None), "develop");
        assert_eq!(configured_ref(None, Some("v1.2.0")), "v1.2.0");
        assert_eq!(configured_ref(Some("develop"), Some("v1.2.0")), "develop");
    }

    #[test]
    fn github_repo_urls_are_recognized_and_reduced_to_owner_repo() {
        assert!(is_github_repo_url("https://github.com/acme/skills.git"));
        assert!(!is_github_repo_url(
            "https://raw.githubusercontent.com/acme/skills/main"
        ));
        assert!(!is_github_repo_url("https://skills.example.test/base/"));
        assert_eq!(
            github_repo_path("https://github.com/acme/skills.git"),
            "acme/skills"
        );
        assert_eq!(
            github_repo_path("http://github.com/acme/skills/"),
            "acme/skills"
        );
    }

    #[test]
    fn repo_paths_lose_dot_and_empty_segments() {
        assert_eq!(
            normalize_repo_path("./plugins/pack/skills/one"),
            "plugins/pack/skills/one"
        );
        assert_eq!(
            normalize_repo_path("plugins//pack/./one/"),
            "plugins/pack/one"
        );
        assert_eq!(normalize_repo_path("./"), "");
        assert_eq!(normalize_repo_path("plain"), "plain");
    }
}

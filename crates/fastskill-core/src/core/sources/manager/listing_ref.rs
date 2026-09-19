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

/// `owner/repo` for a `github.com` repository URL, the only shape whose
/// listing URL embeds a ref, or `None` for anything else.
///
/// Every remote form git accepts is recognized, because a source is commonly
/// configured with whatever `git remote -v` prints: `https://github.com/o/r`,
/// `ssh://git@github.com/o/r.git` and the scp-style `git@github.com:o/r.git`
/// all reduce to `o/r`. Raw-content URLs, other hosts and anything that is not
/// exactly one `owner/repo` pair yield `None`, so a URL we cannot read stays
/// out of the tree link instead of being pasted into one.
pub(super) fn github_repo(base_url: &str) -> Option<&str> {
    let (authority, path) = split_remote(base_url)?;
    // `user[:password]@host[:port]` -> `host`
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }

    let repo = path
        .trim_matches('/')
        .trim_end_matches(".git")
        .trim_end_matches('/');
    let mut segments = repo.split('/');
    let owner = segments.next()?;
    let name = segments.next()?;
    if owner.is_empty() || name.is_empty() || segments.next().is_some() {
        return None;
    }
    Some(repo)
}

/// Split a remote URL into its authority and its path: after `scheme://` the
/// path starts at the first `/`, while a scp-style remote has no scheme and
/// separates the two with `:`. A plain filesystem path has neither and is not
/// a remote URL.
fn split_remote(url: &str) -> Option<(&str, &str)> {
    match url.split_once("://") {
        Some((_scheme, rest)) => rest.split_once('/'),
        None => url.split_once(':'),
    }
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
    fn github_http_urls_are_reduced_to_owner_repo() {
        assert_eq!(
            github_repo("https://github.com/acme/skills.git"),
            Some("acme/skills")
        );
        assert_eq!(
            github_repo("http://github.com/acme/skills/"),
            Some("acme/skills")
        );
        assert_eq!(
            github_repo("https://GitHub.com/acme/skills"),
            Some("acme/skills")
        );
    }

    #[test]
    fn github_ssh_remotes_are_reduced_to_owner_repo() {
        // scp-style, the form `git remote -v` prints for an SSH clone.
        assert_eq!(
            github_repo("git@github.com:acme/skills.git"),
            Some("acme/skills")
        );
        assert_eq!(
            github_repo("git@github.com:acme/skills"),
            Some("acme/skills")
        );
        // ssh:// URL form, with and without the user and a port.
        assert_eq!(
            github_repo("ssh://git@github.com/acme/skills.git"),
            Some("acme/skills")
        );
        assert_eq!(
            github_repo("ssh://github.com/acme/skills"),
            Some("acme/skills")
        );
        assert_eq!(
            github_repo("ssh://git@github.com:22/acme/skills.git"),
            Some("acme/skills")
        );
    }

    #[test]
    fn non_github_repo_urls_have_no_owner_repo() {
        assert_eq!(
            github_repo("https://raw.githubusercontent.com/acme/skills/main"),
            None
        );
        assert_eq!(github_repo("https://skills.example.test/base/"), None);
        assert_eq!(github_repo("git@gitlab.com:acme/skills.git"), None);
        // A host that merely ends in `github.com` is a different host.
        assert_eq!(github_repo("https://notgithub.com/acme/skills"), None);
        assert_eq!(
            github_repo("https://github.com.evil.test/acme/skills"),
            None
        );
    }

    #[test]
    fn github_urls_that_are_not_one_owner_repo_pair_are_rejected() {
        assert_eq!(github_repo("https://github.com/acme"), None);
        assert_eq!(github_repo("https://github.com/"), None);
        assert_eq!(github_repo("https://github.com"), None);
        assert_eq!(
            github_repo("https://github.com/acme/skills/tree/main"),
            None
        );
        assert_eq!(github_repo("/srv/mirrors/acme/skills.git"), None);
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

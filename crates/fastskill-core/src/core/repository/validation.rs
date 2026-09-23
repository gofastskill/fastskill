//! Reject repository settings that would otherwise be silently ignored.
//!
//! A repository's `type` picks the client; its connection keys pick the
//! config variant. When they disagree, or when a key has no effect for that
//! type, the old code dropped the key (or the whole repository) without a
//! word. Validate explicit user input instead; see commits d661d88e and
//! e087c1da for the same rule applied to CLI flags and listing auth.

use super::{RepositoryConfig, RepositoryDefinition, RepositoryType};

/// Text for a git-marketplace repository that sets `auth`.
pub(crate) fn git_auth_unsupported(name: &str) -> String {
    format!(
        "Source '{name}' has `auth` configured, but git sources authenticate via \
         the system git credential helper or SSH agent, not via an `auth` block -- \
         fastskill does not inject PAT/basic credentials into git operations. Remove \
         `auth` from this source and either: (1) configure a git credential helper (e.g. \
         `git config credential.helper store`, or `gh auth login`), or (2) use an SSH \
         remote (e.g. `git@github.com:org/repo.git`) with a key loaded in your SSH agent."
    )
}

/// Text for a zip-url repository that sets `auth`.
pub(crate) fn zip_url_auth_unsupported(name: &str) -> String {
    format!(
        "Source '{name}' has `auth` configured, but zip-url sources fetch via a \
         plain HTTP GET and do not support an `auth` block -- fastskill does not inject \
         PAT/basic credentials into zip-url requests. Remove `auth` from this source and \
         use a pre-signed URL instead (e.g. an S3 or GCS presigned URL), which embeds the \
         credential in the URL itself and needs no separate `auth` configuration."
    )
}

fn type_name(repo_type: &RepositoryType) -> &'static str {
    match repo_type {
        RepositoryType::GitMarketplace => "git-marketplace",
        RepositoryType::HttpRegistry => "http-registry",
        RepositoryType::ZipUrl => "zip-url",
        RepositoryType::Local => "local",
    }
}

fn connection_key(config: &RepositoryConfig) -> (&'static str, &'static str) {
    match config {
        RepositoryConfig::GitMarketplace { .. } => ("url", "git-marketplace"),
        RepositoryConfig::HttpRegistry { .. } => ("index_url", "http-registry"),
        RepositoryConfig::ZipUrl { .. } => ("zip_url", "zip-url"),
        RepositoryConfig::Local { .. } => ("path", "local"),
    }
}

impl RepositoryDefinition {
    /// Fail on settings that would be accepted and then have no effect.
    pub fn validate(&self) -> Result<(), String> {
        let name = &self.name;
        let (key, implied) = connection_key(&self.config);
        let declared = type_name(&self.repo_type);
        if declared != implied {
            return Err(format!(
                "Repository '{name}' declares type = \"{declared}\" but sets `{key}`, which \
                 is the connection key for {implied} repositories. Use the key that matches \
                 the type, or change the type."
            ));
        }
        if let RepositoryConfig::GitMarketplace {
            branch: Some(branch),
            tag: Some(tag),
            ..
        } = &self.config
        {
            return Err(format!(
                "Repository '{name}' sets both branch = \"{branch}\" and tag = \"{tag}\"; \
                 they are mutually exclusive. Keep one."
            ));
        }
        if self.auth.is_some() {
            match self.repo_type {
                RepositoryType::GitMarketplace => return Err(git_auth_unsupported(name)),
                RepositoryType::ZipUrl => return Err(zip_url_auth_unsupported(name)),
                RepositoryType::Local => {
                    return Err(format!(
                        "Repository '{name}' has `auth` configured, but local repositories \
                         read from the filesystem and never use it. Remove `auth`."
                    ))
                }
                RepositoryType::HttpRegistry => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::repository::RepositoryAuth;
    use std::path::PathBuf;

    fn repo(repo_type: RepositoryType, config: RepositoryConfig) -> RepositoryDefinition {
        RepositoryDefinition {
            name: "team".to_string(),
            repo_type,
            priority: 0,
            config,
            auth: None,
            storage: None,
        }
    }

    fn git(branch: Option<&str>, tag: Option<&str>) -> RepositoryConfig {
        RepositoryConfig::GitMarketplace {
            url: "https://example.invalid/skills.git".to_string(),
            branch: branch.map(str::to_string),
            tag: tag.map(str::to_string),
        }
    }

    fn pat() -> Option<RepositoryAuth> {
        Some(RepositoryAuth::Pat {
            env_var: "TOKEN".to_string(),
        })
    }

    #[test]
    fn accepts_consistent_definitions() {
        repo(RepositoryType::GitMarketplace, git(Some("main"), None))
            .validate()
            .unwrap();
        repo(RepositoryType::GitMarketplace, git(None, Some("v1")))
            .validate()
            .unwrap();
        let mut registry = repo(
            RepositoryType::HttpRegistry,
            RepositoryConfig::HttpRegistry {
                index_url: "https://example.invalid/index".to_string(),
            },
        );
        registry.auth = pat();
        registry.validate().unwrap();
    }

    #[test]
    fn rejects_type_that_disagrees_with_connection_key() {
        let error = repo(RepositoryType::ZipUrl, git(None, None))
            .validate()
            .unwrap_err();
        assert!(
            error.contains("type = \"zip-url\" but sets `url`"),
            "{error}"
        );
    }

    #[test]
    fn rejects_branch_and_tag_together() {
        let error = repo(
            RepositoryType::GitMarketplace,
            git(Some("main"), Some("v1")),
        )
        .validate()
        .unwrap_err();
        assert!(error.contains("mutually exclusive"), "{error}");
    }

    #[test]
    fn rejects_auth_where_it_has_no_effect() {
        let mut git_repo = repo(RepositoryType::GitMarketplace, git(None, None));
        git_repo.auth = pat();
        assert!(git_repo
            .validate()
            .unwrap_err()
            .contains("git credential helper"));

        let mut zip = repo(
            RepositoryType::ZipUrl,
            RepositoryConfig::ZipUrl {
                base_url: "https://example.invalid/".to_string(),
            },
        );
        zip.auth = pat();
        assert!(zip.validate().unwrap_err().contains("pre-signed URL"));

        let mut local = repo(
            RepositoryType::Local,
            RepositoryConfig::Local {
                path: PathBuf::from("./skills"),
            },
        );
        local.auth = pat();
        assert!(local.validate().unwrap_err().contains("local repositories"));
    }
}

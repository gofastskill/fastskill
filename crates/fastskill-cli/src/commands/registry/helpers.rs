use crate::error::{CliError, CliResult};
use fastskill_core::core::repository::{
    RepositoryAuth, RepositoryConfig, RepositoryManager, RepositoryType,
};
use std::path::PathBuf;

pub async fn load_repo_manager() -> CliResult<RepositoryManager> {
    let repositories = crate::config::load_repositories_from_project()?;
    Ok(RepositoryManager::from_definitions(repositories))
}

pub fn resolve_repository_name(
    manager: &RepositoryManager,
    name: Option<String>,
) -> CliResult<String> {
    if let Some(repo_name) = name {
        Ok(repo_name)
    } else {
        manager
            .get_default_repository()
            .map(|r| r.name.clone())
            .ok_or_else(|| {
                CliError::Config(
                    "No repository specified and no default repository configured".to_string(),
                )
            })
    }
}

pub fn parse_repository_type(repo_type: &str) -> CliResult<RepositoryType> {
    match repo_type {
        "git-marketplace" => Ok(RepositoryType::GitMarketplace),
        "http-registry" => Ok(RepositoryType::HttpRegistry),
        "zip-url" => Ok(RepositoryType::ZipUrl),
        "local" => Ok(RepositoryType::Local),
        _ => Err(CliError::Config(format!(
            "Invalid repository type: {}. Use: git-marketplace, http-registry, zip-url, or local",
            repo_type
        ))),
    }
}

pub fn create_repository_config(
    repo_type: RepositoryType,
    url_or_path: String,
    branch: Option<String>,
    tag: Option<String>,
) -> RepositoryConfig {
    match repo_type {
        RepositoryType::GitMarketplace => RepositoryConfig::GitMarketplace {
            url: url_or_path,
            branch,
            tag,
        },
        RepositoryType::HttpRegistry => RepositoryConfig::HttpRegistry {
            index_url: url_or_path,
        },
        RepositoryType::ZipUrl => RepositoryConfig::ZipUrl {
            base_url: url_or_path,
        },
        RepositoryType::Local => RepositoryConfig::Local {
            path: PathBuf::from(url_or_path),
        },
    }
}

/// Error text shared by every auth method fastskill does not support.
///
/// `ssh-key`, `ssh`, `basic` and `api_key` used to be accepted here, held in
/// memory, and then silently discarded when the repository was written to
/// `skill-project.toml` -- the manifest's `AuthType` has only ever been able
/// to represent `pat`. Users configured them and believed they were in
/// effect. Rejecting is the honest answer; see also the git and zip-url
/// `auth` rejections in fastskill-core.
fn unsupported_auth_type(auth_type: &str) -> CliError {
    CliError::Config(format!(
        "Unsupported auth type '{auth_type}'. Only `pat` is supported -- it is the only \
         method the project manifest can store, so the others were never persisted even \
         when this command accepted them. For a private git remote, configure a git \
         credential helper or use an SSH remote instead; for a private HTTP registry, use \
         `--auth-type pat --auth-env <VAR>`."
    ))
}

pub fn parse_authentication(
    auth_type: Option<String>,
    auth_env: Option<String>,
    auth_key_path: Option<PathBuf>,
    auth_username: Option<String>,
) -> CliResult<Option<RepositoryAuth>> {
    // These two flags only ever fed the removed methods. Silently ignoring
    // them would recreate exactly the bug this change removes.
    if auth_key_path.is_some() {
        return Err(CliError::Config(
            "--auth-key-path is no longer supported: fastskill does not inject SSH key \
             credentials. Use an SSH remote with a key loaded in your SSH agent instead."
                .to_string(),
        ));
    }
    if auth_username.is_some() {
        return Err(CliError::Config(
            "--auth-username is no longer supported: basic authentication was never \
             persisted to the project manifest. Use `--auth-type pat --auth-env <VAR>`."
                .to_string(),
        ));
    }

    let Some(auth_t) = auth_type else {
        return Ok(None);
    };

    match auth_t.as_str() {
        "pat" => {
            let env_var = auth_env.ok_or_else(|| {
                CliError::Config("--auth-env required for pat authentication".to_string())
            })?;
            Ok(Some(RepositoryAuth::Pat { env_var }))
        }
        "ssh-key" | "ssh" | "basic" | "api_key" => Err(unsupported_auth_type(&auth_t)),
        _ => Err(CliError::Config(format!(
            "Invalid auth type: {}. Use: pat",
            auth_t
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn parse(auth_type: Option<&str>, auth_env: Option<&str>) -> CliResult<Option<RepositoryAuth>> {
        parse_authentication(
            auth_type.map(str::to_string),
            auth_env.map(str::to_string),
            None,
            None,
        )
    }

    #[test]
    fn pat_still_works() {
        let auth = parse(Some("pat"), Some("MY_TOKEN"))
            .expect("pat is supported")
            .expect("an auth block was requested");
        let RepositoryAuth::Pat { env_var } = auth;
        assert_eq!(env_var, "MY_TOKEN");
    }

    #[test]
    fn pat_still_requires_auth_env() {
        let err = parse(Some("pat"), None).expect_err("pat without --auth-env must fail");
        assert!(err.to_string().contains("--auth-env required"));
    }

    #[test]
    fn no_auth_type_means_no_auth() {
        assert!(parse(None, None).expect("absent auth is fine").is_none());
    }

    /// The four methods that used to be accepted here and then silently
    /// dropped on save must now be rejected, and the message must say why
    /// rather than just "invalid".
    #[test]
    fn removed_auth_types_are_rejected_with_an_explanation() {
        for auth_type in ["ssh-key", "ssh", "basic", "api_key"] {
            let err = parse(Some(auth_type), Some("SOME_VAR"))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(&format!("Unsupported auth type '{auth_type}'")),
                "message did not name the rejected type: {err}"
            );
            assert!(
                err.contains("never persisted"),
                "message did not explain that it never took effect: {err}"
            );
        }
    }

    #[test]
    fn genuinely_unknown_auth_type_is_still_rejected() {
        let err = parse(Some("kerberos"), None).unwrap_err().to_string();
        assert!(err.contains("Invalid auth type: kerberos"));
        assert!(err.contains("Use: pat"));
    }

    /// Silently ignoring these would recreate the very bug being fixed.
    #[test]
    fn flags_for_removed_methods_are_rejected_not_ignored() {
        let err = parse_authentication(
            Some("pat".to_string()),
            Some("MY_TOKEN".to_string()),
            Some(PathBuf::from("/tmp/key")),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("--auth-key-path is no longer supported"),
            "{err}"
        );

        let err = parse_authentication(
            Some("pat".to_string()),
            Some("MY_TOKEN".to_string()),
            None,
            Some("someone".to_string()),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("--auth-username is no longer supported"),
            "{err}"
        );
    }
}

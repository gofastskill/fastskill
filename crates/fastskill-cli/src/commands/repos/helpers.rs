use super::args::ReposAddArgs;
use crate::error::{CliError, CliResult};
use fastskill_core::core::repository::{
    user_config, RepositoryAuth, RepositoryConfig, RepositoryManager, RepositoryType,
};
use std::path::PathBuf;

/// Every repository this project can use: the project's own plus the user's
/// repositories.toml, where a user entry replaces a project entry of the same
/// name. For reading only; saving it would copy user entries into the project.
pub async fn load_repo_manager() -> CliResult<RepositoryManager> {
    let repositories = crate::config::load_repositories_from_project()?;
    let user = crate::config::load_user_repositories()?;
    Ok(RepositoryManager::from_definitions(
        user_config::merge_repositories(repositories, user),
    ))
}

/// The project's own repositories, for commands that save them back.
pub async fn load_project_repo_manager() -> CliResult<RepositoryManager> {
    let repositories = crate::config::load_repositories_from_project()?;
    Ok(RepositoryManager::from_definitions(repositories))
}

/// The user's repositories.toml, for `repo add --user` and `repo remove --user`.
pub fn load_user_repo_manager() -> CliResult<RepositoryManager> {
    let path = user_config::user_repositories_path().ok_or_else(|| {
        CliError::Config("FastSkill's config directory is unknown on this platform".to_string())
    })?;
    let mut manager = RepositoryManager::new(path);
    manager
        .load()
        .map_err(|e| CliError::Config(format!("Failed to load user repositories: {e}")))?;
    Ok(manager)
}

/// Whether `name` is configured in the user's repositories.toml.
pub fn is_user_repository(name: &str) -> bool {
    crate::config::load_user_repositories()
        .map(|repositories| repositories.iter().any(|r| r.name == name))
        .unwrap_or(false)
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

pub fn validate_repository_ref_options(
    repo_type: &RepositoryType,
    branch: Option<&str>,
    tag: Option<&str>,
) -> CliResult<()> {
    if branch.is_some() && tag.is_some() {
        return Err(CliError::Config(
            "--branch and --tag cannot be used together".to_string(),
        ));
    }
    if !matches!(repo_type, RepositoryType::GitMarketplace) && (branch.is_some() || tag.is_some()) {
        return Err(CliError::Config(
            "--branch and --tag are only valid for git-marketplace repositories".to_string(),
        ));
    }
    Ok(())
}

/// Error text shared by every auth method fastskill does not support.
///
/// `ssh-key`, `ssh`, `basic` and `api_key` used to be accepted here, held in
/// memory, and then silently discarded when the repository was written to
/// `skill-project.toml`. Users configured them and believed they were in
/// effect. Rejecting is the honest answer; see also the git and zip-url
/// `auth` rejections in fastskill-core.
fn unsupported_auth_type(auth_type: &str) -> CliError {
    CliError::Config(format!(
        "Unsupported auth type '{auth_type}'. Supported types are `pat`, `bearer` and \
         `command`; the others were never persisted even when this command accepted them. \
         For a private git remote, configure a git credential helper or use an SSH remote \
         instead; for a private HTTP registry, use `--auth-type bearer --auth-env <VAR>`."
    ))
}

/// The auth block `repo add` asked for (ADR-0018).
pub fn parse_authentication(args: &ReposAddArgs) -> CliResult<Option<RepositoryAuth>> {
    // These two flags only ever fed the removed methods. Silently ignoring
    // them would recreate exactly the bug this change removes.
    if args.auth_key_path.is_some() {
        return Err(CliError::Config(
            "--auth-key-path is no longer supported: fastskill does not inject SSH key \
             credentials. Use an SSH remote with a key loaded in your SSH agent instead."
                .to_string(),
        ));
    }
    if args.auth_username.is_some() {
        return Err(CliError::Config(
            "--auth-username is no longer supported: basic authentication was never \
             persisted to the project manifest. Use `--auth-type bearer --auth-env <VAR>`."
                .to_string(),
        ));
    }

    let auth_t = args.auth_type.as_deref();
    let has_command_flags = args.credential_command.is_some() || !args.credential_args.is_empty();
    if has_command_flags && auth_t != Some("command") {
        return Err(CliError::Config(
            "--credential-command and --credential-arg are only used with --auth-type command"
                .to_string(),
        ));
    }
    let Some(auth_t) = auth_t else {
        return Ok(None);
    };
    let env_var = |kind: &str| {
        args.auth_env.clone().ok_or_else(|| {
            CliError::Config(format!("--auth-env required for {kind} authentication"))
        })
    };

    match auth_t {
        "pat" => Ok(Some(RepositoryAuth::Pat {
            env_var: env_var("pat")?,
        })),
        "bearer" => Ok(Some(RepositoryAuth::Bearer {
            env_var: env_var("bearer")?,
        })),
        "command" => {
            if args.auth_env.is_some() {
                return Err(CliError::Config(
                    "--auth-env is not used by command authentication; the command prints \
                     the token"
                        .to_string(),
                ));
            }
            let program = args.credential_command.clone().ok_or_else(|| {
                CliError::Config(
                    "--credential-command required for command authentication".to_string(),
                )
            })?;
            if !args.user {
                return Err(CliError::Config(
                    "--auth-type command needs --user: a project file cannot name a command \
                     for FastSkill to run, so this repository can only be saved to your user \
                     repositories.toml"
                        .to_string(),
                ));
            }
            let mut command = vec![program];
            command.extend(args.credential_args.iter().cloned());
            Ok(Some(RepositoryAuth::Command { command }))
        }
        "ssh-key" | "ssh" | "basic" | "api_key" => Err(unsupported_auth_type(auth_t)),
        _ => Err(CliError::Config(format!(
            "Invalid auth type: {auth_t}. Use: pat, bearer, or command"
        ))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn parse(auth_type: Option<&str>, auth_env: Option<&str>) -> CliResult<Option<RepositoryAuth>> {
        parse_authentication(&ReposAddArgs {
            auth_type: auth_type.map(str::to_string),
            auth_env: auth_env.map(str::to_string),
            ..Default::default()
        })
    }

    #[test]
    fn pat_still_works() {
        let auth = parse(Some("pat"), Some("MY_TOKEN"))
            .expect("pat is supported")
            .expect("an auth block was requested");
        assert_eq!(
            auth,
            RepositoryAuth::Pat {
                env_var: "MY_TOKEN".to_string()
            }
        );
    }

    #[test]
    fn bearer_takes_its_variable_from_auth_env() {
        let auth = parse(Some("bearer"), Some("REGISTRY_TOKEN")).unwrap();
        assert_eq!(
            auth,
            Some(RepositoryAuth::Bearer {
                env_var: "REGISTRY_TOKEN".to_string()
            })
        );
        let err = parse(Some("bearer"), None).unwrap_err().to_string();
        assert!(err.contains("--auth-env required for bearer"), "{err}");
    }

    fn command_args(user: bool) -> ReposAddArgs {
        ReposAddArgs {
            auth_type: Some("command".to_string()),
            credential_command: Some("my-login".to_string()),
            credential_args: vec!["token".to_string(), "--quiet".to_string()],
            user,
            ..Default::default()
        }
    }

    #[test]
    fn command_builds_argv_and_needs_user() {
        let auth = parse_authentication(&command_args(true)).unwrap();
        assert_eq!(
            auth,
            Some(RepositoryAuth::Command {
                command: vec!["my-login".into(), "token".into(), "--quiet".into()]
            })
        );
        let err = parse_authentication(&command_args(false))
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs --user"), "{err}");
        assert!(err.contains("cannot name a command"), "{err}");
    }

    #[test]
    fn command_flags_are_checked() {
        let mut no_program = command_args(true);
        no_program.credential_command = None;
        let err = parse_authentication(&no_program).unwrap_err().to_string();
        assert!(err.contains("--credential-command required"), "{err}");

        let mut with_env = command_args(true);
        with_env.auth_env = Some("T".to_string());
        assert!(parse_authentication(&with_env).is_err());

        let mut stray = command_args(true);
        stray.auth_type = Some("bearer".to_string());
        stray.auth_env = Some("T".to_string());
        let err = parse_authentication(&stray).unwrap_err().to_string();
        assert!(err.contains("only used with --auth-type command"), "{err}");
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

    #[test]
    fn repository_refs_are_unambiguous_and_git_only() {
        assert!(validate_repository_ref_options(
            &RepositoryType::GitMarketplace,
            Some("main"),
            None,
        )
        .is_ok());
        assert!(validate_repository_ref_options(
            &RepositoryType::GitMarketplace,
            None,
            Some("v1.2.0"),
        )
        .is_ok());

        let conflicting = validate_repository_ref_options(
            &RepositoryType::GitMarketplace,
            Some("main"),
            Some("v1.2.0"),
        )
        .unwrap_err();
        assert!(conflicting.to_string().contains("cannot be used together"));

        let wrong_type =
            validate_repository_ref_options(&RepositoryType::HttpRegistry, None, Some("v1.2.0"))
                .unwrap_err();
        assert!(wrong_type
            .to_string()
            .contains("only valid for git-marketplace"));
    }

    /// Silently ignoring these would recreate the very bug being fixed.
    #[test]
    fn flags_for_removed_methods_are_rejected_not_ignored() {
        let err = parse_authentication(&ReposAddArgs {
            auth_type: Some("pat".to_string()),
            auth_env: Some("MY_TOKEN".to_string()),
            auth_key_path: Some(PathBuf::from("/tmp/key")),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("--auth-key-path is no longer supported"),
            "{err}"
        );

        let err = parse_authentication(&ReposAddArgs {
            auth_type: Some("pat".to_string()),
            auth_env: Some("MY_TOKEN".to_string()),
            auth_username: Some("someone".to_string()),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("--auth-username is no longer supported"),
            "{err}"
        );
    }
}

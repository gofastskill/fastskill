use crate::cli::error::{CliError, CliResult};
use fastskill::core::repository::{
    RepositoryAuth, RepositoryConfig, RepositoryManager, RepositoryType,
};
use std::path::PathBuf;

pub async fn load_repo_manager() -> CliResult<RepositoryManager> {
    let repositories = crate::cli::config::load_repositories_from_project()?;
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

pub fn parse_authentication(
    auth_type: Option<String>,
    auth_env: Option<String>,
    auth_key_path: Option<PathBuf>,
    auth_username: Option<String>,
) -> CliResult<Option<RepositoryAuth>> {
    if let Some(auth_t) = auth_type {
        match auth_t.as_str() {
            "pat" => {
                let env_var = auth_env.ok_or_else(|| {
                    CliError::Config("--auth-env required for pat authentication".to_string())
                })?;
                Ok(Some(RepositoryAuth::Pat { env_var }))
            }
            "ssh-key" | "ssh" => {
                let key_path = auth_key_path.ok_or_else(|| {
                    CliError::Config("--auth-key-path required for ssh authentication".to_string())
                })?;
                if auth_t == "ssh-key" {
                    Ok(Some(RepositoryAuth::SshKey { path: key_path }))
                } else {
                    Ok(Some(RepositoryAuth::Ssh { key_path }))
                }
            }
            "basic" => {
                let username = auth_username.ok_or_else(|| {
                    CliError::Config(
                        "--auth-username required for basic authentication".to_string(),
                    )
                })?;
                let password_env = auth_env.ok_or_else(|| {
                    CliError::Config("--auth-env required for basic authentication".to_string())
                })?;
                Ok(Some(RepositoryAuth::Basic {
                    username,
                    password_env,
                }))
            }
            "api_key" => {
                let env_var = auth_env.ok_or_else(|| {
                    CliError::Config("--auth-env required for api_key authentication".to_string())
                })?;
                Ok(Some(RepositoryAuth::ApiKey { env_var }))
            }
            _ => Err(CliError::Config(format!(
                "Invalid auth type: {}. Use: pat, ssh-key, ssh, basic, api_key",
                auth_t
            ))),
        }
    } else {
        Ok(None)
    }
}

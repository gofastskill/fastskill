//! Git operations for cloning skill repositories

use crate::core::service::ServiceError;
use crate::core::sources::SourceAuth;

// Make functions public for use in CLI
use git2::{Cred, RemoteCallbacks, Repository};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::{debug, info};

/// Clone a git repository to a temporary directory
pub async fn clone_repository(
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    auth: Option<&SourceAuth>,
) -> Result<TempDir, ServiceError> {
    let temp_dir = TempDir::new().map_err(|e| {
        ServiceError::Custom(format!("Failed to create temporary directory: {}", e))
    })?;

    info!("Cloning repository: {}", url);

    // Build clone options with authentication if provided
    let mut fetch_options = git2::FetchOptions::new();
    if let Some(auth_config) = auth {
        let mut callbacks = RemoteCallbacks::new();
        setup_auth_callbacks(&mut callbacks, auth_config)?;
        fetch_options.remote_callbacks(callbacks);
    }

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_options);

    // Clone the repository
    let repo = builder
        .clone(url, temp_dir.path())
        .map_err(|e| ServiceError::Custom(format!("Failed to clone repository {}: {}", url, e)))?;

    // Checkout branch or tag if specified
    if let Some(branch_name) = branch {
        checkout_branch_or_tag(&repo, branch_name, true)?;
        debug!("Checked out branch: {}", branch_name);
    } else if let Some(tag_name) = tag {
        checkout_branch_or_tag(&repo, tag_name, false)?;
        debug!("Checked out tag: {}", tag_name);
    }

    Ok(temp_dir)
}

/// Checkout a specific branch or tag
fn checkout_branch_or_tag(
    repo: &Repository,
    ref_name: &str,
    is_branch: bool,
) -> Result<(), ServiceError> {
    let (object, reference) = if is_branch {
        repo.revparse_ext(ref_name)
            .map_err(|e| ServiceError::Custom(format!("Branch '{}' not found: {}", ref_name, e)))?
    } else {
        repo.revparse_ext(&format!("refs/tags/{}", ref_name))
            .or_else(|_| repo.revparse_ext(ref_name))
            .map_err(|e| ServiceError::Custom(format!("Tag '{}' not found: {}", ref_name, e)))?
    };

    let commit = object.as_commit().ok_or_else(|| {
        ServiceError::Custom(format!("Cannot checkout {}: not a commit", ref_name))
    })?;
    let tree = commit
        .tree()
        .map_err(|e| ServiceError::Custom(format!("Failed to get tree for {}: {}", ref_name, e)))?;

    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.force();
    repo.checkout_tree(tree.as_object(), Some(&mut checkout_opts))
        .map_err(|e| ServiceError::Custom(format!("Failed to checkout {}: {}", ref_name, e)))?;

    if let Some(ref_reference) = reference {
        if let Some(ref_name) = ref_reference.name() {
            repo.set_head(ref_name).map_err(|e| {
                ServiceError::Custom(format!("Failed to set HEAD to {}: {}", ref_name, e))
            })?;
        }
    }

    Ok(())
}

/// Setup authentication callbacks for git operations
fn setup_auth_callbacks(
    callbacks: &mut RemoteCallbacks,
    auth: &SourceAuth,
) -> Result<(), ServiceError> {
    match auth {
        SourceAuth::Pat { env_var } => {
            let token = std::env::var(env_var).map_err(|_| {
                ServiceError::Custom(format!(
                    "Environment variable '{}' not set for PAT authentication",
                    env_var
                ))
            })?;

            callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                let username = username_from_url.unwrap_or("git");
                Cred::userpass_plaintext(username, &token)
            });
        }
        SourceAuth::SshKey { path } => {
            let key_path = path.clone();
            callbacks.credentials(move |_url, username_from_url, _allowed_types| {
                let username = username_from_url.unwrap_or("git");
                Cred::ssh_key(username, None, &key_path, None)
            });
        }
        SourceAuth::Basic {
            username,
            password_env,
        } => {
            let password = std::env::var(password_env).map_err(|_| {
                ServiceError::Custom(format!(
                    "Environment variable '{}' not set for basic authentication",
                    password_env
                ))
            })?;
            let user = username.clone();
            callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext(&user, &password)
            });
        }
    }
    Ok(())
}

/// Validate that a cloned repository contains a valid skill structure
pub fn validate_cloned_skill(cloned_path: &Path) -> Result<PathBuf, ServiceError> {
    // Check if SKILL.md exists at the root
    let skill_file = cloned_path.join("SKILL.md");
    if skill_file.exists() {
        return Ok(cloned_path.to_path_buf());
    }

    // Check subdirectories for SKILL.md
    let entries = std::fs::read_dir(cloned_path)
        .map_err(|e| ServiceError::Custom(format!("Failed to read cloned directory: {}", e)))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| ServiceError::Custom(format!("Failed to read directory entry: {}", e)))?;
        let path = entry.path();
        if path.is_dir() {
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                return Ok(path);
            }
        }
    }

    Err(ServiceError::Validation(
        "Cloned repository does not contain a valid skill structure (SKILL.md not found)"
            .to_string(),
    ))
}

//! Authentication for registry access

use crate::core::service::ServiceError;
use std::env;
use std::path::PathBuf;

/// Authentication trait for registry access
#[async_trait::async_trait]
pub trait Auth: Send + Sync {
    /// Get authentication header value
    fn get_auth_header(&self) -> Result<String, ServiceError>;

    /// Check if authentication is configured
    fn is_configured(&self) -> bool;
}

/// GitHub Personal Access Token authentication
pub struct GitHubPat {
    token: Option<String>,
    env_var: String,
}

impl GitHubPat {
    pub fn new(env_var: String) -> Self {
        let token = env::var(&env_var).ok();
        Self { token, env_var }
    }
}

#[async_trait::async_trait]
impl Auth for GitHubPat {
    fn get_auth_header(&self) -> Result<String, ServiceError> {
        let token = self.token.as_ref().ok_or_else(|| {
            ServiceError::Custom(format!(
                "GitHub token not found. Set {} environment variable",
                self.env_var
            ))
        })?;
        Ok(format!("token {}", token))
    }

    fn is_configured(&self) -> bool {
        self.token.is_some() || env::var(&self.env_var).is_ok()
    }
}

/// SSH key authentication
pub struct SshKey {
    key_path: PathBuf,
}

impl SshKey {
    pub fn new(key_path: PathBuf) -> Self {
        Self { key_path }
    }
}

#[async_trait::async_trait]
impl Auth for SshKey {
    fn get_auth_header(&self) -> Result<String, ServiceError> {
        // SSH keys are used for git operations, not HTTP headers
        // This is a placeholder - actual SSH auth is handled by git2
        Err(ServiceError::Custom(
            "SSH authentication is handled by git operations, not HTTP headers".to_string(),
        ))
    }

    fn is_configured(&self) -> bool {
        self.key_path.exists()
    }
}

/// API key authentication
pub struct ApiKey {
    key: Option<String>,
    env_var: String,
}

impl ApiKey {
    pub fn new(env_var: String) -> Self {
        let key = env::var(&env_var).ok();
        Self { key, env_var }
    }
}

#[async_trait::async_trait]
impl Auth for ApiKey {
    fn get_auth_header(&self) -> Result<String, ServiceError> {
        let key = if let Some(ref k) = self.key {
            k.as_str()
        } else if let Ok(k) = env::var(&self.env_var) {
            // Store in self for future use (but we can't mutate, so just use the value)
            return Ok(format!("Bearer {}", k));
        } else {
            return Err(ServiceError::Custom(format!(
                "API key not found. Set {} environment variable",
                self.env_var
            )));
        };
        Ok(format!("Bearer {}", key))
    }

    fn is_configured(&self) -> bool {
        self.key.is_some() || env::var(&self.env_var).is_ok()
    }
}

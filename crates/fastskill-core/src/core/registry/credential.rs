//! Bearer and credential-command auth for an http-registry (ADR-0018).
//!
//! The token lives only in memory, inside [`SecretToken`], whose `Debug`
//! output is redacted and which has no `Display`. It is never written to
//! disk, logs, tracing or error messages.

use super::config::AuthConfig;
use super::credential_command::{run_credential_command, CommandSettings};
use crate::core::service::ServiceError;
use reqwest::header::HeaderValue;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::OnceCell;

/// A token. Its `Debug` output is redacted and it has no `Display`, so it
/// cannot reach a log line or an error message by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretToken(String);

impl SecretToken {
    pub(crate) fn new(token: String) -> Self {
        Self(token)
    }

    /// The token itself. Use only to build a request header.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretToken(<redacted>)")
    }
}

/// Where a bearer token comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// An environment variable, read on each use.
    Env { env_var: String },
    /// A program, run at most once per process for this repository.
    Command { argv: Vec<String> },
}

impl Credential {
    /// The bearer credential an auth config names, if it names one.
    pub fn from_config(auth: &AuthConfig) -> Option<Self> {
        match auth {
            AuthConfig::Bearer { env_var } => Some(Self::Env {
                env_var: env_var.clone(),
            }),
            AuthConfig::Command { command } => Some(Self::Command {
                argv: command.clone(),
            }),
            AuthConfig::Pat { .. } | AuthConfig::Ssh { .. } | AuthConfig::ApiKey { .. } => None,
        }
    }

    /// The `Authorization` header value, `Bearer <token>`, marked sensitive.
    /// Errors name the repository and where the token should come from,
    /// never the token.
    pub async fn header(
        &self,
        repository: &str,
        index_url: &str,
    ) -> Result<HeaderValue, ServiceError> {
        let token = match self {
            Self::Env { env_var } => match std::env::var(env_var) {
                Ok(token) if !token.trim().is_empty() => SecretToken::new(token),
                _ => {
                    return Err(ServiceError::Config(format!(
                        "Repository '{repository}' uses bearer auth, but the environment \
                         variable {env_var} is not set or is empty"
                    )))
                }
            },
            Self::Command { argv } => cached_command_token(repository, index_url, argv).await?,
        };
        bearer_header(&token).ok_or_else(|| {
            ServiceError::Config(format!(
                "Repository '{repository}': the token is not a valid HTTP header value"
            ))
        })
    }
}

/// `Bearer <token>` as a sensitive header value, or `None` if the token
/// holds characters a header cannot carry.
pub fn bearer_header(token: &SecretToken) -> Option<HeaderValue> {
    let mut value = HeaderValue::from_str(&format!("Bearer {}", token.expose())).ok()?;
    value.set_sensitive(true);
    Some(value)
}

type CacheKey = (String, String, Vec<String>);
type TokenCell = Arc<OnceCell<SecretToken>>;

/// One cell per (repository, index URL, command): the command runs at most
/// once per process, and concurrent requests wait for that one run. A failed
/// run leaves the cell empty, so a later request tries again.
fn token_cell(key: CacheKey) -> TokenCell {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, TokenCell>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cells = match cache.lock() {
        Ok(cells) => cells,
        Err(poisoned) => poisoned.into_inner(),
    };
    cells.entry(key).or_default().clone()
}

async fn cached_command_token(
    repository: &str,
    index_url: &str,
    argv: &[String],
) -> Result<SecretToken, ServiceError> {
    let cell = token_cell((repository.to_string(), index_url.to_string(), argv.to_vec()));
    cell.get_or_try_init(|| async {
        let settings = CommandSettings::detect()?;
        run_credential_command(argv, &settings).await
    })
    .await
    .cloned()
}

/// What to do with a redirect from `from` to `to` on a request whose token
/// belongs to `registry_origin`.
#[derive(Debug, PartialEq, Eq)]
pub enum RedirectDecision {
    /// Follow it; send the token only when `send_token` is true.
    Follow { send_token: bool },
    /// Do not follow it; the message says why.
    Refuse(String),
}

/// The redirect policy for authenticated requests: refuse a downgrade from
/// https to http and any scheme other than http(s); otherwise follow, and
/// send the token only to the registry's own origin.
pub fn redirect_decision(
    from: &url::Url,
    to: &url::Url,
    registry_origin: &url::Origin,
) -> RedirectDecision {
    if !matches!(to.scheme(), "http" | "https") {
        return RedirectDecision::Refuse(format!(
            "refusing a redirect to the unsupported scheme '{}'",
            to.scheme()
        ));
    }
    if from.scheme() == "https" && to.scheme() == "http" {
        return RedirectDecision::Refuse(format!(
            "refusing a redirect from https to http ({} to {}) on an authenticated request",
            from.origin().ascii_serialization(),
            to.origin().ascii_serialization()
        ));
    }
    RedirectDecision::Follow {
        send_token: to.origin() == *registry_origin,
    }
}

#[cfg(test)]
#[path = "credential_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

//! Authentication configuration management for CLI
//! Handles storage and retrieval of JWT tokens per registry

use crate::cli::error::{CliError, CliResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use dirs::config_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Authentication configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthConfig {
    /// Registry-specific authentication entries
    #[serde(default)]
    registry: HashMap<String, RegistryAuth>,
}

/// Authentication info for a specific registry
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryAuth {
    token: String,
    role: String,
    username: Option<String>,
    expires_at: Option<String>,
    last_refresh: Option<String>,
}

/// JWT claims structure for expiration checking
#[derive(Debug, serde::Deserialize)]
struct Claims {
    exp: usize,
    sub: Option<String>,
    #[allow(dead_code)] // May be used in future for role-based access control
    role: Option<String>,
}

/// Get the config file path
fn get_config_path() -> CliResult<PathBuf> {
    // Check for FASTSKILL_CONFIG_DIR environment variable first
    if let Ok(env_config_dir) = std::env::var("FASTSKILL_CONFIG_DIR") {
        return Ok(PathBuf::from(env_config_dir).join("config.toml"));
    }

    let config_dir = config_dir()
        .ok_or_else(|| CliError::Config("Failed to determine config directory".to_string()))?;
    let fastskill_dir = config_dir.join("fastskill");
    Ok(fastskill_dir.join("config.toml"))
}

/// Normalize registry URL for consistent storage
fn normalize_registry_url(url: &str) -> String {
    let mut normalized = url.trim().to_string();
    // Remove trailing slash
    if normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

/// Load authentication configuration from file
fn load_auth_config() -> CliResult<AuthConfig> {
    let config_path = get_config_path()?;

    if !config_path.exists() {
        return Ok(AuthConfig {
            registry: HashMap::new(),
        });
    }

    let content = fs::read_to_string(&config_path).map_err(|e| {
        CliError::Config(format!(
            "Failed to read config file {}: {}",
            config_path.display(),
            e
        ))
    })?;

    let config: AuthConfig = toml::from_str(&content).map_err(|e| {
        CliError::Config(format!(
            "Failed to parse config file {}: {}",
            config_path.display(),
            e
        ))
    })?;

    Ok(config)
}

/// Save authentication configuration to file
fn save_auth_config(config: &AuthConfig) -> CliResult<()> {
    let config_path = get_config_path()?;
    let config_dir = config_path
        .parent()
        .ok_or_else(|| CliError::Config("Invalid config path".to_string()))?;

    // Create config directory if it doesn't exist
    fs::create_dir_all(config_dir)
        .map_err(|e| CliError::Config(format!("Failed to create config directory: {}", e)))?;

    // Serialize to TOML
    let content = toml::to_string_pretty(config)
        .map_err(|e| CliError::Config(format!("Failed to serialize config: {}", e)))?;

    // Write to file with secure permissions (0o600)
    fs::write(&config_path, content).map_err(|e| {
        CliError::Config(format!(
            "Failed to write config file {}: {}",
            config_path.display(),
            e
        ))
    })?;

    // Set file permissions to 0o600 (read/write for owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&config_path)
            .map_err(|e| CliError::Config(format!("Failed to get file metadata: {}", e)))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&config_path, perms)
            .map_err(|e| CliError::Config(format!("Failed to set file permissions: {}", e)))?;
    }

    Ok(())
}

/// Decode JWT token without verification (for reading claims only)
/// JWTs are base64url-encoded JSON, format: header.payload.signature
fn decode_token_unsafe(token: &str) -> Option<Claims> {
    // Split JWT into parts
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    // Decode the payload (second part)
    let payload = parts[1];

    // Base64url decode
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let json_str = String::from_utf8(decoded).ok()?;

    // Parse JSON into Claims
    serde_json::from_str::<Claims>(&json_str).ok()
}

/// Check if a JWT token is expired or expiring soon
fn is_token_expired_or_expiring_soon(token: &str, buffer_seconds: u64) -> CliResult<bool> {
    match decode_token_unsafe(token) {
        Some(claims) => {
            let exp = claims.exp as u64;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| CliError::Config(format!("Time error: {}", e)))?
                .as_secs();

            // Check if expired or expiring within buffer_seconds
            Ok(exp <= now + buffer_seconds)
        }
        None => {
            // If we can't decode, assume it's invalid/expired
            Ok(true)
        }
    }
}

/// Get token expiration time from JWT
fn get_token_expiration(token: &str) -> Option<DateTime<Utc>> {
    decode_token_unsafe(token).and_then(|claims| {
        let exp = claims.exp as u64;
        DateTime::from_timestamp(exp as i64, 0)
    })
}

/// Get username from JWT token
fn get_token_username(token: &str) -> Option<String> {
    decode_token_unsafe(token).and_then(|claims| claims.sub)
}

/// Get role from JWT token
#[allow(dead_code)] // May be used in future for role-based access control
fn get_token_role(token: &str) -> Option<String> {
    decode_token_unsafe(token).and_then(|claims| claims.role)
}

/// Get authentication token for a specific registry
/// Priority: FASTSKILL_API_TOKEN env var > config file
#[allow(dead_code)] // May be used in future
pub fn get_token_for_registry(registry_url: &str) -> CliResult<Option<String>> {
    // Priority 1: Environment variable
    if let Ok(token) = std::env::var("FASTSKILL_API_TOKEN") {
        return Ok(Some(token));
    }

    // Priority 2: Config file
    let normalized_url = normalize_registry_url(registry_url);
    let config = load_auth_config()?;

    if let Some(auth) = config.registry.get(&normalized_url) {
        Ok(Some(auth.token.clone()))
    } else {
        Ok(None)
    }
}

/// Get token with automatic refresh if needed
pub async fn get_token_with_refresh(registry_url: &str) -> CliResult<Option<String>> {
    // Check environment variable first
    if let Ok(token) = std::env::var("FASTSKILL_API_TOKEN") {
        return Ok(Some(token));
    }

    let normalized_url = normalize_registry_url(registry_url);
    let mut config = load_auth_config()?;

    if let Some(auth) = config.registry.get(&normalized_url) {
        // Check expiration using stored expires_at or decode token
        let is_expired = if let Some(expires_at_str) = &auth.expires_at {
            // Use stored expiration time (more efficient)
            if let Ok(expires_at) = DateTime::parse_from_rfc3339(expires_at_str) {
                let now = Utc::now();
                let buffer = chrono::Duration::seconds(300); // 5 minutes
                expires_at.with_timezone(&Utc) <= now + buffer
            } else {
                // Fallback to decoding token
                is_token_expired_or_expiring_soon(&auth.token, 300)?
            }
        } else {
            // No stored expiration, decode token
            is_token_expired_or_expiring_soon(&auth.token, 300)?
        };

        if is_expired {
            // Token expired, try to refresh
            println!(
                "Token expired for registry: {}, refreshing...",
                normalized_url
            );

            // Refresh token by calling /auth/token endpoint
            let refresh_result = refresh_token_for_registry(&normalized_url, &auth.role).await;

            match refresh_result {
                Ok(new_token) => {
                    // Update config with new token
                    let username = get_token_username(&new_token);
                    let expires_at = get_token_expiration(&new_token).map(|dt| dt.to_rfc3339());

                    let new_auth = RegistryAuth {
                        token: new_token.clone(),
                        role: auth.role.clone(),
                        username,
                        expires_at,
                        last_refresh: Some(Utc::now().to_rfc3339()),
                    };

                    config.registry.insert(normalized_url.clone(), new_auth);
                    save_auth_config(&config)?;

                    println!("✓ Token refreshed");
                    Ok(Some(new_token))
                }
                Err(e) => {
                    eprintln!(
                        "Token refresh failed for registry: {}. Run `fastskill auth login` to re-authenticate.",
                        normalized_url
                    );
                    Err(e)
                }
            }
        } else {
            // Token is still valid
            Ok(Some(auth.token.clone()))
        }
    } else {
        Ok(None)
    }
}

/// Refresh token for a registry by calling /auth/token endpoint
async fn refresh_token_for_registry(registry_url: &str, role: &str) -> CliResult<String> {
    let client = reqwest::Client::new();
    let token_url = format!("{}/auth/token", registry_url);

    let request_body = serde_json::json!({
        "role": role
    });

    let response = client
        .post(&token_url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| CliError::Config(format!("Failed to refresh token: {}", e)))?;

    if !response.status().is_success() {
        return Err(CliError::Config(format!(
            "Token refresh failed with status {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| CliError::Config(format!("Failed to parse token response: {}", e)))?;

    let token = json
        .get("data")
        .and_then(|d| d.get("token"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| CliError::Config("Invalid token response format".to_string()))?;

    Ok(token.to_string())
}

/// Store authentication token for a registry
pub async fn set_token_for_registry(registry_url: &str, token: &str, role: &str) -> CliResult<()> {
    let normalized_url = normalize_registry_url(registry_url);
    let mut config = load_auth_config()?;

    let username = get_token_username(token);
    let expires_at = get_token_expiration(token).map(|dt| dt.to_rfc3339());

    let auth = RegistryAuth {
        token: token.to_string(),
        role: role.to_string(),
        username,
        expires_at,
        last_refresh: Some(Utc::now().to_rfc3339()),
    };

    config.registry.insert(normalized_url, auth);
    save_auth_config(&config)?;

    Ok(())
}

/// Remove authentication token for a registry
pub fn remove_token_for_registry(registry_url: &str) -> CliResult<()> {
    let normalized_url = normalize_registry_url(registry_url);
    let mut config = load_auth_config()?;

    config.registry.remove(&normalized_url);
    save_auth_config(&config)?;

    Ok(())
}

/// Get authentication info for a registry (for whoami command)
pub fn get_auth_info_for_registry(registry_url: &str) -> CliResult<Option<AuthInfo>> {
    let normalized_url = normalize_registry_url(registry_url);
    let config = load_auth_config()?;

    if let Some(auth) = config.registry.get(&normalized_url) {
        Ok(Some(AuthInfo {
            registry_url: normalized_url,
            username: auth.username.clone(),
            role: auth.role.clone(),
            expires_at: auth.expires_at.clone(),
            last_refresh: auth.last_refresh.clone(),
        }))
    } else {
        Ok(None)
    }
}

/// Authentication information structure
#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub registry_url: String,
    pub username: Option<String>,
    pub role: String,
    pub expires_at: Option<String>,
    pub last_refresh: Option<String>,
}

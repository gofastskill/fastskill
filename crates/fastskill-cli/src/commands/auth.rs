//! Authentication commands for CLI

use crate::auth_config;
use crate::error::{CliError, CliResult};
use crate::utils::messages;
use clap::{Args, Subcommand};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;

/// Authentication command arguments
#[derive(Debug, Args)]
#[command(after_help = "Examples:\n  fastskill auth login\n  fastskill auth whoami")]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Login to a registry and store authentication token
    #[command(
        after_help = "Examples:\n  fastskill auth login\n  fastskill auth login --registry https://registry.example.com"
    )]
    Login(LoginArgs),
    /// Logout from a registry (remove stored token)
    #[command(
        after_help = "Examples:\n  fastskill auth logout\n  fastskill auth logout --registry https://registry.example.com"
    )]
    Logout(LogoutArgs),
    /// Show current authentication status
    #[command(after_help = "Examples:\n  fastskill auth whoami")]
    Whoami(WhoamiArgs),
}

/// Login command arguments
#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Registry URL to authenticate with
    #[arg(long)]
    pub registry: Option<String>,

    /// Role for the token (default: manager)
    #[arg(long, default_value = "manager")]
    pub role: String,
}

impl IntoCommandSpec for LoginArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Login to a skill registry",
            syntax: Some("auth login [OPTIONS]"),
            category: Some("auth"),
            args: vec![
                ArgSpec {
                    name: "registry",
                    kind: ArgKind::Option,
                    long: Some("registry"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Registry URL to authenticate with",
                    ..Default::default()
                },
                ArgSpec {
                    name: "role",
                    kind: ArgKind::Option,
                    long: Some("role"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Role for the token (default: manager)",
                    default: Some(ArgValue::Str("manager".to_string())),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for LoginArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            registry: map.get("registry").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            role: map
                .get("role")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "manager".to_string()),
        }
    }
}

/// Logout command arguments
#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// Registry URL to logout from
    #[arg(long)]
    pub registry: Option<String>,
}

impl IntoCommandSpec for LogoutArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Logout from a skill registry",
            syntax: Some("auth logout [OPTIONS]"),
            category: Some("auth"),
            args: vec![ArgSpec {
                name: "registry",
                kind: ArgKind::Option,
                long: Some("registry"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                help: "Registry URL to logout from",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for LogoutArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            registry: map.get("registry").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

/// Whoami command arguments
#[derive(Debug, Args)]
pub struct WhoamiArgs {
    /// Registry URL to check (default: from FASTSKILL_API_URL or http://localhost:8080)
    #[arg(long)]
    pub registry: Option<String>,
}

impl IntoCommandSpec for WhoamiArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Show current authenticated user",
            syntax: Some("auth whoami [OPTIONS]"),
            category: Some("auth"),
            args: vec![ArgSpec {
                name: "registry",
                kind: ArgKind::Option,
                long: Some("registry"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                help: "Registry URL to check (default: from FASTSKILL_API_URL or http://localhost:8080)",
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for WhoamiArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            registry: map.get("registry").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        }
    }
}

/// Execute login command
pub async fn execute_login(args: LoginArgs) -> CliResult<()> {
    // Determine registry URL
    let registry_url = args
        .registry
        .or_else(|| std::env::var("FASTSKILL_API_URL").ok())
        .unwrap_or_else(|| "http://localhost:8080".to_string());

    println!("Logging in to registry: {}", registry_url);

    // Call /auth/token endpoint
    let client = reqwest::Client::new();
    let token_url = format!("{}/auth/token", registry_url);

    let request_body = serde_json::json!({
        "role": args.role
    });

    let response = client
        .post(&token_url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| CliError::Config(format!("Failed to connect to registry: {}", e)))?;

    if !response.status().is_success() {
        return Err(CliError::Config(format!(
            "Login failed with status {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| CliError::Config(format!("Failed to parse response: {}", e)))?;

    let token = json
        .get("data")
        .and_then(|d| d.get("token"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| CliError::Config("Invalid token response format".to_string()))?;

    // Store token in config
    auth_config::set_token_for_registry(&registry_url, token, &args.role).await?;

    println!(
        "{}",
        messages::ok(&format!("Successfully logged in to {}", registry_url))
    );
    println!("Token saved and will be used automatically");

    Ok(())
}

/// Execute logout command
pub fn execute_logout(args: LogoutArgs) -> CliResult<()> {
    // Determine registry URL
    let registry_url = args
        .registry
        .or_else(|| std::env::var("FASTSKILL_API_URL").ok())
        .unwrap_or_else(|| "http://localhost:8080".to_string());

    // Remove token from config
    auth_config::remove_token_for_registry(&registry_url)?;

    println!(
        "{}",
        messages::ok(&format!("Successfully logged out from {}", registry_url))
    );

    Ok(())
}

/// Execute whoami command
pub fn execute_whoami(args: WhoamiArgs) -> CliResult<()> {
    // Determine registry URL
    let registry_url = args
        .registry
        .or_else(|| std::env::var("FASTSKILL_API_URL").ok())
        .unwrap_or_else(|| "http://localhost:8080".to_string());

    // Check if using environment variable
    if std::env::var("FASTSKILL_API_TOKEN").is_ok() {
        println!("Using token from FASTSKILL_API_TOKEN environment variable");
        println!("Registry: {}", registry_url);
        println!("Note: Environment variable tokens don't store user info");
        return Ok(());
    }

    // Get auth info from config
    match auth_config::get_auth_info_for_registry(&registry_url)? {
        Some(info) => {
            println!("Registry: {}", info.registry_url);
            if let Some(username) = &info.username {
                println!("Username: {}", username);
            }
            println!("Role: {}", info.role);
            if let Some(expires_at) = &info.expires_at {
                println!("Token expires: {}", expires_at);
            }
            if let Some(last_refresh) = &info.last_refresh {
                println!("Last refresh: {}", last_refresh);
            }
        }
        None => {
            println!("Not logged in to registry: {}", registry_url);
            println!("Run `fastskill auth login` to authenticate");
        }
    }

    Ok(())
}

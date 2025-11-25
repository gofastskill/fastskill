//! Authentication commands for CLI

use crate::cli::auth_config;
use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use clap::{Args, Subcommand};

/// Authentication command arguments
#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Login to a registry and store authentication token
    Login(LoginArgs),
    /// Logout from a registry (remove stored token)
    Logout(LogoutArgs),
    /// Show current authentication status
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

/// Logout command arguments
#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// Registry URL to logout from
    #[arg(long)]
    pub registry: Option<String>,
}

/// Whoami command arguments
#[derive(Debug, Args)]
pub struct WhoamiArgs {
    /// Registry URL to check (default: from FASTSKILL_API_URL or http://localhost:8080)
    #[arg(long)]
    pub registry: Option<String>,
}

/// Execute authentication command
pub async fn execute_auth(args: AuthArgs) -> CliResult<()> {
    match args.command {
        AuthCommand::Login(login_args) => execute_login(login_args).await,
        AuthCommand::Logout(logout_args) => execute_logout(logout_args),
        AuthCommand::Whoami(whoami_args) => execute_whoami(whoami_args),
    }
}

/// Execute login command
async fn execute_login(args: LoginArgs) -> CliResult<()> {
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
fn execute_logout(args: LogoutArgs) -> CliResult<()> {
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
fn execute_whoami(args: WhoamiArgs) -> CliResult<()> {
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

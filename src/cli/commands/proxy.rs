//! Proxy command - starts the FastSkill proxy server

use crate::cli::config::get_skills_toml_path;
use crate::cli::error::CliError;
use crate::cli::utils::messages;
use clap::Args;
use fastskill::core::service::FastSkillService;
use fastskill::http::proxy::{serve_proxy, ProxyCommandArgs, ProxyConfig};
use std::sync::Arc;

/// Start the FastSkill proxy server
#[derive(Debug, Args)]
pub struct ProxyCommand {
    /// Port for the proxy server (default: 8081)
    #[arg(long, default_value = "8081")]
    port: u16,

    /// Target Anthropic API URL (default: https://api.anthropic.com)
    #[arg(long, default_value = "https://api.anthropic.com")]
    target_url: String,

    /// Enable dynamic skill discovery mode
    #[arg(long)]
    enable_dynamic: bool,

    /// Maximum number of skills to inject in dynamic mode
    #[arg(long, default_value = "3")]
    max_dynamic_skills: usize,

    /// Request timeout in seconds
    #[arg(long, default_value = "300")]
    request_timeout_secs: u64,

    /// Enable request logging
    #[arg(long)]
    enable_logging: bool,
}

impl ProxyCommand {
    /// Execute the proxy command
    pub async fn execute(self, service: Arc<FastSkillService>) -> Result<(), CliError> {
        // Resolve skills directory from config
        let skills_dir = crate::cli::config::resolve_skills_storage_directory()?;

        // Try to load skills.toml for proxy config
        let manifest_path = get_skills_toml_path()?;

        // Convert ProxyCommand to ProxyCommandArgs
        let cli_args = ProxyCommandArgs {
            port: Some(self.port),
            target_url: self.target_url.clone(),
            enable_dynamic: if self.enable_dynamic {
                Some(true)
            } else {
                None
            },
            max_dynamic_skills: Some(self.max_dynamic_skills),
            request_timeout_secs: Some(self.request_timeout_secs),
            enable_logging: Some(self.enable_logging),
        };

        // Load config with fallback priority: manifest -> CLI args -> defaults
        let manifest_path_opt: Option<&std::path::Path> = if manifest_path.exists() {
            Some(manifest_path.as_path())
        } else {
            None
        };
        let mut config = ProxyConfig::load_with_fallback(manifest_path_opt, &cli_args)
            .map_err(|e| CliError::Config(format!("Failed to load proxy config: {}", e)))?;

        // Ensure skills_dir is set (from resolved path)
        config.skills_dir = skills_dir;

        println!("Starting FastSkill proxy server...");
        println!("  Proxy port: {}", config.port);
        println!("  Target API: {}", config.target_url);
        println!(
            "  Dynamic injection: {}",
            if config.enable_dynamic_injection {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!("  Skills directory: {}", config.skills_dir.display());
        println!();
        println!(
            "  {}",
            messages::info(&format!(
                "Point your Anthropic client to http://localhost:{}",
                config.port
            ))
        );
        println!(
            "  {}",
            messages::info("The proxy will intercept /v1/messages calls and inject skills")
        );
        println!();

        // Start the proxy server
        serve_proxy(config, service)
            .await
            .map_err(|e| CliError::Config(format!("Failed to start proxy server: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastskill::{FastSkillService, ServiceConfig};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_proxy_command_execute_with_default_args() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        let _service = Arc::new(service);

        let _proxy_cmd = ProxyCommand {
            port: 8081,
            target_url: "https://api.anthropic.com".to_string(),
            enable_dynamic: false,
            max_dynamic_skills: 3,
            request_timeout_secs: 300,
            enable_logging: false,
        };

        // Note: This test doesn't actually start the proxy server since it would block
        // We just verify the command can be created with valid args
        // The actual execute() call would block, so we skip it in unit tests
    }

    #[tokio::test]
    async fn test_proxy_command_with_dynamic_enabled() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        let _service = Arc::new(service);

        let _proxy_cmd = ProxyCommand {
            port: 9999,
            target_url: "https://api.anthropic.com".to_string(),
            enable_dynamic: true,
            max_dynamic_skills: 5,
            request_timeout_secs: 600,
            enable_logging: true,
        };

        // Verify args are accepted
    }

    #[tokio::test]
    async fn test_proxy_command_with_custom_target() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();
        let _service = Arc::new(service);

        let _proxy_cmd = ProxyCommand {
            port: 8081,
            target_url: "https://custom-api.example.com".to_string(),
            enable_dynamic: false,
            max_dynamic_skills: 3,
            request_timeout_secs: 300,
            enable_logging: false,
        };

        // Verify args are accepted
    }
}

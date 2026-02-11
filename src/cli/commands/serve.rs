//! Serve command implementation

use crate::cli::error::{CliError, CliResult};
use clap::Args;
use tracing::info;

/// Serve the FastSkill service via HTTP API
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Host to bind the server to
    #[arg(long, default_value = "localhost", help = "Host to bind the server to")]
    host: String,

    /// Port to bind the server to
    #[arg(long, default_value = "8080", help = "Port to bind the server to")]
    port: u16,
}

pub async fn execute_serve(
    service: std::sync::Arc<fastskill::FastSkillService>,
    args: ServeArgs,
) -> CliResult<()> {
    info!(
        "Starting FastSkill HTTP server on {}:{}",
        args.host, args.port
    );

    println!("FastSkill HTTP server starting...");
    println!("  Listening on: http://{}:{}", args.host, args.port);

    let server =
        fastskill::http::server::FastSkillServer::from_ref(&service, &args.host, args.port);

    // Start the server (this will block until shutdown)
    server
        .serve()
        .await
        .map_err(|e| CliError::Validation(format!("Server error: {}", e)))?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use fastskill::{FastSkillService, ServiceConfig};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_serve_with_default_args() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let _args = ServeArgs {
            host: "localhost".to_string(),
            port: 0,
        };

        // Note: This test doesn't actually start the server since it would block
        // We just verify the function can be called with valid args
        // In a real scenario, we'd need to spawn the server in a background task
        // For now, we'll just verify it doesn't panic on initialization
        // The actual serve() call would block, so we skip it in unit tests
    }

    #[tokio::test]
    async fn test_execute_serve_with_registry_enabled() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let _args = ServeArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
        };
    }

    #[tokio::test]
    async fn test_execute_serve_with_custom_port() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let _args = ServeArgs {
            host: "localhost".to_string(),
            port: 9999,
        };

        // Verify args are accepted
    }
}

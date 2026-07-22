//! Serve command implementation

use crate::error::{CliError, CliResult};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use tracing::info;

/// Serve the FastSkill service via HTTP API
#[derive(Debug)]
pub struct ServeArgs {
    /// Host to bind the server to
    host: String,

    /// Port to bind the server to
    port: u16,

    /// Enable mutating (write) endpoints. Read-only by default (ADR-0003).
    enable_write: bool,
}

impl IntoCommandSpec for ServeArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Start the FastSkill HTTP API server",
            syntax: Some("serve [OPTIONS]"),
            category: Some("server"),
            args: vec![
                ArgSpec {
                    name: "host",
                    long: Some("host"),
                    short: None,
                    help: "Host to bind the server to",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("localhost".to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "port",
                    long: Some("port"),
                    short: None,
                    help: "Port to bind the server to",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Int(8080)),
                    ..Default::default()
                },
                ArgSpec {
                    name: "enable-write",
                    long: Some("enable-write"),
                    short: None,
                    help: "Enable mutating (write) endpoints (read-only by default)",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for ServeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ServeArgs {
            host: map
                .get("host")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "localhost".to_string()),
            port: map
                .get("port")
                .and_then(|v| {
                    if let ArgValue::Int(n) = v {
                        Some(*n as u16)
                    } else {
                        None
                    }
                })
                .unwrap_or(8080),
            enable_write: map
                .get("enable-write")
                .map(|v| matches!(v, ArgValue::Bool(true)))
                .unwrap_or(false),
        }
    }
}

pub async fn execute_serve(
    global: bool,
    skills_dir: Option<std::path::PathBuf>,
    args: ServeArgs,
) -> CliResult<()> {
    info!(
        "Starting FastSkill HTTP server on {}:{} (write endpoints {})",
        args.host,
        args.port,
        if args.enable_write {
            "enabled"
        } else {
            "disabled"
        }
    );

    println!("FastSkill HTTP server starting...");
    if args.enable_write {
        println!("  Write endpoints: ENABLED (--enable-write)");
    } else {
        println!("  Write endpoints: disabled (read-only); pass --enable-write to enable");
    }

    // Build the served service directly (rather than reusing the CLI's cached
    // singleton via `FsState::service_with`) so this exact instance can carry
    // BOTH the edge-injected embedding provider + repository manager
    // (`inject_edge_services`, same as every other command) AND the served
    // project's root (ADR-0005 install seam): `add_from_origin`/`preflight`-driven
    // writes (via `POST /skills/install`, `/skills/update`) must land in the
    // served project's `skill-project.toml`/`skills.lock`, not wherever the
    // server process happens to have cwd set.
    let cfg = crate::config::create_service_config(global, skills_dir)?;
    let mut service = fastskill_core::FastSkillService::new(cfg)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;
    let mut service = crate::config::inject_edge_services(service)?;

    if let Ok(current_dir) = std::env::current_dir() {
        if let Ok(project_config) = fastskill_core::core::load_project_config(&current_dir) {
            service = service.with_project_root(project_config.project_root);
        }
    }

    let service = std::sync::Arc::new(service);

    let server =
        fastskill_core::http::server::FastSkillServer::from_ref(&service, &args.host, args.port)
            .enable_write(args.enable_write);

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
    use fastskill_core::{FastSkillService, ServiceConfig};
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
            enable_write: false,
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
            enable_write: false,
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
            enable_write: false,
        };

        // Verify args are accepted
    }
}

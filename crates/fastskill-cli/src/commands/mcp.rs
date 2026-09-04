//! `fastskill mcp serve` — the MCP server, with a runtime write gate.
//!
//! ADR-0003 makes read-only-by-default the rule for every surface that can
//! mutate state, not just HTTP. `fastskill serve` implements it as an Axum
//! middleware over the routes in [`fastskill_core::write_ops`]; this module
//! implements the same gate for MCP, over the *commands* in that same table:
//!
//! * without `--enable-write`, mutating tools are absent from `tools/list` and
//!   a `tools/call` naming one is refused with `MCP_TOOL_DENIED`;
//! * with `--enable-write`, they are listed and dispatched normally.
//!
//! The flag is spelled exactly as `fastskill serve --enable-write`, because it
//! means the same thing.
//!
//! ## Why this replaces cli-framework's built-in `mcp serve`
//!
//! `McpToolRegistry` keeps one map behind both `list_tools()` and
//! `resolve_tool()`, so inside the framework a tool can be hidden from the list
//! (`expose_mcp: false`) *or* refused with a custom message (an `ExecutionGate`),
//! never both: a hidden tool answers `tools/call` with "not registered", which
//! tells an operator nothing about the gate that hid it. Registering `mcp/serve`
//! here suppresses the framework's auto-registration (see `AppBuilder::build`)
//! and lets a thin [`WriteGatedHandler`] wrap the framework's own
//! `CliFrameworkHandler`, so the listing filter and the call refusal are driven
//! by one set of tool names. `mcp install` / `mcp list` still auto-register.

use cli_framework::app::context::AppContext;
use cli_framework::command::{CommandRegistry, FromArgValueMap, IntoCommandSpec};
use cli_framework::mcp::banner::{emit_banner, BannerData, BannerSettings};
use cli_framework::mcp::resources::ResourceRegistry;
use cli_framework::mcp::{
    CliFrameworkHandler, McpToolExportPolicy, McpToolRegistry, McpTransportKind,
};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::{CommandSpec, GroupMetadata};
use cli_framework::spec::value::ArgValue;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ErrorCode, ErrorData, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult,
    ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, OnceLock};

const DEFAULT_TRANSPORT: &str = "http";
const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: &str = "8080";
const DEFAULT_PATH: &str = "/mcp";

/// JSON-RPC code cli-framework uses for a tool refused before dispatch.
const MCP_TOOL_DENIED: i32 = -32005;

/// The command registry whose commands are exported as MCP tools.
///
/// `mcp serve` is itself a registered command, so its handler cannot capture the
/// registry at registration time — the registry does not exist yet. `main`
/// publishes it here once the app is built and before any command runs.
/// (cli-framework's own `mcp serve` solves the same problem by snapshotting the
/// registry inside `AppBuilder::build`.)
static COMMAND_REGISTRY: OnceLock<Arc<CommandRegistry>> = OnceLock::new();

/// Publish the built command registry for `mcp serve` to export as tools.
///
/// The first call wins; later calls are ignored, so a stray second publish
/// cannot swap the served tool set mid-run.
pub fn set_command_registry(registry: Arc<CommandRegistry>) {
    let _ = COMMAND_REGISTRY.set(registry);
}

fn command_registry() -> anyhow::Result<Arc<CommandRegistry>> {
    COMMAND_REGISTRY.get().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "internal error: the command registry was not published before `mcp serve` ran"
        )
    })
}

/// Metadata for the top-level `mcp` group node.
pub fn group_metadata() -> GroupMetadata {
    GroupMetadata {
        summary: "MCP server management",
        hidden: false,
    }
}

/// The MCP tool names that mutate state, derived from the one shared definition
/// in [`fastskill_core::write_ops`].
///
/// Tool names follow cli-framework's convention: `{app}_{command/path}`, with
/// `/` replaced by `_`.
pub fn mutating_tool_names(app_name: &str) -> HashSet<String> {
    fastskill_core::write_ops::write_command_paths()
        .map(|path| format!("{}_{}", app_name, path.join("_")))
        .collect()
}

/// Arguments to `fastskill mcp serve`.
///
/// Mirrors cli-framework's built-in spec (`--transport`, `--host`, `--port`,
/// `--path`) so the command reads identically, and adds `--enable-write`.
#[derive(Debug)]
pub struct McpServeArgs {
    transport: String,
    /// Raw values, kept as written so the http-only flags can be rejected when
    /// combined with `--transport stdio`. Spec defaults are injected before the
    /// handler runs, so "differs from the default" is what "the user set it"
    /// means here — the same test cli-framework's own `mcp serve` applies.
    host: String,
    port: String,
    path: String,
    /// Expose and allow the mutating tools. Off by default (ADR-0003).
    enable_write: bool,
}

impl IntoCommandSpec for McpServeArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Start the MCP server (http or stdio)",
            syntax: Some(
                "mcp serve [--transport http|stdio] [--host H] [--port P] [--path PATH] [--enable-write]",
            ),
            category: Some("mcp"),
            examples: vec![
                "fastskill mcp serve --transport stdio",
                "fastskill mcp serve --transport stdio --enable-write",
            ],
            args: vec![
                ArgSpec {
                    name: "transport",
                    long: Some("transport"),
                    short: None,
                    help: "Transport: http (Streamable HTTP) or stdio (stdin/stdout JSON-RPC)",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Enum(vec!["http", "stdio"]),
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Enum(DEFAULT_TRANSPORT.to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "host",
                    long: Some("host"),
                    short: None,
                    help: "Bind address for the MCP server",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str(DEFAULT_HOST.to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "port",
                    long: Some("port"),
                    short: None,
                    help: "Bind port for the MCP server",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str(DEFAULT_PORT.to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "path",
                    long: Some("path"),
                    short: None,
                    help: "HTTP path prefix for MCP endpoints",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str(DEFAULT_PATH.to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "enable-write",
                    long: Some("enable-write"),
                    short: None,
                    help: "Expose and allow mutating tools (install, remove, update, ...). \
                           Off by default: they are hidden from tools/list and refused.",
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

fn arg_str(map: &HashMap<String, ArgValue>, key: &str, fallback: &str) -> String {
    match map.get(key) {
        Some(ArgValue::Str(s) | ArgValue::Enum(s)) => s.clone(),
        _ => fallback.to_string(),
    }
}

impl FromArgValueMap for McpServeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            transport: arg_str(map, "transport", DEFAULT_TRANSPORT),
            host: arg_str(map, "host", DEFAULT_HOST),
            port: arg_str(map, "port", DEFAULT_PORT),
            path: arg_str(map, "path", DEFAULT_PATH),
            enable_write: matches!(map.get("enable-write"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Banner settings for this invocation, read from the context before the
/// handler's future is created (`AppContext` cannot cross an await point).
pub fn banner_settings(ctx: &dyn AppContext) -> BannerSettings {
    BannerSettings::resolve(ctx.opt_global_args(), &HashMap::new())
}

/// An MCP handler that hides and refuses mutating tools.
///
/// Everything except tool listing and dispatch is delegated verbatim to
/// cli-framework's handler.
#[derive(Clone)]
struct WriteGatedHandler {
    inner: CliFrameworkHandler,
    /// Tool names to hide and refuse. Empty when `--enable-write` was passed.
    blocked: Arc<HashSet<String>>,
}

impl WriteGatedHandler {
    fn denial(&self, tool: &str) -> ErrorData {
        ErrorData::new(
            ErrorCode(MCP_TOOL_DENIED),
            Cow::Owned(format!(
                "MCP_TOOL_DENIED: '{}' mutates state and is disabled on this server. \
                 Restart it as `fastskill mcp serve --enable-write` to allow write tools.",
                tool
            )),
            None,
        )
    }
}

impl ServerHandler for WriteGatedHandler {
    fn get_info(&self) -> ServerInfo {
        self.inner.get_info()
    }

    fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
        self.inner.list_resources(request, context)
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, ErrorData>> + Send + '_ {
        self.inner.read_resource(request, context)
    }

    fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        let inner = self.inner.list_tools(request, context);
        let blocked = Arc::clone(&self.blocked);
        async move {
            let mut result = inner.await?;
            result.tools.retain(|tool| !blocked.contains(&*tool.name));
            Ok(result)
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        let denied = if self.blocked.contains(&*request.name) {
            Some(self.denial(&request.name))
        } else {
            None
        };
        // Built unconditionally so the delegated future has a nameable type. It
        // is only polled when the call was not refused, and building it runs no
        // command — dispatch happens on first poll.
        let inner = self.inner.call_tool(request, context);
        async move {
            match denied {
                Some(error) => Err(error),
                None => inner.await,
            }
        }
    }
}

/// The tool set this server exports, honouring `expose_mcp` exactly as
/// cli-framework's own `mcp serve` does.
fn tool_registry(app_name: &str) -> anyhow::Result<Arc<McpToolRegistry>> {
    let registry = command_registry()?;
    Ok(Arc::new(
        McpToolRegistry::from_command_registry_with_policy(
            &registry,
            app_name,
            McpToolExportPolicy::ExposeMcpOnly,
        ),
    ))
}

/// Banner data with the gated tools removed, so the startup box lists exactly
/// what a client will see.
fn visible_banner(mut data: BannerData, blocked: &HashSet<String>) -> BannerData {
    data.tools.retain(|tool| !blocked.contains(&tool.name));
    data
}

/// Run `fastskill mcp serve`.
pub async fn execute_mcp_serve(
    app_name: &'static str,
    args: McpServeArgs,
    banner: BannerSettings,
) -> anyhow::Result<()> {
    let blocked: Arc<HashSet<String>> = Arc::new(if args.enable_write {
        HashSet::new()
    } else {
        mutating_tool_names(app_name)
    });
    let tools = tool_registry(app_name)?;
    let resources = Arc::new(ResourceRegistry::new());

    tracing::info!(
        "MCP: {} tools exported; {} write tools {}",
        tools.tool_count().saturating_sub(blocked.len()),
        blocked.len(),
        if args.enable_write {
            "enabled"
        } else {
            "hidden and refused (pass --enable-write to allow them)"
        }
    );

    if args.transport == "stdio" {
        if args.host != DEFAULT_HOST || args.port != DEFAULT_PORT || args.path != DEFAULT_PATH {
            return Err(anyhow::anyhow!(
                "[E004] invalid usage: '--host', '--port', and '--path' are only valid when --transport=http"
            ));
        }
        return serve_stdio(tools, resources, blocked, banner).await;
    }

    let port = args.port.parse::<u16>().map_err(|_| {
        anyhow::anyhow!(
            "[E004] invalid value '{}' for 'port'; expected u16 (0-65535)",
            args.port
        )
    })?;
    serve_http(
        tools, resources, blocked, banner, &args.host, port, &args.path,
    )
    .await
}

async fn serve_stdio(
    tools: Arc<McpToolRegistry>,
    resources: Arc<ResourceRegistry>,
    blocked: Arc<HashSet<String>>,
    banner: BannerSettings,
) -> anyhow::Result<()> {
    tracing::info!("MCP stdio server starting (stdin/stdout)");
    // The banner goes to stderr under stdio — stdout is the JSON-RPC channel.
    emit_banner(&visible_banner(BannerData::stdio(&tools), &blocked), banner);

    // Tool calls are serialized behind a mutex to keep concurrent replies from
    // interleaving on stdout, exactly as cli-framework's stdio transport does.
    let inner = CliFrameworkHandler::new(tools, McpTransportKind::Stdio)
        .with_resource_registry(resources)
        .with_stdio_serialization(Arc::new(tokio::sync::Mutex::new(())));
    let handler = WriteGatedHandler { inner, blocked };

    let running = rmcp::serve_server(handler, rmcp::transport::stdio())
        .await
        .map_err(|e| anyhow::anyhow!("MCP_STDIO_IO_ERROR: {}", e))?;
    let reason = running
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("MCP_STDIO_IO_ERROR: {}", e))?;
    tracing::info!("MCP stdio server stopped: {:?}", reason);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn serve_http(
    tools: Arc<McpToolRegistry>,
    resources: Arc<ResourceRegistry>,
    blocked: Arc<HashSet<String>>,
    banner: BannerSettings,
    host: &str,
    port: u16,
    path: &str,
) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port))
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "MCP_BIND_FAILED: address {}:{} already in use: {}",
                host,
                port,
                e
            )
        })?;

    tracing::info!("MCP server listening on http://{}:{}{}", host, port, path);
    emit_banner(
        &visible_banner(BannerData::http(host, port, path, &tools), &blocked),
        banner,
    );

    let service = StreamableHttpService::new(
        move || {
            Ok(WriteGatedHandler {
                inner: CliFrameworkHandler::new(Arc::clone(&tools), McpTransportKind::Http)
                    .with_resource_registry(Arc::clone(&resources)),
                blocked: Arc::clone(&blocked),
            })
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    // The service is mounted flat and nested at the declared prefix, mirroring
    // cli-framework's `start_streamable_http`.
    let inner = axum::Router::new()
        .route_service("/", service.clone())
        .route_service("/{*path}", service);
    let router = axum::Router::new().nest(path, inner);

    axum::serve(listener, router)
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutating_tool_names_cover_the_known_writers() {
        let names = mutating_tool_names("fastskill");
        for expected in [
            "fastskill_init",
            "fastskill_install",
            "fastskill_add",
            "fastskill_update",
            "fastskill_remove",
            "fastskill_reindex",
            "fastskill_repos_add",
            "fastskill_repos_remove",
            "fastskill_repos_update",
            "fastskill_repos_refresh",
            "fastskill_marketplace_create",
            "fastskill_optimize_run",
        ] {
            assert!(names.contains(expected), "{} was not gated", expected);
        }
    }

    #[test]
    fn read_only_tools_are_not_gated() {
        let names = mutating_tool_names("fastskill");
        for readonly in [
            "fastskill_list",
            "fastskill_read",
            "fastskill_search",
            "fastskill_doctor",
            "fastskill_repos_list",
        ] {
            assert!(!names.contains(readonly), "{} must stay exported", readonly);
        }
    }

    #[test]
    fn enable_write_defaults_to_off() {
        let args = McpServeArgs::from_arg_value_map(&HashMap::new());
        assert!(
            !args.enable_write,
            "the write gate must be closed unless --enable-write is passed"
        );
        assert_eq!(args.transport, DEFAULT_TRANSPORT);
    }
}

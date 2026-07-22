//! Axum HTTP server implementation

use crate::core::service::FastSkillService;
use crate::http::handlers::{
    manifest, registry, reindex, resolve, search, skills, status, AppState,
};
use crate::http::models::{ApiResponse, ErrorResponse};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use cli_framework::api::{ApiServerBuilder, ApiVersion, ApiVersionName, DefaultVersion, Stability};
use include_dir::{include_dir, Dir};
use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

/// Static assets embedded at compile time
static EMBEDDED_STATIC: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/http/static");

/// Write-gate middleware (ADR-0003 / WRITE-GATE).
///
/// Applied only to mutating routes. When `AppState::enable_write` is false, it
/// short-circuits with `403 Forbidden` and a plain message pointing the operator
/// at `--enable-write`. When writes are enabled it passes the request through.
async fn write_gate(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.enable_write {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::<()>::error(ErrorResponse {
                code: "FORBIDDEN".to_string(),
                message: "write operations disabled; start server with --enable-write".to_string(),
                details: None,
            })),
        )
            .into_response();
    }
    next.run(req).await
}

/// Serves embedded static files for the registry UI.
async fn serve_embedded_static(req: Request) -> Result<Response, StatusCode> {
    let path = req.uri().path().trim_start_matches('/');
    let name = match path {
        "" | "index.html" => "index.html",
        "app.js" => "app.js",
        "styles.css" => "styles.css",
        _ => return Err(StatusCode::NOT_FOUND),
    };
    let file = EMBEDDED_STATIC
        .get_file(name)
        .ok_or(StatusCode::NOT_FOUND)?;
    let body = file.contents();
    let content_type: HeaderValue = match name {
        "index.html" => HeaderValue::from_static("text/html; charset=utf-8"),
        "app.js" => HeaderValue::from_static("application/javascript; charset=utf-8"),
        "styles.css" => HeaderValue::from_static("text/css; charset=utf-8"),
        _ => HeaderValue::from_static("application/octet-stream"),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Get allowed methods for CORS
fn get_allowed_methods() -> [Method; 5] {
    [
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::OPTIONS,
    ]
}

/// Build default CORS layer (deny all origins)
fn build_default_cors_layer() -> CorsLayer {
    CorsLayer::new()
}

/// Parse origin strings to HeaderValues
fn parse_origins(origins: &[String]) -> Result<Vec<HeaderValue>, String> {
    origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin)
                .map_err(|e| format!("Invalid origin header value '{}': {}", origin, e))
        })
        .collect()
}

/// Parse header strings to HeaderNames
fn parse_headers(headers: &[String]) -> Result<Vec<HeaderName>, String> {
    headers
        .iter()
        .map(|header| {
            HeaderName::from_str(header)
                .map_err(|e| format!("Invalid header name '{}': {}", header, e))
        })
        .collect()
}

/// Build CORS layer with configured origins and headers
fn build_configured_cors_layer(
    origin_header_values: Vec<HeaderValue>,
    allowed_origins: &[String],
    allowed_headers: &[String],
) -> CorsLayer {
    info!(
        "Configuring CORS for {} origins: {}",
        origin_header_values.len(),
        allowed_origins.join(", ")
    );

    // SEC-10: never combine a wildcard origin with credentialed CORS. A literal
    // "*" in the origin list is inert for browsers when credentials are on
    // (they reject `*` + credentials), so this configuration is a foot-gun that
    // silently doesn't work. Fail loudly and fall back to deny-all instead.
    if allowed_origins.iter().any(|o| o == "*") {
        tracing::error!(
            "CORS allowed_origins contains \"*\" which cannot be combined with credentials; \
             denying all origins. Configure explicit origins instead of \"*\"."
        );
        return build_default_cors_layer();
    }

    let header_names = match parse_headers(allowed_headers) {
        Ok(values) => values,
        Err(e) => {
            tracing::error!("Failed to build CORS headers: {}", e);
            return CorsLayer::new()
                .allow_methods(get_allowed_methods())
                .allow_origin(AllowOrigin::list(origin_header_values))
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);
        }
    };

    CorsLayer::new()
        .allow_methods(get_allowed_methods())
        .allow_origin(AllowOrigin::list(origin_header_values))
        .allow_headers(header_names)
        .allow_credentials(true)
}

/// Build CORS layer from service configuration
pub fn build_cors_layer(config: &crate::core::service::ServiceConfig) -> CorsLayer {
    let http_config = match config.http_server.as_ref() {
        None => {
            info!("No CORS configuration found - denying all origins");
            return build_default_cors_layer();
        }
        Some(cfg) => cfg,
    };

    if http_config.allowed_origins.is_empty() {
        info!("Empty CORS allowed_origins - denying all origins");
        return build_default_cors_layer();
    }

    let origin_header_values = match parse_origins(&http_config.allowed_origins) {
        Ok(values) => values,
        Err(e) => {
            tracing::error!("Failed to build CORS origins: {}", e);
            return build_default_cors_layer();
        }
    };

    build_configured_cors_layer(
        origin_header_values,
        &http_config.allowed_origins,
        &http_config.allowed_headers,
    )
}

/// FastSkill HTTP server
pub struct FastSkillServer {
    service: Arc<FastSkillService>,
    addr: SocketAddr,
    /// When false (default), mutating routes are gated and return 403 (ADR-0003).
    enable_write: bool,
}

impl FastSkillServer {
    /// Create a new read-only server instance.
    ///
    /// Mutating routes are gated off by default; call [`Self::enable_write`] to
    /// turn them on.
    pub fn new(service: Arc<FastSkillService>, host: &str, port: u16) -> Self {
        let addr = match Self::parse_address(host, port) {
            Ok(addr) => addr,
            Err(e) => {
                eprintln!("Invalid address: {}:{} - {}", host, port, e);
                std::process::exit(1);
            }
        };

        Self {
            service,
            addr,
            enable_write: false,
        }
    }

    /// Choose whether mutating routes are enabled (ADR-0003 / WRITE-GATE).
    ///
    /// Mirrors `AppState::with_enable_write`.
    pub fn enable_write(mut self, enable_write: bool) -> Self {
        self.enable_write = enable_write;
        self
    }

    /// Parse and normalize host:port into a SocketAddr
    fn parse_address(host: &str, port: u16) -> Result<SocketAddr, String> {
        // Normalize common hostnames for SocketAddr compatibility
        let normalized_host = Self::normalize_host(host);

        // Format the address string - IPv6 addresses need brackets
        let addr_str = if normalized_host.contains(':') {
            // IPv6 address - wrap in brackets
            format!("[{}]:{}", normalized_host, port)
        } else {
            // IPv4 address or hostname
            format!("{}:{}", normalized_host, port)
        };

        addr_str.parse().map_err(|_| {
            format!(
                "Unable to parse address '{}'. Use IP addresses like '127.0.0.1', '0.0.0.0', '::1', or hostnames that resolve to IP addresses",
                addr_str
            )
        })
    }

    /// Normalize hostnames for SocketAddr compatibility
    fn normalize_host(host: &str) -> String {
        match host {
            // Convert localhost to 127.0.0.1 for dev machine compatibility
            "localhost" => "127.0.0.1".to_string(),
            // IPv6 localhost variants
            "::1" | "[::1]" => "::1".to_string(),
            // All IPv6 interfaces
            "::" | "[::]" => "::".to_string(),
            // Keep other values as-is (IP addresses, other hostnames)
            _ => host.to_string(),
        }
    }

    /// Create a new read-only server instance from an Arc-wrapped service reference.
    ///
    /// Mutating routes are gated off by default; call [`Self::enable_write`] to
    /// turn them on.
    pub fn from_ref(service: &Arc<FastSkillService>, host: &str, port: u16) -> Self {
        let service_arc = Arc::clone(service);

        let addr = match Self::parse_address(host, port) {
            Ok(addr) => addr,
            Err(e) => {
                eprintln!("Invalid address: {}:{} - {}", host, port, e);
                std::process::exit(1);
            }
        };

        Self {
            service: service_arc,
            addr,
            enable_write: false,
        }
    }

    /// READ routes under /api/v1/ — pure reads, always mounted (ADR-0003).
    ///
    /// list/get skills, project view, search, resolve, status, the registry
    /// browse (GET) routes, and the manifest read. Never mutate state.
    fn create_read_routes_v1() -> Router<AppState> {
        Router::new()
            .route("/skills", get(skills::list_skills))
            .route("/skills/{id}", get(skills::get_skill))
            .route("/skills/{id}/content", get(skills::get_skill_content))
            .route("/project", get(manifest::get_project))
            .route("/search", post(search::search_skills))
            .route("/resolve", post(resolve::resolve_context))
            .route("/status", get(status::status))
            .route("/registry/index/skills", get(registry::list_index_skills))
            .route("/registry/sources", get(registry::list_sources))
            .route("/registry/skills", get(registry::list_all_skills))
            .route(
                "/registry/sources/{name}/skills",
                get(registry::list_source_skills),
            )
            .route(
                "/registry/sources/{name}/marketplace",
                get(registry::get_marketplace),
            )
            .route("/manifest/skills", get(manifest::list_manifest_skills))
    }

    /// WRITE routes under /api/v1/ — anything that is not a pure read (ADR-0003).
    ///
    /// These paths are ALWAYS registered but wrapped in the write-gate middleware
    /// so they return 403 (not 404) when `--enable-write` is off. Includes:
    /// install/update/delete skills, reindex, registry refresh, and manifest
    /// mutators. (`POST /skills` create + `PUT /skills/{id}` field-edit removed
    /// per PARTIAL-1 / spec 003.) `/skills/upgrade` is kept mounted alongside
    /// `/skills/update` as a back-compat alias (spec 003 §2) — same handler.
    fn create_write_routes_v1() -> Router<AppState> {
        Router::new()
            .route("/skills/{id}", delete(skills::delete_skill))
            .route("/skills/install", post(skills::install_skill))
            .route("/skills/update", post(skills::update_skills))
            .route("/skills/upgrade", post(skills::update_skills))
            .route("/reindex", post(reindex::reindex_all))
            .route("/reindex/{id}", post(reindex::reindex_skill))
            .route("/registry/refresh", post(registry::refresh_sources))
            .route("/manifest/skills", post(manifest::add_skill_to_manifest))
            .route(
                "/manifest/skills/{id}",
                put(manifest::update_skill_in_manifest),
            )
            .route(
                "/manifest/skills/{id}",
                delete(manifest::remove_skill_from_manifest),
            )
    }

    /// Raw index routes for mount("/index") — paths relative to the mount point
    fn create_registry_index_routes_v1() -> Router<AppState> {
        Router::new().route("/{*skill_id}", get(registry::serve_index_file))
    }

    /// UI static file routes (unchanged paths, used as root_fallback)
    fn create_ui_routes() -> Router<AppState> {
        info!("Serving UI at /");
        Router::new()
            .route("/dashboard", get(status::root))
            .route("/", get(serve_embedded_static))
            .route("/index.html", get(serve_embedded_static))
            .route("/app.js", get(serve_embedded_static))
            .route("/styles.css", get(serve_embedded_static))
    }

    /// Start the server using cli-framework ApiServerBuilder
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        // Load project configuration (same as previous create_router logic)
        let current_dir = env::current_dir()?;
        let project_config = crate::core::load_project_config(&current_dir).ok();
        if project_config.is_none() {
            tracing::warn!(
                "No skill-project.toml found in {} or any parent. \
                 Project-level features (manifest, skills install) will be unavailable. \
                 Run `fastskill init` in your project root to create one.",
                current_dir.display()
            );
        }

        let mut state = AppState::new(self.service.clone())?;
        if let Some(cfg) = project_config {
            state = state.with_project_config(
                cfg.project_root,
                cfg.project_file_path,
                cfg.skills_directory,
            );
        }
        state = state.with_enable_write(self.enable_write);

        // WRITE routes are always registered, but wrapped in the write-gate
        // middleware so they return 403 (discoverable) rather than 404 when
        // writes are disabled (ADR-0003 / WRITE-GATE).
        let write_router = Self::create_write_routes_v1()
            .route_layer(middleware::from_fn_with_state(state.clone(), write_gate));

        // Build versioned v1 router with compression (applied to fastskill routes only)
        let v1_router = Router::new()
            .merge(Self::create_read_routes_v1())
            .merge(write_router)
            .layer(TraceLayer::new_for_http())
            .layer(CompressionLayer::new())
            .with_state(state.clone());

        // Raw index surface mounted at /index (unchanged URL contract)
        let index_router = Self::create_registry_index_routes_v1().with_state(state.clone());

        // Console UI served as root fallback
        let ui_router = Self::create_ui_routes().with_state(state.clone());

        let cors_layer = build_cors_layer(self.service.config());

        let server = ApiServerBuilder::new()
            .version(ApiVersion {
                name: ApiVersionName::parse("v1")?,
                router: v1_router,
                stability: Stability::Stable,
                deprecation: None,
            })
            .default_version(DefaultVersion::Pinned(ApiVersionName::parse("v1")?))
            .mount("/index", index_router)
            .cors(cors_layer)
            .root_fallback(ui_router)
            .health_version(env!("CARGO_PKG_VERSION"))
            .build();

        info!("Starting FastSkill HTTP server on {}", self.addr);

        let addr_str = self.addr.to_string();
        println!("  Listening on: http://{}", self.addr);
        server.serve(&addr_str).await?;

        Ok(())
    }

    /// Get server address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

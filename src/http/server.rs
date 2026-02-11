//! Axum HTTP server implementation

use crate::core::registry::{StagingManager, ValidationWorker, ValidationWorkerConfig};
use crate::core::service::{FastSkillService, ServiceConfig};
use crate::http::handlers::{
    auth, claude_api, manifest, registry, registry_publish, reindex, search, skills, status,
    AppState,
};
use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, Method, StatusCode},
    response::Response,
    routing::{delete, get, post, put},
    Router,
};
use include_dir::{include_dir, Dir};
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::info;

/// Static assets embedded at compile time
static EMBEDDED_STATIC: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/http/static");

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

/// Validate registry configuration and return detailed error message if invalid
fn validate_registry_config(config: &ServiceConfig) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut incomplete_fields = Vec::new();

    // Check registry_blob_storage
    match &config.registry_blob_storage {
        None => {
            missing.push("registry_blob_storage");
        }
        Some(blob_config) => {
            // Validate required fields in blob storage config
            let mut blob_missing = Vec::new();

            if blob_config.storage_type.is_empty() {
                blob_missing.push("storage_type");
            }
            if blob_config.bucket.is_empty() {
                blob_missing.push("bucket");
            }
            if blob_config.region.is_empty() {
                blob_missing.push("region");
            }
            if blob_config.access_key.is_empty() {
                blob_missing.push("access_key");
            }
            if blob_config.secret_key.is_empty() {
                blob_missing.push("secret_key");
            }

            if !blob_missing.is_empty() {
                incomplete_fields.push(("registry_blob_storage", blob_missing));
            }
        }
    }

    // Build error message
    if missing.is_empty() && incomplete_fields.is_empty() {
        return Ok(());
    }

    let mut error_msg =
        String::from("Registry enabled but required configuration is missing or incomplete:\n");

    if !missing.is_empty() {
        error_msg.push_str("  Missing configuration:\n");
        for item in &missing {
            error_msg.push_str(&format!("    - {}\n", item));
        }
    }

    if !incomplete_fields.is_empty() {
        error_msg.push_str("  Incomplete configuration:\n");
        for (config_name, fields) in &incomplete_fields {
            error_msg.push_str(&format!("    {} is missing fields:\n", config_name));
            for field in fields {
                error_msg.push_str(&format!("      - {}\n", field));
            }
        }
    }

    error_msg.push_str("\nS3 configuration is required for operational registry publishing.");

    Err(error_msg)
}

/// Display startup banner with ASCII art
fn display_startup_banner() {
    // TODO: Implement startup banner display
    let _version = crate::VERSION;
}

/// FastSkill HTTP server
pub struct FastSkillServer {
    service: Arc<FastSkillService>,
    addr: SocketAddr,
}

impl FastSkillServer {
    /// Create a new server instance
    pub fn new(service: Arc<FastSkillService>, host: &str, port: u16) -> Self {
        let addr = match Self::parse_address(host, port) {
            Ok(addr) => addr,
            Err(e) => {
                eprintln!("Invalid address: {}:{} - {}", host, port, e);
                std::process::exit(1);
            }
        };

        Self { service, addr }
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

    /// Create a new server instance from an Arc-wrapped service reference
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
        }
    }

    /// Create the Axum router with all routes
    fn create_router(&self) -> Result<Router, Box<dyn std::error::Error>> {
        // Load project configuration
        let current_dir = env::current_dir()?;
        let config = crate::core::load_project_config(&current_dir)
            .map_err(|e| format!("Failed to load project config: {}", e))?;

        let state = AppState::new(self.service.clone())?.with_project_config(
            config.project_root,
            config.project_file_path,
            config.skills_directory,
        );

        let mut router = Router::new()
            // Skills CRUD endpoints
            .route("/api/skills", get(skills::list_skills))
            .route("/api/skills", post(skills::create_skill))
            .route("/api/skills/:id", get(skills::get_skill))
            .route("/api/skills/:id", put(skills::update_skill))
            .route("/api/skills/:id", delete(skills::delete_skill))
            .route("/api/skills/upgrade", post(skills::upgrade_skills))
            .route("/api/project", get(manifest::get_project))
            // Search endpoint
            .route("/api/search", post(search::search_skills))
            // Reindex endpoints
            .route("/api/reindex", post(reindex::reindex_all))
            .route("/api/reindex/:id", post(reindex::reindex_skill))
            // Auth endpoints
            .route("/auth/token", post(auth::generate_token))
            .route("/auth/verify", get(auth::verify_token))
            // Status endpoint
            .route("/api/status", get(status::status))
            // Claude Code v1 API endpoints
            .route("/v1/skills", post(claude_api::create_skill))
            .route("/v1/skills", get(claude_api::list_skills))
            .route("/v1/skills/:skill_id", get(claude_api::get_skill))
            .route("/v1/skills/:skill_id", delete(claude_api::delete_skill))
            .route(
                "/v1/skills/:skill_id/versions",
                post(claude_api::create_skill_version),
            )
            .route(
                "/v1/skills/:skill_id/versions",
                get(claude_api::list_skill_versions),
            )
            .route(
                "/v1/skills/:skill_id/versions/:version",
                get(claude_api::get_skill_version),
            )
            .route(
                "/v1/skills/:skill_id/versions/:version",
                delete(claude_api::delete_skill_version),
            );

        // Installed-skills UI at / (always from embedded static); dashboard at /dashboard
        info!("Serving UI at /");
        router = router
            .route("/dashboard", get(status::root))
            .route("/", get(serve_embedded_static))
            .route("/index.html", get(serve_embedded_static))
            .route("/app.js", get(serve_embedded_static))
            .route("/styles.css", get(serve_embedded_static));

        // Registry and manifest API routes (always available)
        router = router
            .route("/index/*skill_id", get(registry::serve_index_file))
            .route(
                "/api/registry/index/skills",
                get(registry::list_index_skills),
            )
            .route("/api/registry/sources", get(registry::list_sources))
            .route("/api/registry/skills", get(registry::list_all_skills))
            .route(
                "/api/registry/sources/:name/skills",
                get(registry::list_source_skills),
            )
            .route(
                "/api/registry/sources/:name/marketplace",
                get(registry::get_marketplace),
            )
            .route("/api/registry/refresh", post(registry::refresh_sources))
            .route(
                "/api/registry/publish",
                post(registry_publish::publish_package),
            )
            .route(
                "/api/registry/publish/status/:job_id",
                get(registry_publish::get_publish_status),
            )
            .route("/api/manifest/skills", get(manifest::list_manifest_skills))
            .route(
                "/api/manifest/skills",
                post(manifest::add_skill_to_manifest),
            )
            .route(
                "/api/manifest/skills/:id",
                put(manifest::update_skill_in_manifest),
            )
            .route(
                "/api/manifest/skills/:id",
                delete(manifest::remove_skill_from_manifest),
            );

        // Add middleware and state
        Ok(router
            .layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(CompressionLayer::new())
                    .layer(
                        CorsLayer::new()
                            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                            .allow_headers(Any)
                            .allow_origin(Any), // TODO: Configure CORS properly
                    ),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.jwt_service.clone(),
                crate::http::auth::middleware::auth_middleware,
            ))
            .with_state(state))
    }

    /// Start the server
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        // Display startup banner
        display_startup_banner();

        let app = self.create_router()?;

        // Start validation worker only when registry config is valid
        let config = self.service.config();
        if validate_registry_config(config).is_ok() {
            let staging_dir = config
                .staging_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from(".staging"));

            let staging_manager = StagingManager::new(staging_dir);
            if staging_manager.initialize().is_ok() {
                let worker_config = ValidationWorkerConfig {
                    poll_interval_secs: 5,
                    blob_storage_config: config.registry_blob_storage.clone(),
                    registry_index_path: config.registry_index_path.clone(),
                    blob_base_url: config.registry_blob_base_url.clone(),
                };
                let worker = ValidationWorker::new(staging_manager, worker_config);
                worker.start();
                info!("Validation worker started");
            }
        }

        info!("Starting FastSkill HTTP server on {}", self.addr);

        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        let actual_addr = listener.local_addr()?;
        info!("Server bound to {}", actual_addr);

        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Get server address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

/// Convenience function to create and start a server
pub async fn serve(
    service: Arc<FastSkillService>,
    host: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = FastSkillServer::new(service, host, port);
    server.serve().await
}

//! Axum HTTP server implementation

use crate::core::registry::{StagingManager, ValidationWorker, ValidationWorkerConfig};
use crate::core::service::{FastSkillService, ServiceConfig};
use crate::http::handlers::{
    auth, claude_api, manifest, registry, registry_publish, reindex, search, skills, status,
    AppState,
};
use axum::{
    http::Method,
    response::Html,
    routing::{delete, get, post, put},
    Router,
};
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::{compression::CompressionLayer, services::ServeDir, trace::TraceLayer};
use tracing::{info, warn};

/// Resolve skills.toml path (similar to CLI config resolution)
fn resolve_skills_toml_path() -> PathBuf {
    // Priority 1: Environment variable
    if let Ok(env_path) = env::var("FASTSKILL_SKILLS_TOML_PATH") {
        return PathBuf::from(env_path);
    }

    // Priority 2: Walk up directory tree to find .claude/skills.toml
    if let Ok(current_dir) = env::current_dir() {
        let mut current = current_dir.clone();
        loop {
            let skills_toml = current.join(".claude/skills.toml");
            if skills_toml.is_file() {
                return skills_toml.canonicalize().unwrap_or(skills_toml);
            }

            if !current.pop() {
                break;
            }
        }
    }

    // Priority 3: Default to current directory
    PathBuf::from(".claude/skills.toml")
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
    enable_registry: bool,
    auto_generate_mdc: bool,
}

impl FastSkillServer {
    /// Create a new server instance
    pub fn new(
        service: Arc<FastSkillService>,
        host: &str,
        port: u16,
        enable_registry: bool,
    ) -> Self {
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
            enable_registry,
            auto_generate_mdc: false,
        }
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

    /// Resolve the static directory path for the registry UI
    ///
    /// This function tries multiple strategies to find the static directory:
    /// 1. Uses CARGO_MANIFEST_DIR (works in development and when built from source)
    /// 2. Tries relative paths from current directory
    /// 3. Tries paths relative to executable location
    ///
    /// This ensures the registry UI works regardless of where the binary is run from.
    pub fn resolve_static_dir() -> PathBuf {
        // Try multiple strategies to find the static directory

        // Strategy 0: Check environment variable (highest priority)
        if let Ok(env_path) = std::env::var("FASTSKILL_STATIC_DIR") {
            let static_path = PathBuf::from(env_path);
            if static_path.exists() {
                info!(
                    "Resolved static directory (from FASTSKILL_STATIC_DIR): {}",
                    static_path.display()
                );
                return static_path;
            }
        }

        // Strategy 1: Use CARGO_MANIFEST_DIR (works in development and when built from source)
        #[allow(unused_mut)]
        let mut static_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        static_path.push("src");
        static_path.push("http");
        static_path.push("static");

        if static_path.exists() {
            info!(
                "Resolved static directory (CARGO_MANIFEST_DIR): {}",
                static_path.display()
            );
            return static_path;
        }

        // Strategy 2: Try direct relative path from current directory
        if let Ok(current_dir) = std::env::current_dir() {
            let test_path = current_dir.join("src/http/static");
            if test_path.exists() {
                info!(
                    "Resolved static directory (relative to current dir): {}",
                    test_path.display()
                );
                return test_path;
            }

            // Strategy 2b: Try relative to current directory by walking up to find the crate
            let mut search_dir = current_dir.clone();
            // Walk up to 10 levels to find the src/http/static directory
            for _ in 0..10 {
                let test_path = search_dir.join("src/http/static");
                if test_path.exists() {
                    info!(
                        "Resolved static directory (relative to current dir, walking up): {}",
                        test_path.display()
                    );
                    return test_path;
                }
                if !search_dir.pop() {
                    break;
                }
            }
        }

        // Strategy 3: Try relative to executable location
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let mut exe_relative = exe_dir.to_path_buf();
                exe_relative.push("static");
                if exe_relative.exists() {
                    info!(
                        "Resolved static directory (relative to executable): {}",
                        exe_relative.display()
                    );
                    return exe_relative;
                }

                // Try going up a few directories
                for _ in 0..5 {
                    exe_relative.pop();
                    let mut test_path = exe_relative.clone();
                    test_path.push("src");
                    test_path.push("http");
                    test_path.push("static");
                    if test_path.exists() {
                        info!(
                            "Resolved static directory (relative to executable, going up): {}",
                            test_path.display()
                        );
                        return test_path;
                    }
                }
            }
        }

        // Fallback: return the compile-time path (will fail at runtime if not found, but at least we tried)
        warn!(
            "Could not find static directory, using compile-time path: {}",
            static_path.display()
        );
        static_path
    }

    /// Serve the registry index.html file
    async fn serve_registry_index(
        static_dir: PathBuf,
    ) -> Result<Html<String>, axum::http::StatusCode> {
        let index_path = static_dir.join("index.html");

        match tokio::fs::read_to_string(&index_path).await {
            Ok(content) => Ok(Html(content)),
            Err(e) => {
                warn!(
                    "Failed to read index.html from {}: {}",
                    index_path.display(),
                    e
                );
                Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Create a new server instance from a service reference
    pub fn from_ref(
        service: &FastSkillService,
        host: &str,
        port: u16,
        enable_registry: bool,
        auto_generate_mdc: bool,
    ) -> Self {
        // Create Arc from reference (safe because we know the service lives long enough)
        let service_arc = unsafe {
            Arc::increment_strong_count(service);
            Arc::from_raw(service as *const FastSkillService)
        };

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
            enable_registry,
            auto_generate_mdc,
        }
    }

    /// Create the Axum router with all routes
    fn create_router(&self) -> Result<Router, Box<dyn std::error::Error>> {
        // Resolve skills.toml path
        // Try to use the config module if available, otherwise default
        let skills_toml_path = resolve_skills_toml_path();

        let state = AppState::new(self.service.clone())
            .with_skills_toml_path(skills_toml_path)
            .with_auto_generate_mdc(self.auto_generate_mdc);

        let mut router = Router::new()
            // Root endpoint
            .route("/", get(status::root))
            // Skills CRUD endpoints
            .route("/api/skills", get(skills::list_skills))
            .route("/api/skills", post(skills::create_skill))
            .route("/api/skills/:id", get(skills::get_skill))
            .route("/api/skills/:id", put(skills::update_skill))
            .route("/api/skills/:id", delete(skills::delete_skill))
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

        // Add registry routes if enabled
        if self.enable_registry {
            // Resolve static directory path relative to crate root
            let static_dir = Self::resolve_static_dir();

            if !static_dir.exists() {
                warn!(
                    "Registry static directory not found at: {}. Registry UI may not work correctly.",
                    static_dir.display()
                );
            } else {
                info!("Serving registry UI from: {}", static_dir.display());

                // Verify key files exist
                let index_path = static_dir.join("index.html");
                let styles_path = static_dir.join("styles.css");
                let app_js_path = static_dir.join("app.js");

                if !index_path.exists() {
                    warn!("index.html not found at: {}", index_path.display());
                }
                if !styles_path.exists() {
                    warn!("styles.css not found at: {}", styles_path.display());
                }
                if !app_js_path.exists() {
                    warn!("app.js not found at: {}", app_js_path.display());
                }
            }

            // Clone static_dir for the handler closures
            let static_dir_for_index = static_dir.clone();
            let static_dir_for_index2 = static_dir.clone();

            // Create a nested router for registry UI
            // The route "/" handles /registry/ (with trailing slash)
            // The fallback_service handles all other paths under /registry/ and serves static files
            // If a file is not found, it falls back to serving index.html (SPA behavior)
            let registry_router = Router::new()
                .route(
                    "/",
                    get(move || {
                        let static_dir = static_dir_for_index.clone();
                        async move {
                            Self::serve_registry_index(static_dir)
                                .await
                                .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }),
                )
                .fallback_service(ServeDir::new(static_dir.clone()));

            router = router
                // Registry index file serving (flat layout: /index/{scope}/{skill-name})
                .route("/index/*skill_id", get(registry::serve_index_file))
                .route(
                    "/api/registry/index/skills",
                    get(registry::list_index_skills),
                )
                // Registry API endpoints
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
                // Registry publish endpoints
                .route(
                    "/api/registry/publish",
                    post(registry_publish::publish_package),
                )
                .route(
                    "/api/registry/publish/status/:job_id",
                    get(registry_publish::get_publish_status),
                )
                // Manifest API endpoints
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
                )
                .route("/api/manifest/generate-mdc", post(manifest::generate_mdc))
                // Explicit route for /registry (without trailing slash) to serve index.html
                .route(
                    "/registry",
                    get(move || {
                        let static_dir = static_dir_for_index2.clone();
                        async move {
                            Self::serve_registry_index(static_dir)
                                .await
                                .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                        }
                    }),
                )
                // Registry UI router (handles /registry/ and static files)
                .nest_service("/registry/", registry_router);
        }

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
            .with_state(state))
    }

    /// Start the server
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        // Display startup banner
        display_startup_banner();

        let app = self.create_router()?;

        // Start validation worker if registry is enabled
        if self.enable_registry {
            let config = self.service.config();

            // Validate required configuration for operational registry
            if let Err(err_msg) = validate_registry_config(config) {
                return Err(err_msg.into());
            }

            let staging_dir =
                config.staging_dir.clone().unwrap_or_else(|| PathBuf::from(".staging"));

            let staging_manager = StagingManager::new(staging_dir);
            staging_manager
                .initialize()
                .map_err(|e| format!("Failed to initialize staging: {}", e))?;

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
    enable_registry: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = FastSkillServer::new(service, host, port, enable_registry);
    server.serve().await
}

//! FastSkill proxy server implementation

use crate::core::service::FastSkillService;
use crate::http::proxy::{config::ProxyConfig, handler::ProxyHandler, ProxyService};
use axum::{
    extract::Request,
    http::Method,
    routing::{any, get},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::info;

/// FastSkill proxy server
pub struct ProxyServer {
    config: ProxyConfig,
    #[allow(dead_code)] // Reserved for future functionality
    service: Arc<dyn ProxyService>,
    handler: Arc<ProxyHandler>,
}

impl ProxyServer {
    /// Create a new proxy server instance
    pub fn new(
        config: ProxyConfig,
        service: Arc<dyn ProxyService>,
    ) -> Result<Self, reqwest::Error> {
        let handler = Arc::new(ProxyHandler::new(config.clone(), service.clone())?);

        Ok(Self {
            config,
            service,
            handler,
        })
    }

    /// Create the Axum router with proxy routes
    fn create_router(&self) -> Router {
        let handler = Arc::clone(&self.handler);
        Router::new()
            // Health check endpoint
            .route("/health", get(|| async { "Proxy server is running" }))
            // Catch-all route for proxying requests
            .route(
                "/*path",
                any(move |req: Request| {
                    let handler = Arc::clone(&handler);
                    async move { handler.handle_request(req).await }
                }),
            )
            .layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(CompressionLayer::new())
                    .layer(
                        CorsLayer::new()
                            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                            .allow_headers(Any)
                            .allow_origin(Any), // TODO: Configure CORS properly for proxy
                    ),
            )
    }

    /// Start the proxy server
    pub async fn serve(&self) -> Result<(), Box<dyn std::error::Error>> {
        let app = self.create_router();
        let addr = SocketAddr::from(([0, 0, 0, 0], self.config.port));

        info!("Starting FastSkill proxy server on {}", addr);
        info!("Target Anthropic API: {}", self.config.target_url);
        info!(
            "Dynamic injection: {}",
            self.config.enable_dynamic_injection
        );
        info!("Skills directory: {}", self.config.skills_dir.display());

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Get server address
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from(([0, 0, 0, 0], self.config.port))
    }
}

/// Convenience function to create and start a proxy server
pub async fn serve_proxy(
    config: ProxyConfig,
    service: Arc<FastSkillService>,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = ProxyServer::new(config, service)?;
    server.serve().await
}

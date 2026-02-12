//! Axum middleware for JWT authentication

use crate::http::auth::{jwt::JwtService, roles::UserRole};
use crate::http::errors::HttpError;
use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

/// Authentication middleware for Axum
#[derive(Clone)]
pub struct AuthMiddleware {
    jwt_service: Arc<JwtService>,
    required_role: Option<UserRole>,
}

impl AuthMiddleware {
    /// Create middleware requiring a specific role
    pub fn require_role(jwt_service: Arc<JwtService>, role: UserRole) -> Self {
        Self {
            jwt_service,
            required_role: Some(role),
        }
    }

    /// Create middleware that allows any authenticated user
    pub fn authenticated(jwt_service: Arc<JwtService>) -> Self {
        Self {
            jwt_service,
            required_role: None,
        }
    }

    /// Extract and validate JWT token from request
    pub fn extract_token(req: &Request) -> Option<String> {
        // Try Authorization header first
        if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if let Some(stripped) = auth_str.strip_prefix("Bearer ") {
                    return Some(stripped.to_string());
                }
            }
        }

        // Try X-API-Key header as fallback
        if let Some(api_key) = req.headers().get("x-api-key") {
            if let Ok(key_str) = api_key.to_str() {
                return Some(key_str.to_string());
            }
        }

        None
    }

    /// Authenticate request and extract user info
    pub fn authenticate(&self, req: &Request) -> Result<Option<UserRole>, HttpError> {
        let token = Self::extract_token(req).ok_or_else(|| {
            HttpError::Unauthorized("No authentication token provided".to_string())
        })?;

        let claims = self.jwt_service.validate_token(&token)?;

        let user_role = UserRole::parse_role(&claims.role)?;

        // Check if user has required role
        if let Some(required) = &self.required_role {
            if !user_role.includes(required) {
                return Err(HttpError::Forbidden(format!(
                    "Insufficient permissions. Required: {}, Got: {}",
                    required, user_role
                )));
            }
        }

        Ok(Some(user_role))
    }
}

/// Check if a route is public (does not require authentication)
fn is_public_route(method: &Method, path: &str) -> bool {
    match (method, path) {
        // POST /auth/token - token endpoint
        (&Method::POST, "/auth/token") => true,

        // GET / - root endpoint
        (&Method::GET, "/") => true,

        // GET /dashboard - dashboard
        (&Method::GET, "/dashboard") => true,

        // Static assets
        (&Method::GET, "/index.html") => true,
        (&Method::GET, "/app.js") => true,
        (&Method::GET, "/styles.css") => true,

        // Registry index endpoints (read-only)
        (&Method::GET, path) if path.starts_with("/index/") => true,
        (&Method::GET, "/api/registry/index/skills") => true,
        (&Method::GET, "/api/registry/sources") => true,
        (&Method::GET, "/api/registry/skills") => true,
        (&Method::GET, path) if path.starts_with("/api/registry/sources/") => true,

        // All other routes require authentication
        _ => false,
    }
}

/// Axum middleware function that enforces JWT authentication
pub async fn auth_middleware(
    State(jwt_service): State<Arc<JwtService>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // Check if this is a public route
    if is_public_route(&method, &path) {
        // For public routes, attach anonymous AuthContext and proceed
        let mut req = req;
        req.extensions_mut().insert(AuthContext::anonymous());
        return Ok(next.run(req).await);
    }

    // For protected routes, extract and validate token
    let token = AuthMiddleware::extract_token(&req);
    let user_role_and_id = match token {
        Some(token_str) => match jwt_service.validate_token(&token_str) {
            Ok(claims) => match UserRole::parse_role(&claims.role) {
                Ok(role) => Some((role, claims.sub)),
                Err(_) => {
                    return Err(StatusCode::UNAUTHORIZED);
                }
            },
            Err(_) => {
                return Err(StatusCode::UNAUTHORIZED);
            }
        },
        None => {
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Attach AuthContext to request extensions
    if let Some((user_role, user_id)) = user_role_and_id {
        let mut req = req;
        req.extensions_mut()
            .insert(AuthContext::authenticated(user_id, user_role));
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Authentication state for request context
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_role: Option<UserRole>,
    pub user_id: Option<String>,
}

impl AuthContext {
    pub fn new(user_role: Option<UserRole>, user_id: Option<String>) -> Self {
        Self { user_role, user_id }
    }

    pub fn anonymous() -> Self {
        Self::new(None, None)
    }

    pub fn authenticated(user_id: String, role: UserRole) -> Self {
        Self::new(Some(role), Some(user_id))
    }
}

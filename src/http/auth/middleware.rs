//! Axum middleware for JWT authentication

use crate::http::auth::{jwt::JwtService, roles::UserRole};
use crate::http::errors::HttpError;
use axum::{
    extract::Request,
    http::{header, StatusCode},
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

/// Axum middleware function
pub async fn auth_middleware(req: Request, next: Next) -> Result<Response, StatusCode> {
    // For now, we'll skip authentication in middleware and handle it in handlers
    // This allows for flexible per-endpoint auth requirements
    // TODO: Implement proper middleware with state

    Ok(next.run(req).await)
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

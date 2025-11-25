//! Authentication endpoint handlers

use crate::http::auth::{jwt::JwtService, roles::EndpointPermissions};
use crate::http::errors::{HttpError, HttpResult};
use crate::http::handlers::AppState;
use crate::http::models::*;
use axum::{extract::State, Json};
use validator::Validate;

/// POST /auth/token - Generate JWT token (for local development)
pub async fn generate_token(
    State(_state): State<AppState>,
    Json(request): Json<TokenRequest>,
) -> HttpResult<axum::Json<ApiResponse<TokenResponse>>> {
    // Check permissions (public for local dev)
    let _check = EndpointPermissions::AUTH_TOKEN.check(None);

    // Validate request
    request.validate().map_err(|e| {
        HttpError::ValidationError(
            e.field_errors()
                .into_iter()
                .map(|(field, errors)| {
                    (
                        field.to_string(),
                        errors
                            .iter()
                            .map(|e| e.message.clone().unwrap_or_default().to_string())
                            .collect(),
                    )
                })
                .collect(),
        )
    })?;

    // For now, we'll create a simple JWT service instance
    // In a real implementation, this would be part of the app state
    let jwt_service = JwtService::from_env()
        .map_err(|e| HttpError::InternalServerError(format!("JWT service error: {:?}", e)))?;

    let response = jwt_service.generate_dev_token(&request)?;

    Ok(axum::Json(ApiResponse::success(response)))
}

/// GET /auth/verify - Verify JWT token
pub async fn verify_token(
    State(_state): State<AppState>,
    // TODO: Extract token from Authorization header
) -> HttpResult<axum::Json<ApiResponse<VerifyResponse>>> {
    // Check permissions
    let _check = EndpointPermissions::AUTH_VERIFY.check(None);

    // For now, return a mock response
    let response = VerifyResponse {
        valid: true,
        role: Some("manager".to_string()),
        expires_at: Some("2024-12-31T23:59:59Z".to_string()),
    };

    Ok(axum::Json(ApiResponse::success(response)))
}

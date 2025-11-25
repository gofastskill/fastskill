//! HTTP error handling and conversion

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::HashMap;

/// HTTP error types
#[derive(Debug, Clone)]
pub enum HttpError {
    /// Authentication errors
    Unauthorized(String),
    Forbidden(String),

    /// Validation errors
    BadRequest(String),
    ValidationError(HashMap<String, Vec<String>>),

    /// Not found errors
    NotFound(String),

    /// Conflict errors
    Conflict(String),

    /// Server errors
    InternalServerError(String),

    /// Service-specific errors
    ServiceError(String),
}

impl HttpError {
    /// Convert to HTTP status code
    pub fn status_code(&self) -> StatusCode {
        match self {
            HttpError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            HttpError::Forbidden(_) => StatusCode::FORBIDDEN,
            HttpError::BadRequest(_) | HttpError::ValidationError(_) => StatusCode::BAD_REQUEST,
            HttpError::NotFound(_) => StatusCode::NOT_FOUND,
            HttpError::Conflict(_) => StatusCode::CONFLICT,
            HttpError::InternalServerError(_) | HttpError::ServiceError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    /// Get error code string
    pub fn error_code(&self) -> &'static str {
        match self {
            HttpError::Unauthorized(_) => "UNAUTHORIZED",
            HttpError::Forbidden(_) => "FORBIDDEN",
            HttpError::BadRequest(_) => "BAD_REQUEST",
            HttpError::ValidationError(_) => "VALIDATION_ERROR",
            HttpError::NotFound(_) => "NOT_FOUND",
            HttpError::Conflict(_) => "CONFLICT",
            HttpError::InternalServerError(_) => "INTERNAL_SERVER_ERROR",
            HttpError::ServiceError(_) => "SERVICE_ERROR",
        }
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            HttpError::Forbidden(msg) => write!(f, "Forbidden: {}", msg),
            HttpError::BadRequest(msg) => write!(f, "Bad Request: {}", msg),
            HttpError::ValidationError(errors) => {
                write!(f, "Validation Error: {:?}", errors)
            }
            HttpError::NotFound(msg) => write!(f, "Not Found: {}", msg),
            HttpError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            HttpError::InternalServerError(msg) => write!(f, "Internal Server Error: {}", msg),
            HttpError::ServiceError(msg) => write!(f, "Service Error: {}", msg),
        }
    }
}

impl std::error::Error for HttpError {}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_code = self.error_code();

        let (message, details) = match self {
            HttpError::ValidationError(errors) => {
                ("Validation failed".to_string(), Some(json!(errors)))
            }
            HttpError::Unauthorized(msg)
            | HttpError::Forbidden(msg)
            | HttpError::BadRequest(msg)
            | HttpError::NotFound(msg)
            | HttpError::Conflict(msg)
            | HttpError::InternalServerError(msg)
            | HttpError::ServiceError(msg) => (msg, None),
        };

        let body = Json(json!({
            "success": false,
            "error": {
                "code": error_code,
                "message": message,
                "details": details
            }
        }));

        (status, body).into_response()
    }
}

/// Convert service errors to HTTP errors
impl From<crate::core::service::ServiceError> for HttpError {
    fn from(err: crate::core::service::ServiceError) -> Self {
        match err {
            crate::core::service::ServiceError::SkillNotFound(msg) => HttpError::NotFound(msg),
            crate::core::service::ServiceError::Validation(msg) => HttpError::BadRequest(msg),
            crate::core::service::ServiceError::Config(msg) => HttpError::InternalServerError(msg),
            crate::core::service::ServiceError::Io(err) => {
                HttpError::InternalServerError(err.to_string())
            }
            crate::core::service::ServiceError::Custom(msg) => HttpError::ServiceError(msg),
            crate::core::service::ServiceError::Storage(msg) => HttpError::InternalServerError(msg),
            crate::core::service::ServiceError::Execution(_) => {
                HttpError::InternalServerError("Execution error".to_string())
            }
            crate::core::service::ServiceError::Event(msg) => HttpError::InternalServerError(msg),
            crate::core::service::ServiceError::InvalidOperation(msg) => HttpError::BadRequest(msg),
        }
    }
}

/// Result type alias for HTTP operations
pub type HttpResult<T> = Result<T, HttpError>;

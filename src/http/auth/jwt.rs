//! JWT token management

use crate::http::errors::{HttpError, HttpResult};
use crate::http::models::{Claims, TokenRequest, TokenResponse};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// JWT service for token management
#[derive(Clone)]
pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    issuer: String,
    token_expiry: Duration,
}

impl JwtService {
    /// Create a new JWT service
    pub fn new(secret: &str, issuer: &str, token_expiry_seconds: u64) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            issuer: issuer.to_string(),
            token_expiry: Duration::from_secs(token_expiry_seconds),
        }
    }

    /// Create a new JWT service from environment
    pub fn from_env() -> HttpResult<Self> {
        let secret = std::env::var("FASTSKILL_JWT_SECRET")
            .unwrap_or_else(|_| "dev-secret-key-change-in-production".to_string());

        let issuer =
            std::env::var("FASTSKILL_JWT_ISSUER").unwrap_or_else(|_| "fastskill".to_string());

        let expiry_seconds = std::env::var("FASTSKILL_JWT_EXPIRY")
            .unwrap_or_else(|_| "3600".to_string())
            .parse::<u64>()
            .map_err(|_| {
                HttpError::InternalServerError("Invalid JWT expiry configuration".to_string())
            })?;

        Ok(Self::new(&secret, &issuer, expiry_seconds))
    }

    /// Generate a JWT token for a user
    pub fn generate_token(&self, subject: &str, role: &str) -> HttpResult<TokenResponse> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HttpError::InternalServerError("Time went backwards".to_string()))?;

        let expiry = now + self.token_expiry;

        let claims = Claims {
            sub: subject.to_string(),
            role: role.to_string(),
            exp: expiry.as_secs() as usize,
            iat: now.as_secs() as usize,
            iss: self.issuer.clone(),
        };

        let token = encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|_| HttpError::InternalServerError("Failed to generate token".to_string()))?;

        Ok(TokenResponse {
            token,
            token_type: "Bearer".to_string(),
            expires_in: self.token_expiry.as_secs() as i64,
            role: role.to_string(),
        })
    }

    /// Validate and decode a JWT token
    pub fn validate_token(&self, token: &str) -> HttpResult<Claims> {
        let validation = Validation::default();
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)
            .map_err(|_| HttpError::Unauthorized("Invalid token".to_string()))?;

        Ok(token_data.claims)
    }

    /// Generate a development token (for local testing)
    pub fn generate_dev_token(&self, request: &TokenRequest) -> HttpResult<TokenResponse> {
        let subject = request
            .username
            .clone()
            .unwrap_or_else(|| "dev-user".to_string());
        self.generate_token(&subject, &request.role)
    }
}

// Claims structure is defined in models.rs

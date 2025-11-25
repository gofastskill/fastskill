//! JWT authentication and authorization

pub mod jwt;
pub mod middleware;
pub mod roles;

pub use crate::http::models::Claims;
/// Re-export commonly used auth types
pub use jwt::JwtService;
pub use middleware::AuthMiddleware;
pub use roles::UserRole;

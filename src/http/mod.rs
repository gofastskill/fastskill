//! HTTP server implementation for FastSkill
//!
//! This module provides a REST API server using Axum with full CRUD operations
//! for skills management.

pub mod errors;
pub mod handlers;
pub mod models;
pub mod server;

pub use models::{ApiResponse, ErrorResponse};
/// Re-export commonly used types
pub use server::FastSkillServer;

//! HTTP request handlers

pub mod auth;
pub mod claude_api;
pub mod manifest;
pub mod registry;
pub mod registry_publish;
pub mod reindex;
pub mod search;
pub mod skills;
pub mod status;

// Re-export AppState (used by all handlers)
pub use status::AppState;

// Note: Handler functions are accessed via module paths (e.g., skills::list_skills)
// to avoid ambiguous re-exports between skills and claude_api modules

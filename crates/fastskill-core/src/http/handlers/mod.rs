//! HTTP request handlers

pub mod manifest;
pub mod registry;
pub mod reindex;
pub mod resolve;
pub mod search;
pub mod skills;
pub mod status;

// Re-export AppState (used by all handlers)
pub use status::AppState;

// Note: Handler functions are accessed via module paths (e.g., skills::list_skills)
// to avoid ambiguous re-exports between modules.

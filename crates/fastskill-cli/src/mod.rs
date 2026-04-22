//! CLI module - re-export all submodules

#[allow(clippy::module_inception)]
pub mod auth_config;
#[allow(clippy::module_inception)]
pub mod cli;
pub mod commands;
pub mod config;
pub mod config_file;
pub mod error;
pub mod utils;

// Re-export main types for convenience
pub use cli::Cli;

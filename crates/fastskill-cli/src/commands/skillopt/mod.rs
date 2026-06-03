//! SkillOpt command group — iterative skill-document optimization via text-gradient.
//!
//! All subcommands are registered individually via `AppBuilder::register::<T>()` in main.rs.
//! The per-subcommand Args structs implement `IntoCommandSpec + FromArgValueMap` directly.

pub mod config;
pub mod export;
pub mod inspect;
pub mod resume;
pub mod run;
pub mod status;

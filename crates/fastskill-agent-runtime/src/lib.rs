//! Agent runtime discovery and selection for FastSkill.
//!
//! This crate provides primitives for resolving `--agent` and `--all` CLI flags
//! into a validated, deduplicated set of runtime IDs via `aikit_sdk::runnable_agents()`.
//! It is intentionally kept independent of `fastskill-core` and `fastskill-evals`.

pub mod runtime_selector;
pub use runtime_selector::*;

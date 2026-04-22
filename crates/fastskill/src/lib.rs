//! # FastSkill - AI Skills Management Toolkit
//!
//! This crate is a facade that re-exports `fastskill-core` for backward compatibility.
//!
//! For new code, consider using `fastskill-core` directly.
//!
//! ## Migration Guide
//!
//! To migrate from this facade to `fastskill-core`:
//!
//! ```toml
//! # Before
//! [dependencies]
//! fastskill = "0.9"
//!
//! # After
//! [dependencies]
//! fastskill-core = "0.9"
//! ```
//!
//! Then update imports:
//!
//! ```rust,ignore
//! // Before
//! use fastskill::{FastSkillService, ServiceConfig};
//!
//! // After
//! use fastskill_core::{FastSkillService, ServiceConfig};
//! ```

// Re-export everything from fastskill-core
pub use fastskill_core::*;

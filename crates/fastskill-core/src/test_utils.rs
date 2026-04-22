//! Test utilities shared across the crate
//!
//! This module provides common test helpers that can be used by both unit tests
//! in src/ and integration tests in tests/.

use std::sync::Mutex;

/// Global mutex for serializing tests that change the current working directory.
/// Tests that call `std::env::set_current_dir()` should acquire this lock to prevent
/// race conditions when running tests in parallel.
pub static DIR_MUTEX: Mutex<()> = Mutex::new(());

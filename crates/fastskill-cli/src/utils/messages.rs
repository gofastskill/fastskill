//! Message formatting utilities for consistent CLI output

/// Format a success message
pub fn ok(msg: &str) -> String {
    format!("[OK] {}", msg)
}

/// Format an error message
pub fn error(msg: &str) -> String {
    format!("[ERROR] {}", msg)
}

/// Format a warning message
pub fn warning(msg: &str) -> String {
    format!("[WARNING] {}", msg)
}

/// Format an info message
pub fn info(msg: &str) -> String {
    format!("[INFO] {}", msg)
}

/// Format a progress message
#[allow(dead_code)]
pub fn progress(current: usize, total: usize, msg: &str) -> String {
    format!("[{}/{}] {}", current, total, msg)
}

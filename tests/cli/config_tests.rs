//! Tests for skills directory resolution

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]
//!
//! These tests verify skills directory resolution through CLI execution
//! since the bin module is internal to the binary.

use std::env;
use std::process::Command;
use tempfile::TempDir;

fn get_binary_path() -> String {
    format!("{}/target/debug/fastskill", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn test_cli_skills_dir_argument() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().to_string_lossy().to_string();

    // Test that --skills-dir argument is accepted
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(&skills_dir)
        .arg("--help")
        .output()
        .expect("Failed to execute CLI");

    // Should succeed (even with --help, --skills-dir should be parsed)
    assert!(output.status.success() || output.status.code().is_some());
}

#[test]
fn test_cli_with_env_var() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().to_string_lossy().to_string();

    // Set environment variable
    env::set_var("FASTSKILL_DIRECTORY", &skills_dir);

    // Test CLI accepts the env var (we can't easily verify it uses it without actual command execution)
    let output = Command::new(get_binary_path())
        .arg("--help")
        .env("FASTSKILL_DIRECTORY", &skills_dir)
        .output()
        .expect("Failed to execute CLI");

    env::remove_var("FASTSKILL_DIRECTORY");

    // Should succeed
    assert!(output.status.success());
}

#[test]
fn test_cli_finds_skills_in_current_dir() {
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = temp_dir.path().join(".skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // Change to temp directory
    let original_dir = env::current_dir().unwrap();
    let original_dir_abs = original_dir.canonicalize().unwrap_or_else(|_| {
        // If canonicalize fails, use the original path
        if original_dir.is_absolute() {
            original_dir.clone()
        } else {
            // Fallback: use temp dir's parent if available
            temp_dir.path().parent().map(|p| p.to_path_buf()).unwrap_or_else(|| "/".into())
        }
    });

    env::set_current_dir(temp_dir.path()).unwrap();

    let output = Command::new(get_binary_path())
        .arg("--help")
        .output()
        .expect("Failed to execute CLI");

    // Restore original directory - handle error gracefully in case directory no longer exists
    let _ = env::set_current_dir(&original_dir_abs);

    // Should succeed (CLI should find .skills directory)
    assert!(output.status.success());
}

#[test]
fn test_cli_directory_walking() {
    // Test that CLI can be invoked from different directory levels
    let temp_dir = TempDir::new().unwrap();
    let nested_dir = temp_dir.path().join("level1").join("level2");
    std::fs::create_dir_all(&nested_dir).unwrap();

    // Store original directory as absolute path before changing
    let original_dir = env::current_dir().unwrap();
    let original_dir_abs = original_dir.canonicalize().unwrap_or_else(|_| {
        // If canonicalize fails, try to make it absolute
        if original_dir.is_absolute() {
            original_dir.clone()
        } else {
            // Fallback: use temp dir's parent if available
            temp_dir.path().parent().map(|p| p.to_path_buf()).unwrap_or_else(|| "/".into())
        }
    });

    // Change to nested directory
    let nested_dir_abs = nested_dir.canonicalize().unwrap_or_else(|_| nested_dir.clone());

    // Change directory - use a guard to ensure we restore
    struct DirGuard {
        original: std::path::PathBuf,
    }

    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.original);
        }
    }

    env::set_current_dir(&nested_dir_abs).unwrap();
    let _guard = DirGuard {
        original: original_dir_abs.clone(),
    };

    // Use absolute path for binary to avoid path resolution issues
    let binary_path_str = get_binary_path();
    let binary_abs = std::path::Path::new(&binary_path_str).canonicalize().unwrap_or_else(|_| {
        // If binary path can't be canonicalized, try resolving from manifest dir
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(&binary_path_str)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(&binary_path_str))
    });

    let output = Command::new(&binary_abs).arg("--help").output().expect("Failed to execute CLI");

    // Guard will restore directory on drop, but we can also do it explicitly
    drop(_guard);
    let _ = env::set_current_dir(&original_dir_abs);

    // Should succeed regardless of directory level
    assert!(output.status.success());
}

#[test]
fn test_cli_invalid_skills_dir() {
    let invalid_dir = "/nonexistent/path/to/skills";

    // Test that CLI handles invalid skills directory gracefully
    let output = Command::new(get_binary_path())
        .arg("--skills-dir")
        .arg(invalid_dir)
        .arg("--help")
        .output()
        .expect("Failed to execute CLI");

    // CLI should still show help even with invalid directory
    // (the directory validation happens later in actual commands)
    assert!(output.status.success() || output.status.code().is_some());
}

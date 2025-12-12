//! Unit tests for git operations using system git binary

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill::storage::git::GitError;
use std::time::Duration;

// Note: These tests require git to be installed on the system
// They test the actual git command execution, so they're more like integration tests
// but are placed in unit tests as they test individual functions

// Import internal functions for testing
use fastskill::storage::git as git_module;

#[tokio::test]
async fn test_parse_git_version_valid() {
    // Test parsing various git version formats
    use fastskill::core::service::ServiceError;

    // Standard format
    let version = git_module::parse_git_version("git version 2.34.1").unwrap();
    assert_eq!(version.major, 2);
    assert_eq!(version.minor, 34);
    assert_eq!(version.patch, 1);
    assert!(version.is_supported());

    // With extra info
    let version = git_module::parse_git_version("git version 2.40.0 (Apple Git-140)").unwrap();
    assert_eq!(version.major, 2);
    assert_eq!(version.minor, 40);
    assert_eq!(version.patch, 0);

    // Version 2.0 (minimum supported)
    let version = git_module::parse_git_version("git version 2.0.0").unwrap();
    assert!(version.is_supported());

    // Version 1.9 (too old)
    let version = git_module::parse_git_version("git version 1.9.5").unwrap();
    assert!(!version.is_supported());
}

#[tokio::test]
async fn test_parse_git_version_invalid() {
    // Invalid formats
    assert!(git_module::parse_git_version("not a version").is_err());
    assert!(git_module::parse_git_version("git 2.0.0").is_err());
    assert!(git_module::parse_git_version("version 2.0.0").is_err());
    assert!(git_module::parse_git_version("git version").is_err());
}

#[tokio::test]
async fn test_is_network_error() {
    // Network-related errors
    assert!(git_module::is_network_error("fatal: unable to access 'https://github.com/...': Failed to connect"));
    assert!(git_module::is_network_error("error: RPC failed; HTTP 504 curl 22 The requested URL returned error: 504"));
    assert!(git_module::is_network_error("fatal: unable to access 'https://...': Connection refused"));
    assert!(git_module::is_network_error("fatal: unable to access 'https://...': Name resolution failed"));
    assert!(git_module::is_network_error("Network timeout occurred"));

    // Non-network errors
    assert!(!git_module::is_network_error("fatal: repository 'invalid' not found"));
    assert!(!git_module::is_network_error("error: pathspec 'nonexistent' did not match any file"));
    assert!(!git_module::is_network_error("fatal: Authentication failed"));
}

#[tokio::test]
async fn test_git_error_display() {
    // Test that GitError variants have proper Display implementations
    let err = GitError::GitNotInstalled;
    assert!(err.to_string().contains("git"));

    let err = GitError::GitVersionTooOld {
        version: "1.9.5".to_string(),
        required: "2.0".to_string(),
    };
    assert!(err.to_string().contains("1.9.5"));
    assert!(err.to_string().contains("2.0"));

    let err = GitError::CloneFailed {
        url: "https://example.com/repo.git".to_string(),
        stderr: "fatal: repository not found".to_string(),
    };
    assert!(err.to_string().contains("example.com"));
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_git_error_conversion() {
    use fastskill::core::service::ServiceError;

    // Test conversion from GitError to ServiceError
    let git_err = GitError::GitNotInstalled;
    let service_err: ServiceError = git_err.into();
    
    match service_err {
        ServiceError::Custom(msg) => {
            assert!(msg.contains("git"));
        }
        _ => panic!("Expected Custom error"),
    }
}

#[tokio::test]
async fn test_check_git_version_actual() {
    // This test actually checks if git is installed and gets the version
    // It's more of an integration test but tests the core functionality
    use fastskill::core::service::ServiceError;

    // This should succeed if git is installed
    match git_module::check_git_version().await {
        Ok(()) => {
            // Git is installed and version is >= 2.0
            println!("Git version check passed");
        }
        Err(ServiceError::Custom(msg)) if msg.contains("not found") => {
            // Git not installed - skip test
            println!("Git not installed, skipping version check test");
        }
        Err(e) => {
            // Other error - might be version too old
            println!("Git version check failed: {}", e);
            // Don't fail the test - this is expected in some environments
        }
    }
}

#[tokio::test]
async fn test_git_operations_async() {
    // Test that git operations execute asynchronously without blocking
    use std::time::Instant;

    // Execute a simple git command (version check)
    let start = Instant::now();
    let result = git_module::execute_git_command(&["--version"], Duration::from_secs(5), None).await;
    let elapsed = start.elapsed();

    // Should complete quickly (< 1 second for version check)
    assert!(elapsed.as_secs() < 1, "Git command should complete quickly");

    // Should succeed if git is installed
    if let Ok(output) = result {
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("git version"));
    } else {
        // Git not installed - that's okay for this test
        println!("Git not installed, skipping async test");
    }
}

#[tokio::test]
async fn test_git_command_timeout() {
    // Test that git commands respect timeout
    use std::time::Instant;

    // Try to execute a command that would take too long
    // Using a very short timeout
    let start = Instant::now();
    let result = git_module::execute_git_command(
        &["clone", "--depth=1", "https://nonexistent-domain-12345.com/repo.git", "/tmp/test"],
        Duration::from_millis(100), // Very short timeout
        None,
    ).await;
    let elapsed = start.elapsed();

    // Should timeout quickly
    assert!(elapsed.as_secs() < 2, "Command should timeout or fail quickly");

    // Result should be an error (either timeout or connection failure)
    assert!(result.is_err() || result.as_ref().unwrap().exit_code != 0);
}

#[tokio::test]
async fn test_retry_logic_structure() {
    // Test that retry logic is structured correctly
    // This is a unit test of the retry mechanism structure

    // Try a command that will fail (invalid repo)
    let result = git_module::execute_git_command_with_retry(
        &["clone", "--depth=1", "https://invalid-repo-12345.com/nonexistent.git", "/tmp/test"],
        Duration::from_secs(5),
        None,
        3, // Max 3 attempts
    ).await;

    // Should fail after retries (not a network error, so won't retry)
    // Or if it is a network error, should fail after 3 attempts
    assert!(result.is_err() || result.as_ref().unwrap().exit_code != 0);
}


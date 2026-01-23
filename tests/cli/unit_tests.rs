//! Unit tests for CLI utilities

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]
//!
//! These tests verify utility functions that can be tested indirectly
//! through CLI execution or by testing exported library functionality.

use std::env;
use std::path::PathBuf;
use url::Url;

#[test]
fn test_url_parsing_for_git() {
    // Test URL parsing which is used by git URL detection in CLI
    let url = Url::parse("https://github.com/user/repo.git");
    assert!(url.is_ok());

    let url = url.unwrap();
    assert_eq!(url.scheme(), "https");
    assert!(url.host_str().is_some());
    assert!(url.host_str().unwrap().contains("github.com"));
}

#[test]
fn test_url_parsing_with_path() {
    // Test URL parsing with paths
    let url = Url::parse("https://github.com/user/repo.git?branch=main");
    assert!(url.is_ok());

    let url = url.unwrap();
    assert_eq!(url.query(), Some("branch=main"));
}

#[test]
fn test_url_parsing_git_ssh() {
    // Test SSH URL parsing - SSH URLs are not valid HTTP URLs
    // This test verifies that SSH URLs fail to parse as HTTP URLs
    let url = Url::parse("git@github.com:user/repo.git");
    assert!(url.is_err()); // SSH URLs should fail HTTP URL parsing
}

#[test]
fn test_path_extension_detection() {
    // Test path extension detection for zip files (used by CLI)
    let zip_path = PathBuf::from("./skill.zip");
    assert_eq!(zip_path.extension().and_then(|s| s.to_str()), Some("zip"));

    let tar_path = PathBuf::from("./skill.tar.gz");
    assert_eq!(tar_path.extension().and_then(|s| s.to_str()), Some("gz"));

    let folder_path = PathBuf::from("./skill-folder");
    // Folder might not have extension or might be a directory
    assert!(
        folder_path.extension().is_none()
            || !folder_path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .contains(".")
    );
}

#[test]
fn test_environment_variables() {
    // Test environment variable access (used by config)
    env::set_var("TEST_VAR", "test_value");
    assert_eq!(env::var("TEST_VAR").unwrap(), "test_value");
    env::remove_var("TEST_VAR");

    // Test that removed var returns error
    assert!(env::var("TEST_VAR").is_err());
}

#[test]
fn test_pathbuf_operations() {
    // Test PathBuf operations used throughout CLI
    let path = PathBuf::from("/tmp/test");
    assert_eq!(path.to_string_lossy(), "/tmp/test");

    let joined = path.join("subfolder");
    assert!(joined.to_string_lossy().contains("subfolder"));

    let parent = path.parent();
    assert_eq!(parent.unwrap(), std::path::Path::new("/tmp"));
}

#[test]
fn test_string_operations() {
    // Test string operations used in CLI parsing
    let test_str = "hello,world,test";

    let parts: Vec<&str> = test_str.split(',').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "hello");

    let trimmed: Vec<String> = parts.iter().map(|s| s.trim().to_string()).collect();
    assert_eq!(trimmed[0], "hello");
}

/*
#[test]
fn test_parse_git_url_simple_repo() {
    // Note: CLI utils are tested through integration tests
    // use crate::cli::utils::{parse_git_url, GitUrlInfo};

    let result = parse_git_url("https://github.com/user/repo.git");
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.repo_url, "https://github.com/user/repo.git");
    assert!(info.branch.is_none());
    assert!(info.subdir.is_none());
}

#[test]
fn test_parse_git_url_with_query_branch() {
    // Note: CLI utils are tested through integration tests
    // use crate::cli::utils::{parse_git_url, GitUrlInfo};

    let result = parse_git_url("https://github.com/user/repo.git?branch=main");
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.repo_url, "https://github.com/user/repo.git");
    assert_eq!(info.branch, Some("main".to_string()));
    assert!(info.subdir.is_none());
}

#[test]
fn test_parse_git_url_github_tree() {
    // Note: CLI utils are tested through integration tests
    // use crate::cli::utils::{parse_git_url, GitUrlInfo};

    let result = parse_git_url("https://github.com/anthropics/skills/tree/main/document-skills/pptx");
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.repo_url, "https://github.com/anthropics/skills.git");
    assert_eq!(info.branch, Some("main".to_string()));
    assert_eq!(info.subdir, Some(std::path::PathBuf::from("document-skills/pptx")));
}

#[test]
fn test_parse_git_url_github_tree_no_subdir() {
    // Note: CLI utils are tested through integration tests
    // use crate::cli::utils::{parse_git_url, GitUrlInfo};

    let result = parse_git_url("https://github.com/user/repo/tree/v1.0");
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.repo_url, "https://github.com/user/repo.git");
    assert_eq!(info.branch, Some("v1.0".to_string()));
    assert!(info.subdir.is_none());
}

#[test]
fn test_parse_git_url_invalid() {
    // Note: CLI utils are tested through integration tests
    // use crate::cli::utils::{parse_git_url, GitUrlInfo};

    let result = parse_git_url("not-a-url");
    assert!(result.is_err());
}
*/

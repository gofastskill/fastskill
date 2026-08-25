use super::*;

#[test]
fn test_build_clone_args_disables_line_ending_translation() {
    // Git for Windows defaults core.autocrlf=true, which would rewrite LF
    // to CRLF on checkout and make an installed skill differ from the
    // published source (and from the same skill installed from a zip).
    let args = build_clone_args("https://example.com/r.git", "/dest", None, None);
    assert!(args.windows(2).any(|w| w == ["-c", "core.autocrlf=false"]));
    assert!(args.windows(2).any(|w| w == ["-c", "core.eol=lf"]));
    // Config must precede the subcommand or git rejects it.
    let clone_at = args.iter().position(|a| *a == "clone").unwrap();
    let autocrlf_at = args
        .iter()
        .position(|a| *a == "core.autocrlf=false")
        .unwrap();
    assert!(autocrlf_at < clone_at);
}

#[test]
fn test_build_clone_args_includes_protocol_flags_and_end_of_options() {
    let args = build_clone_args("https://example.com/repo.git", "/tmp/dest", None, None);

    // Protocol allowlist (SEC-11): ext:: and file:// transports disabled.
    assert!(args
        .windows(2)
        .any(|w| w == ["-c", "protocol.ext.allow=never"]));
    assert!(args
        .windows(2)
        .any(|w| w == ["-c", "protocol.file.allow=never"]));

    // The protocol -c flags precede the `clone` subcommand.
    let clone_pos = args.iter().position(|a| *a == "clone").unwrap();
    let ext_pos = args
        .iter()
        .position(|a| *a == "protocol.ext.allow=never")
        .unwrap();
    assert!(ext_pos < clone_pos);

    // `--` end-of-options separator immediately precedes the positional url/dest.
    let dd = args.iter().position(|a| *a == "--").unwrap();
    assert_eq!(args[dd + 1], "https://example.com/repo.git");
    assert_eq!(args[dd + 2], "/tmp/dest");
    assert_eq!(dd + 3, args.len(), "url and dest must be the final args");
}

#[test]
fn test_build_clone_args_url_cannot_be_read_as_flag() {
    // A url that begins with '-' must sit after `--` so git can't parse it as an option.
    let args = build_clone_args("--upload-pack=evil", "/tmp/dest", None, None);
    let dd = args.iter().position(|a| *a == "--").unwrap();
    assert!(args[dd + 1].starts_with('-'));
    assert_eq!(args[dd + 1], "--upload-pack=evil");
}

#[test]
fn test_build_clone_args_with_branch() {
    let args = build_clone_args("https://h/r.git", "/d", Some("main"), None);
    assert!(args.windows(2).any(|w| w == ["--branch", "main"]));
}

#[test]
fn test_build_clone_args_with_tag() {
    let args = build_clone_args("https://h/r.git", "/d", None, Some("v1.0.0"));
    assert!(args.windows(2).any(|w| w == ["--branch", "v1.0.0"]));
}

#[test]
fn test_build_checkout_args_qualifies_branch_and_tag_refs() {
    assert_eq!(
        build_checkout_args("main", true),
        vec!["checkout".to_string(), "refs/heads/main".to_string()]
    );
    assert_eq!(
        build_checkout_args("v1.0.0", false),
        vec!["checkout".to_string(), "refs/tags/v1.0.0".to_string()]
    );
}

#[test]
fn test_build_checkout_args_ref_cannot_be_read_as_flag() {
    // SEC-12: the fixed `refs/heads/`/`refs/tags/` prefix means the final
    // argument can never begin with `-`, however `ref_name` is chosen.
    let args = build_checkout_args("--evil-flag", true);
    assert_eq!(args[1], "refs/heads/--evil-flag");
    assert!(!args[1].starts_with('-'));
}

#[test]
fn test_redact_url_credentials() {
    assert_eq!(
        redact_url_credentials("https://user:token@github.com/o/r.git"),
        "https://***@github.com/o/r.git"
    );
    assert_eq!(
        redact_url_credentials("https://x-access-token:ghp_secret@host/path"),
        "https://***@host/path"
    );
    // No credentials -> unchanged.
    assert_eq!(
        redact_url_credentials("https://github.com/o/r.git"),
        "https://github.com/o/r.git"
    );
    // Non-URL -> unchanged.
    assert_eq!(redact_url_credentials("not-a-url"), "not-a-url");
}

// --- Relocated from tests/unit/storage/git_test.rs (dead orphaned integration
// test directory that could never reach these `pub(crate)` items). Moved here
// as in-crate unit tests so they actually compile and run. ---

#[test]
fn test_parse_git_version_valid() {
    // Standard format
    let version = parse_git_version("git version 2.34.1").unwrap();
    assert_eq!(version.major, 2);
    assert_eq!(version.minor, 34);
    assert_eq!(version.patch, 1);
    assert!(version.is_supported());

    // With extra info
    let version = parse_git_version("git version 2.40.0 (Apple Git-140)").unwrap();
    assert_eq!(version.major, 2);
    assert_eq!(version.minor, 40);
    assert_eq!(version.patch, 0);

    // Version 2.0 (minimum supported)
    let version = parse_git_version("git version 2.0.0").unwrap();
    assert!(version.is_supported());

    // Version 1.9 (too old)
    let version = parse_git_version("git version 1.9.5").unwrap();
    assert!(!version.is_supported());
}

#[test]
fn test_parse_git_version_invalid() {
    // Invalid formats
    assert!(parse_git_version("not a version").is_err());
    assert!(parse_git_version("git 2.0.0").is_err());
    assert!(parse_git_version("version 2.0.0").is_err());
    assert!(parse_git_version("git version").is_err());
}

#[test]
fn test_is_network_error() {
    // Network-related errors
    assert!(is_network_error(
        "fatal: unable to access 'https://github.com/...': Failed to connect"
    ));
    // Transport failures worth retrying: a 5xx from the server, or a
    // curl-level error. Neither carries any of the substrings above.
    assert!(is_network_error(
        "error: RPC failed; HTTP 504 curl 22 The requested URL returned error: 504"
    ));
    assert!(is_network_error("error: RPC failed; HTTP 502"));
    assert!(is_network_error(
        "error: RPC failed; curl 56 GnuTLS recv error (-54)"
    ));

    // Connections dropped mid-transfer.
    assert!(is_network_error(
        "fatal: the remote end hung up unexpectedly"
    ));
    assert!(is_network_error("error: RPC failed; early EOF"));
    assert!(is_network_error(
        "fatal: could not resolve host: github.com"
    ));

    assert!(is_network_error(
        "fatal: unable to access 'https://...': Connection refused"
    ));
    assert!(is_network_error(
        "fatal: unable to access 'https://...': Name resolution failed"
    ));
    assert!(is_network_error("Network timeout occurred"));

    // Non-network errors
    assert!(!is_network_error("fatal: repository 'invalid' not found"));
    assert!(!is_network_error(
        "error: pathspec 'nonexistent' did not match any file"
    ));
    assert!(!is_network_error("fatal: Authentication failed"));

    // 4xx responses are durable answers about auth or existence: retrying
    // returns the identical result, so they must NOT be treated as network
    // errors even though they arrive as "RPC failed".
    assert!(!is_network_error(
        "error: RPC failed; HTTP 403 curl 22 The requested URL returned error: 403"
    ));
    assert!(!is_network_error("error: RPC failed; HTTP 404"));
    assert!(!is_network_error("error: RPC failed; HTTP 401"));
}

#[test]
fn test_git_error_display() {
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

#[test]
fn test_git_error_conversion() {
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

/// Environment-dependent: shells out to the real `git` binary. Gracefully
/// tolerates git being missing or too old (does not fail the test), but
/// still exercises `check_git_version()` against whatever git CI provides.
#[tokio::test]
async fn test_check_git_version_actual() {
    // This should succeed if git is installed
    match check_git_version().await {
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

/// Environment-dependent: requires `git` on PATH; skips gracefully if absent.
#[tokio::test]
async fn test_git_operations_async() {
    // Test that git operations execute asynchronously without blocking
    use std::time::Instant;

    // Execute a simple git command (version check)
    let start = Instant::now();
    let result = execute_git_command(&["--version"], Duration::from_secs(5), None).await;
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

/// Slow/network-dependent: attempts a real (doomed) clone against a
/// nonexistent domain to prove the timeout wrapper fires. Bounded by a
/// 100ms timeout so it should resolve in well under 2s, but it does
/// perform a real DNS lookup / outbound connection attempt.
#[tokio::test]
async fn test_git_command_timeout() {
    // Test that git commands respect timeout
    use std::time::Instant;

    // Try to execute a command that would take too long
    // Using a very short timeout
    let start = Instant::now();
    let result = execute_git_command(
        &[
            "clone",
            "--depth=1",
            "https://nonexistent-domain-12345.com/repo.git",
            "/tmp/test",
        ],
        Duration::from_millis(100), // Very short timeout
        None,
    )
    .await;
    let elapsed = start.elapsed();

    // Should timeout quickly
    assert!(
        elapsed.as_secs() < 2,
        "Command should timeout or fail quickly"
    );

    // Result should be an error (either timeout or connection failure)
    assert!(result.is_err() || result.as_ref().unwrap().exit_code != 0);
}

/// Slow/network-dependent: exercises the retry-with-backoff path against a
/// real invalid domain. `is_network_error` matches on "unable to access",
/// so a DNS failure here is classified as retryable and the test can take
/// several seconds (up to ~3s of backoff sleep across 3 attempts) even
/// though each individual git invocation fails fast.
#[tokio::test]
async fn test_retry_logic_structure() {
    // Test that retry logic is structured correctly
    // This is a unit test of the retry mechanism structure

    // Try a command that will fail (invalid repo)
    let result = execute_git_command_with_retry(
        &[
            "clone",
            "--depth=1",
            "https://invalid-repo-12345.com/nonexistent.git",
            "/tmp/test",
        ],
        Duration::from_secs(5),
        None,
        3, // Max 3 attempts
    )
    .await;

    // Should fail after retries (not a network error, so won't retry)
    // Or if it is a network error, should fail after 3 attempts
    assert!(result.is_err() || result.as_ref().unwrap().exit_code != 0);
}

// ── ls-remote helpers ─────────────────────────────────────────────────

#[test]
fn test_ls_remote_query_refs_branch_is_fully_qualified() {
    assert_eq!(
        ls_remote_query_refs(Some("main"), None),
        vec!["refs/heads/main".to_string()]
    );
}

#[test]
fn test_ls_remote_query_refs_tag_includes_peeled_form() {
    assert_eq!(
        ls_remote_query_refs(None, Some("v1.0.0")),
        vec![
            "refs/tags/v1.0.0".to_string(),
            "refs/tags/v1.0.0^{}".to_string(),
        ]
    );
}

#[test]
fn test_ls_remote_query_refs_default_is_head() {
    assert_eq!(ls_remote_query_refs(None, None), vec!["HEAD".to_string()]);
}

#[test]
fn test_build_ls_remote_args_protocol_flags_and_end_of_options() {
    let refs = vec!["HEAD".to_string()];
    let args = build_ls_remote_args("https://example.com/repo.git", &refs);

    assert!(args
        .windows(2)
        .any(|w| w == ["-c", "protocol.ext.allow=never"]));
    assert!(args
        .windows(2)
        .any(|w| w == ["-c", "protocol.file.allow=never"]));

    let dd = args.iter().position(|a| *a == "--").unwrap();
    assert_eq!(args[dd + 1], "https://example.com/repo.git");
    assert_eq!(args[dd + 2], "HEAD");
}

#[test]
fn test_build_ls_remote_args_url_cannot_be_read_as_flag() {
    let refs = vec!["HEAD".to_string()];
    let args = build_ls_remote_args("--upload-pack=evil", &refs);
    let dd = args.iter().position(|a| *a == "--").unwrap();
    assert_eq!(args[dd + 1], "--upload-pack=evil");
}

#[test]
fn test_parse_ls_remote_output_multiple_lines() {
    let stdout = "abc123\trefs/heads/main\ndef456\trefs/tags/v1.0.0\n";
    let refs = parse_ls_remote_output(stdout);
    assert_eq!(
        refs.get("refs/heads/main").map(String::as_str),
        Some("abc123")
    );
    assert_eq!(
        refs.get("refs/tags/v1.0.0").map(String::as_str),
        Some("def456")
    );
}

#[test]
fn test_parse_ls_remote_output_empty_is_empty_map() {
    assert!(parse_ls_remote_output("").is_empty());
}

#[test]
fn test_resolve_sha_from_refs_prefers_peeled_tag() {
    let mut refs = HashMap::new();
    refs.insert("refs/tags/v1.0.0".to_string(), "tagobj".to_string());
    refs.insert("refs/tags/v1.0.0^{}".to_string(), "commitsha".to_string());
    let query_refs = ls_remote_query_refs(None, Some("v1.0.0"));

    let sha = resolve_sha_from_refs(&refs, &query_refs, "url").unwrap();
    assert_eq!(sha, "commitsha");
}

#[test]
fn test_resolve_sha_from_refs_lightweight_tag_falls_back_to_direct() {
    let mut refs = HashMap::new();
    refs.insert("refs/tags/v1.0.0".to_string(), "commitsha".to_string());
    let query_refs = ls_remote_query_refs(None, Some("v1.0.0"));

    let sha = resolve_sha_from_refs(&refs, &query_refs, "url").unwrap();
    assert_eq!(sha, "commitsha");
}

#[test]
fn test_resolve_sha_from_refs_missing_ref_errors() {
    let refs = HashMap::new();
    let query_refs = ls_remote_query_refs(Some("nope"), None);

    let err = resolve_sha_from_refs(&refs, &query_refs, "url").unwrap_err();
    assert!(matches!(err, GitError::RefNotFound { .. }));
}

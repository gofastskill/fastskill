//! Immutable Git commit acquisition used by strict lock restoration.

use super::git::{
    check_git_version, execute_git_command_with_retry, redact_url_credentials, GitError,
    CLONE_INVOCATIONS,
};
use crate::core::service::ServiceError;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::TempDir;

fn valid_commit_id(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn build_checkout_args(commit: &str) -> Vec<&str> {
    vec![
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        "checkout",
        "--quiet",
        "--detach",
        commit,
    ]
}

/// Clone and check out one immutable commit. A full object negotiation is used
/// because a shallow clone of the current branch cannot restore an older lock.
pub async fn clone_repository_at_commit(url: &str, commit: &str) -> Result<TempDir, ServiceError> {
    if !valid_commit_id(commit) {
        return Err(ServiceError::Validation(
            "git commit must be a 40- or 64-character hexadecimal object ID".to_string(),
        ));
    }
    check_git_version().await?;
    CLONE_INVOCATIONS.fetch_add(1, Ordering::SeqCst);
    let temp_dir = TempDir::new()?;
    let destination = temp_dir.path().to_string_lossy().into_owned();
    let args = [
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.file.allow=never",
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        "clone",
        "--quiet",
        "--no-checkout",
        "--no-tags",
        "--",
        url,
        destination.as_str(),
    ];
    execute_git_command_with_retry(&args, Duration::from_secs(300), None, 3)
        .await
        .map_err(|error| GitError::CloneFailed {
            url: redact_url_credentials(url),
            stderr: error.to_string(),
        })?;
    let checkout = build_checkout_args(commit);
    execute_git_command_with_retry(&checkout, Duration::from_secs(60), Some(temp_dir.path()), 1)
        .await
        .map_err(|error| GitError::CheckoutFailed {
            ref_name: commit.to_string(),
            stderr: error.to_string(),
        })?;
    Ok(temp_dir)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_full_hex_object_ids() {
        assert!(valid_commit_id(&"a".repeat(40)));
        assert!(valid_commit_id(&"B".repeat(64)));
        assert!(!valid_commit_id("abc"));
        assert!(!valid_commit_id(&format!("{}g", "a".repeat(39))));
    }

    #[test]
    fn immutable_checkout_disables_line_ending_translation() {
        let commit = "a".repeat(40);
        let args = build_checkout_args(&commit);
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-c", "core.autocrlf=false"]));
        assert!(args.windows(2).any(|pair| pair == ["-c", "core.eol=lf"]));
        let autocrlf = args
            .iter()
            .position(|arg| *arg == "core.autocrlf=false")
            .expect("line-ending config");
        let checkout = args
            .iter()
            .position(|arg| *arg == "checkout")
            .expect("checkout command");
        assert!(autocrlf < checkout);
    }

    #[tokio::test]
    async fn rejects_invalid_commit_before_cloning() {
        let error = clone_repository_at_commit("https://example.invalid/repo.git", "main")
            .await
            .expect_err("symbolic refs are not immutable commit IDs");
        assert!(error.to_string().contains("40- or 64-character"));
    }

    #[tokio::test]
    async fn wraps_clone_failure_with_redacted_url() {
        let error = clone_repository_at_commit(
            "file://user:secret@/definitely/missing/repo.git",
            &"a".repeat(40),
        )
        .await
        .expect_err("the local transport and missing repository must be rejected");
        let message = error.to_string();
        assert!(message.contains("Failed to clone"));
        assert!(!message.contains("secret"));
    }
}

//! A git clone isolated from the machine it runs on, for callers that clone
//! repositories they do not control -- a service importing skills on behalf
//! of its users, say -- where `clone_repository`'s assumptions (a developer's
//! own git setup, ambient credentials, retries) are the wrong ones.
//!
//! On top of the SEC-11 hardening every clone gets, an isolated clone:
//!
//! - reads no system, global or user git configuration, and inherits no
//!   environment beyond `PATH`, proxy and CA settings, so a credential helper,
//!   `url.<base>.insteadOf`, an include, a filter driver or a `GIT_CONFIG_*`
//!   variable on the host never applies;
//! - runs no hooks and copies no templates;
//! - never prompts, and speaks only the protocols the caller allows (HTTPS by
//!   default), with redirects off by default so a server cannot send the clone
//!   to a host the caller did not choose;
//! - is bounded by one overall deadline, a byte cap and a file cap, checked
//!   while git runs, and killed when any is crossed.
//!
//! The result is the checked-out tree at one commit, with its `.git` removed.
//! Symbolic links stay as they are in the repository: deciding whether to
//! accept them is the caller's job, and
//! [`content_digest`](crate::core::content_digest::content_digest) refuses
//! them.

use super::git::{check_git_version, redact_url_credentials, scrub_inherited_git_env, GitError};
use crate::core::service::ServiceError;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{interval, sleep_until, Instant, MissedTickBehavior};

/// Environment variables an isolated clone keeps: what git needs to find
/// itself and reach the network, and nothing that configures git.
const PASSED_ENV_VARS: &[&str] = &[
    "PATH",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "GIT_SSL_CAINFO",
    "GIT_SSL_CAPATH",
];

/// How often the clone's size is checked while git runs.
const SIZE_POLL: Duration = Duration::from_millis(200);

/// How much of git's stderr is kept for the error message.
const STDERR_LIMIT: usize = 8 * 1024;

/// Limits and policy for [`clone_isolated`].
#[derive(Debug, Clone)]
pub struct IsolatedCloneOptions {
    /// The deadline for the whole operation.
    pub timeout: Duration,
    /// The most bytes the clone may take on disk, git metadata included.
    pub max_bytes: u64,
    /// The most files and directories the clone may hold, git metadata included.
    pub max_files: u64,
    /// The transports git may use, as `GIT_ALLOW_PROTOCOL` names them.
    pub allowed_protocols: Vec<String>,
    /// Whether git may follow an HTTP redirect on the initial request.
    pub follow_redirects: bool,
    /// Where to create the clone; the system temporary directory if `None`.
    pub parent_dir: Option<PathBuf>,
}

impl Default for IsolatedCloneOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_bytes: 256 * 1024 * 1024,
            max_files: 10_000,
            allowed_protocols: vec!["https".to_string()],
            follow_redirects: false,
            parent_dir: None,
        }
    }
}

/// A checked-out tree at one commit, removed when dropped.
#[derive(Debug)]
pub struct IsolatedClone {
    workspace: TempDir,
    commit: String,
}

impl IsolatedClone {
    /// The checked-out tree, without `.git`.
    pub fn path(&self) -> PathBuf {
        self.workspace.path().join(TREE_DIR)
    }

    /// The full object ID of the commit the tree was checked out at.
    pub fn commit(&self) -> &str {
        &self.commit
    }
}

const TREE_DIR: &str = "tree";
const HOME_DIR: &str = "home";

/// Clone `branch` (the remote's default branch if `None`) of `url` at depth
/// one, isolated as the module docs describe.
///
/// # Errors
///
/// Fails if the options are invalid, git is missing or too old, the clone
/// fails, or the deadline, byte cap or file cap is crossed; nothing is left on
/// disk.
pub async fn clone_isolated(
    url: &str,
    branch: Option<&str>,
    options: &IsolatedCloneOptions,
) -> Result<IsolatedClone, ServiceError> {
    let allow = allowed_protocols(&options.allowed_protocols)?;
    let deadline = Instant::now() + options.timeout;
    check_git_version().await?;

    let workspace = match &options.parent_dir {
        Some(parent) => TempDir::new_in(parent)?,
        None => TempDir::new()?,
    };
    let home = workspace.path().join(HOME_DIR);
    let tree = workspace.path().join(TREE_DIR);
    for dir in ["hooks", "template"] {
        std::fs::create_dir_all(home.join(dir))?;
    }
    std::fs::write(home.join("gitconfig"), "")?;

    let safe_url = redact_url_credentials(url);
    let clone_args = build_isolated_clone_args(url, &tree, &home, branch, options);
    let mut clone = isolated_git(&home, &allow);
    clone.args(&clone_args);
    let failed = |stderr: String| GitError::CloneFailed {
        url: safe_url.clone(),
        stderr,
    };
    run_bounded(clone, &tree, deadline, options, &safe_url)
        .await?
        .map_err(failed)?;

    let mut rev_parse = isolated_git(&home, &allow);
    rev_parse.current_dir(&tree);
    rev_parse.args(["rev-parse", "--verify", "HEAD^{commit}"]);
    let output = run_bounded(rev_parse, &tree, deadline, options, &safe_url)
        .await?
        .map_err(failed)?;
    let commit = output.trim().to_string();
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(failed(format!("unexpected commit id {commit:?}")).into());
    }

    std::fs::remove_dir_all(tree.join(".git"))?;
    Ok(IsolatedClone { workspace, commit })
}

/// Validate the allowed transports and join them as `GIT_ALLOW_PROTOCOL`
/// expects. `ext` and `file` are never allowed, whatever the caller asks.
pub(crate) fn allowed_protocols(protocols: &[String]) -> Result<String, ServiceError> {
    let valid = |name: &String| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b))
            && name != "ext"
            && name != "file"
    };
    if protocols.is_empty() || !protocols.iter().all(valid) {
        return Err(ServiceError::Validation(format!(
            "allowed git protocols must be non-empty transport names other than ext and file: \
             {protocols:?}"
        )));
    }
    Ok(protocols.join(":"))
}

/// The argument vector of an isolated `git clone`.
pub(crate) fn build_isolated_clone_args(
    url: &str,
    tree: &Path,
    home: &Path,
    branch: Option<&str>,
    options: &IsolatedCloneOptions,
) -> Vec<String> {
    let redirects = if options.follow_redirects {
        "initial"
    } else {
        "false"
    };
    let mut args: Vec<String> = [
        "protocol.ext.allow=never".to_string(),
        "protocol.file.allow=never".to_string(),
        format!("core.hooksPath={}", home.join("hooks").display()),
        "core.fsmonitor=false".to_string(),
        "core.autocrlf=false".to_string(),
        "core.eol=lf".to_string(),
        "credential.helper=".to_string(),
        format!("http.followRedirects={redirects}"),
    ]
    .into_iter()
    .flat_map(|setting| ["-c".to_string(), setting])
    .collect();
    args.extend(
        [
            "clone",
            "--depth=1",
            "--quiet",
            "--single-branch",
            "--no-tags",
            "--no-recurse-submodules",
        ]
        .map(String::from),
    );
    args.push(format!("--template={}", home.join("template").display()));
    if let Some(branch) = branch {
        args.push(format!("--branch={branch}"));
    }
    args.push("--".to_string());
    args.push(url.to_string());
    args.push(tree.display().to_string());
    args
}

/// A `git` command with an empty environment but for [`PASSED_ENV_VARS`],
/// and every source of configuration pointed at `home`, which holds nothing.
pub(crate) fn isolated_git(home: &Path, allowed_protocols: &str) -> Command {
    let mut cmd = Command::new("git");
    cmd.env_clear();
    for var in PASSED_ENV_VARS {
        if let Some(value) = std::env::var_os(var) {
            cmd.env(var, value);
        }
    }
    // Not needed after `env_clear`, but keeps the guarantee if the list grows.
    scrub_inherited_git_env(&mut cmd);
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", allowed_protocols)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

/// Run `cmd` until it exits, the deadline passes or `watched` crosses a cap.
/// The outer error is a crossed limit; the inner one is git's failure, with
/// the start of its stderr.
async fn run_bounded(
    mut cmd: Command,
    watched: &Path,
    deadline: Instant,
    options: &IsolatedCloneOptions,
    safe_url: &str,
) -> Result<Result<String, String>, ServiceError> {
    let mut child = cmd.spawn().map_err(|e| -> ServiceError {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::GitNotInstalled.into()
        } else {
            ServiceError::Custom(format!("Failed to execute git command: {e}"))
        }
    })?;
    let stdout = tokio::spawn(read_capped(child.stdout.take()));
    let stderr = tokio::spawn(read_capped(child.stderr.take()));
    let mut poll = interval(SIZE_POLL);
    poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let status = loop {
        tokio::select! {
            status = child.wait() => break status?,
            _ = sleep_until(deadline) => {
                let _ = child.kill().await;
                return Err(GitError::Timeout {
                    operation: format!("isolated clone of {safe_url}"),
                    timeout_secs: options.timeout.as_secs(),
                }
                .into());
            }
            _ = poll.tick() => {
                if let Some(limit) = crossed_limit(watched, options).await? {
                    let _ = child.kill().await;
                    return Err(GitError::CloneTooLarge {
                        url: safe_url.to_string(),
                        limit,
                    }
                    .into());
                }
            }
        }
    };
    if let Some(limit) = crossed_limit(watched, options).await? {
        return Err(GitError::CloneTooLarge {
            url: safe_url.to_string(),
            limit,
        }
        .into());
    }
    let stdout = stdout.await.unwrap_or_default();
    let stderr = stderr.await.unwrap_or_default();
    Ok(if status.success() {
        Ok(stdout)
    } else {
        Err(stderr.trim().to_string())
    })
}

/// Drain `pipe` to its end, so git never blocks on it, keeping only the
/// first [`STDERR_LIMIT`] bytes.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut kept = Vec::new();
    let mut chunk = [0u8; 4096];
    while let Ok(read) = pipe.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        let room = STDERR_LIMIT.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..read.min(room)]);
    }
    String::from_utf8_lossy(&kept).into_owned()
}

/// Which cap `dir` has crossed, if any.
async fn crossed_limit(
    dir: &Path,
    options: &IsolatedCloneOptions,
) -> Result<Option<String>, ServiceError> {
    let dir = dir.to_path_buf();
    let (bytes, files) = tokio::task::spawn_blocking(move || disk_usage(&dir))
        .await
        .map_err(|e| ServiceError::Custom(format!("Failed to measure the clone: {e}")))?;
    Ok(if bytes > options.max_bytes {
        Some(format!("more than {} bytes", options.max_bytes))
    } else if files > options.max_files {
        Some(format!("more than {} files", options.max_files))
    } else {
        None
    })
}

/// The bytes and entries under `dir`, without following links. Entries that
/// vanish while git works are skipped.
pub(crate) fn disk_usage(dir: &Path) -> (u64, u64) {
    walkdir::WalkDir::new(dir)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.metadata().ok())
        .fold((0, 0), |(bytes, files), metadata| {
            (bytes.saturating_add(metadata.len()), files + 1)
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

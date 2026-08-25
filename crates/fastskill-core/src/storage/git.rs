//! Git operations for cloning skill repositories using system git binary

use crate::core::service::ServiceError;
use crate::core::sources::SourceAuth;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Git operation error types
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git binary not found. Please install git: https://git-scm.com/downloads")]
    GitNotInstalled,

    #[error("Git version {version} is too old. FastSkill requires git {required} or higher. Please upgrade: https://git-scm.com/downloads")]
    GitVersionTooOld { version: String, required: String },

    #[error("Failed to clone repository {url}: {stderr}")]
    CloneFailed { url: String, stderr: String },

    #[error("Failed to checkout {ref_name}: {stderr}")]
    CheckoutFailed { ref_name: String, stderr: String },

    #[error("Git operation '{operation}' timed out after {timeout_secs} seconds")]
    Timeout {
        operation: String,
        timeout_secs: u64,
    },

    #[error("Network error for {url} (attempt {attempt}/{max_attempts})")]
    NetworkError {
        url: String,
        attempt: u32,
        max_attempts: u32,
    },

    #[error("Invalid git URL '{url}': {reason}")]
    InvalidUrl { url: String, reason: String },

    #[error("Authentication failed for {url}: {stderr}")]
    AuthenticationFailed { url: String, stderr: String },

    #[error("Failed to resolve ref for {url}: {stderr}")]
    LsRemoteFailed { url: String, stderr: String },

    #[error("Ref '{ref_name}' not found on {url}")]
    RefNotFound { url: String, ref_name: String },

    #[error(
        "Git sources authenticate via the system git credential helper or SSH agent, not via \
         `auth` config -- fastskill does not inject PAT/basic credentials into git operations. \
         Configure a git credential helper (e.g. `git config credential.helper store`, or `gh \
         auth login`) or use an SSH remote (e.g. `git@github.com:org/repo.git`) with a key \
         loaded in your SSH agent instead."
    )]
    AuthNotSupported,
}

impl From<GitError> for ServiceError {
    fn from(err: GitError) -> Self {
        ServiceError::Custom(err.to_string())
    }
}

/// Git version information (cached after first check)
#[derive(Debug, Clone)]
pub(crate) struct GitVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl GitVersion {
    fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    fn is_supported(&self) -> bool {
        self.major >= 2
    }
}

impl std::fmt::Display for GitVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Cached git version (checked once per process)
static GIT_VERSION: OnceLock<Result<GitVersion, ServiceError>> = OnceLock::new();

/// Check git version and cache result
pub(crate) async fn check_git_version() -> Result<(), ServiceError> {
    // Check if already cached
    if let Some(result) = GIT_VERSION.get() {
        return result.as_ref().map(|_| ()).map_err(|_| {
            GitError::GitVersionTooOld {
                version: "unknown".to_string(),
                required: "2.0".to_string(),
            }
            .into()
        });
    }

    // Execute git --version
    let output = Command::new("git")
        .arg("--version")
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::GitNotInstalled.into()
            } else {
                ServiceError::Custom(format!("Failed to execute git --version: {}", e))
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ServiceError::Custom(format!(
            "git --version failed: {}",
            stderr
        )));
    }

    // Parse version string
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = parse_git_version(&stdout)?;

    // Validate version >= 2.0
    if !version.is_supported() {
        return Err(GitError::GitVersionTooOld {
            version: format!("{}", version),
            required: "2.0".to_string(),
        }
        .into());
    }

    // Cache the result
    GIT_VERSION.set(Ok(version)).ok();

    Ok(())
}

/// Parse git version from output string (e.g., "git version 2.34.1")
pub(crate) fn parse_git_version(version_str: &str) -> Result<GitVersion, ServiceError> {
    // Expected format: "git version X.Y.Z" or "git version X.Y.Z (extra info)"
    let parts: Vec<&str> = version_str.split_whitespace().collect();
    if parts.len() < 3 || parts[0] != "git" || parts[1] != "version" {
        return Err(ServiceError::Custom(format!(
            "Unexpected git version format: {}",
            version_str
        )));
    }

    let version_part = parts[2];
    // Remove any trailing parentheses or extra info
    let version_part = version_part
        .split('(')
        .next()
        .unwrap_or(version_part)
        .trim();
    let version_numbers: Vec<&str> = version_part.split('.').collect();

    if version_numbers.len() < 2 {
        return Err(ServiceError::Custom(format!(
            "Invalid git version format: {}",
            version_part
        )));
    }

    let major = version_numbers[0]
        .parse::<u32>()
        .map_err(|e| ServiceError::Custom(format!("Failed to parse git major version: {}", e)))?;
    let minor = version_numbers[1]
        .parse::<u32>()
        .map_err(|e| ServiceError::Custom(format!("Failed to parse git minor version: {}", e)))?;
    let patch = version_numbers
        .get(2)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    Ok(GitVersion::new(major, minor, patch))
}

/// Redact embedded credentials (`user:token@host`) from a URL before logging.
///
/// Converts e.g. `https://user:token@host/path` into `https://***@host/path`.
/// Leaves URLs without userinfo untouched.
pub(crate) fn redact_url_credentials(url: &str) -> String {
    if let Some(scheme_pos) = url.find("://") {
        let after = scheme_pos + 3;
        let rest = &url[after..];
        let authority_end = rest.find('/').map(|i| after + i).unwrap_or(url.len());
        let authority = &url[after..authority_end];
        if let Some(at) = authority.find('@') {
            let host = &authority[at + 1..];
            return format!("{}***@{}{}", &url[..after], host, &url[authority_end..]);
        }
    }
    url.to_string()
}

/// Build the argument vector for `git clone`.
///
/// Hardening (SEC-11):
/// - `-c protocol.ext.allow=never -c protocol.file.allow=never` disables the
///   `ext::` transport (arbitrary command execution) and `file://` local reads.
/// - `--` terminates options before the positional `url`/`dest`, so neither can
///   be interpreted as a flag even if it begins with `-`.
pub(crate) fn build_clone_args<'a>(
    url: &'a str,
    dest: &'a str,
    branch: Option<&'a str>,
    tag: Option<&'a str>,
) -> Vec<&'a str> {
    let mut args = vec![
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.file.allow=never",
        // Check out exactly what the repository contains. Git for Windows
        // defaults `core.autocrlf=true`, which rewrites LF to CRLF on
        // checkout -- so the same skill installed on Windows would differ
        // byte-for-byte from the published source, and from the same skill
        // installed on that machine from a zip. A package manager should
        // deliver the artifact as published, so opt out of the conversion.
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        "clone",
        "--depth=1",
        "--quiet",
    ];

    if let Some(branch) = branch {
        args.extend(["--branch", branch]);
    } else if let Some(tag) = tag {
        args.extend(["--branch", tag]);
    }

    args.push("--single-branch");
    args.push("--no-tags");
    // End-of-options separator before positional arguments.
    args.push("--");
    args.push(url);
    args.push(dest);
    args
}

/// Build the argument vector for `git checkout` (SEC-12).
///
/// Fully-qualifies `ref_name` as `refs/heads/<name>` or `refs/tags/<name>`
/// (per `is_branch`) rather than using a bare `--` end-of-options separator.
/// This is deliberate, not just a style choice: `git checkout -- <name>`
/// (the `build_clone_args`-style guard) means "restore the *pathspec*
/// `<name>` from the index", not "switch to the ref `<name>`" — it fails with
/// `pathspec '<name>' did not match any file(s)` for every real branch/tag,
/// since checkout only treats an argument as a revision when it is *not*
/// preceded by `--`. Prefixing with a fixed, non-attacker-controlled
/// `refs/heads/`/`refs/tags/` gives the same SEC-12 protection (the full
/// argument can never begin with `-`, so it can never be read as a flag)
/// while keeping correct ref (not path) semantics.
pub(crate) fn build_checkout_args(ref_name: &str, is_branch: bool) -> Vec<String> {
    let qualified = if is_branch {
        format!("refs/heads/{ref_name}")
    } else {
        format!("refs/tags/{ref_name}")
    };
    vec!["checkout".to_string(), qualified]
}

/// Command output structure
#[allow(dead_code)] // stdout may be used for future progress parsing
pub(crate) struct CommandOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

/// Environment variables git exports to its own subprocesses (hooks, `rebase
/// --exec`, aliases, filters) to pin them to *the invoking repository*.
///
/// If `fastskill` is itself run from such a context, these are inherited, and
/// every `git` we spawn silently retargets the caller's repo instead of the
/// path we asked for — `clone` writes objects into the wrong `GIT_DIR`, and a
/// `commit` in a scratch directory can even fire the caller's hooks. Clear
/// them so our invocations always mean what the arguments say.
const INHERITED_GIT_ENV_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
];

/// Remove the repository-pinning variables listed in
/// [`INHERITED_GIT_ENV_VARS`] from `cmd`'s environment.
///
/// Note `GIT_DIR` *overrides* `Command::current_dir`, so setting a working
/// directory is not on its own enough to target a repository — any git spawn
/// that relies on cwd must go through here too.
pub(crate) fn scrub_inherited_git_env(cmd: &mut Command) {
    for var in INHERITED_GIT_ENV_VARS {
        cmd.env_remove(var);
    }
}

/// Execute git command with timeout
pub(crate) async fn execute_git_command(
    args: &[&str],
    timeout_duration: Duration,
    cwd: Option<&Path>,
) -> Result<CommandOutput, ServiceError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    scrub_inherited_git_env(&mut cmd);

    // Execute with timeout
    let args_str = args.join(" ");
    let output = timeout(timeout_duration, cmd.output())
        .await
        .map_err(|_| -> ServiceError {
            GitError::Timeout {
                operation: args_str.clone(),
                timeout_secs: timeout_duration.as_secs(),
            }
            .into()
        })?
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::GitNotInstalled.into()
            } else {
                ServiceError::Custom(format!("Failed to execute git command: {}", e))
            }
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    Ok(CommandOutput {
        stdout,
        stderr,
        exit_code,
    })
}

/// Check if error is a network error (retryable)
pub(crate) fn is_network_error(stderr: &str) -> bool {
    let lower_stderr = stderr.to_lowercase();
    if lower_stderr.contains("network")
        || lower_stderr.contains("connection")
        || lower_stderr.contains("timeout")
        || lower_stderr.contains("unable to access")
        || lower_stderr.contains("failed to connect")
        || lower_stderr.contains("connection refused")
        || lower_stderr.contains("name resolution")
        || lower_stderr.contains("could not resolve host")
    {
        return true;
    }

    // git reports transport failures as "error: RPC failed; ...". Retry only the
    // ones that can plausibly succeed next time: a 5xx from the server or a
    // curl-level transport error. A 4xx (401/403/404) is a durable answer about
    // auth or existence and would fail identically on every retry.
    if lower_stderr.contains("rpc failed")
        && !lower_stderr.contains("http 4")
        && (lower_stderr.contains("http 5") || lower_stderr.contains("curl "))
    {
        // The 4xx exclusion comes first because git appends a curl code even to
        // 4xx responses ("HTTP 403 curl 22 ..."), which would otherwise read as
        // a retryable transport error.
        return true;
    }

    // Connections that dropped mid-transfer. These carry none of the substrings
    // above but are exactly the transient case retrying exists for.
    lower_stderr.contains("early eof")
        || lower_stderr.contains("remote end hung up")
        || lower_stderr.contains("recv failure")
        || lower_stderr.contains("send failure")
}

/// Execute git command with retry logic for network errors
pub(crate) async fn execute_git_command_with_retry(
    args: &[&str],
    timeout_duration: Duration,
    cwd: Option<&Path>,
    max_attempts: u32,
) -> Result<CommandOutput, ServiceError> {
    let mut attempt = 1;
    let mut delay = Duration::from_secs(1); // Start with 1 second

    loop {
        match execute_git_command(args, timeout_duration, cwd).await {
            Ok(output) => {
                if output.exit_code == 0 {
                    return Ok(output);
                }

                // Check if it's a network error and we should retry
                if attempt < max_attempts && is_network_error(&output.stderr) {
                    warn!(
                        "Git operation failed with network error (attempt {}/{}): {}",
                        attempt, max_attempts, output.stderr
                    );
                    info!("Retrying in {:?}...", delay);
                    tokio::time::sleep(delay).await;
                    delay *= 2; // Exponential backoff: 1s, 2s, 4s
                    attempt += 1;
                    continue;
                }

                // Not a network error or max attempts reached
                return Err(ServiceError::Custom(format!(
                    "Git command failed: {}",
                    output.stderr
                )));
            }
            Err(e) => {
                // Check if it's a timeout or network-related error
                let error_msg = e.to_string();
                if attempt < max_attempts
                    && (error_msg.contains("timeout") || error_msg.contains("network"))
                {
                    warn!(
                        "Git operation failed (attempt {}/{}): {}",
                        attempt, max_attempts, error_msg
                    );
                    info!("Retrying in {:?}...", delay);
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                    attempt += 1;
                    continue;
                }

                return Err(e);
            }
        }
    }
}

/// Number of times [`clone_repository`] has actually run a `git clone`.
///
/// Instrumentation-only (PRD 006 "Local Skill Cache", US-002): the content
/// cache should make most installs skip cloning entirely, and the only
/// reliable way to assert that in an integration test — without a full mock
/// git layer — is to count real invocations. Deliberately not gated behind
/// `#[cfg(test)]`: integration tests are a separate compilation unit and
/// cannot see items scoped that way. The cost of an uncontended atomic
/// increment per clone is negligible in production.
#[doc(hidden)] // test instrumentation, not supported public API
pub static CLONE_INVOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Resolve a branch or tag name on the remote at `url` to its commit SHA,
/// without cloning (PRD 006 "Local Skill Cache", US-002).
///
/// `branch` and `tag` are mutually exclusive, mirroring [`clone_repository`];
/// when both are `None`, resolves `HEAD` (the remote's default branch). For an
/// annotated tag, resolves to the commit the tag *points at* (the peeled
/// object) — never the tag object's own SHA — so this matches what a
/// `clone` + `checkout` of that tag lands on.
///
/// Follows the same conventions as [`clone_repository`]: shells out via
/// `Command::new("git")`, the protocol allowlist + `--` end-of-options guard
/// (SEC-11), credential redaction in logs, the git-version check, and the
/// shared retry wrapper.
///
/// # Errors
///
/// Returns `ServiceError` if git is not installed, the version is too old, the
/// remote could not be reached, or `branch`/`tag`/`HEAD` does not exist on it.
pub async fn ls_remote(
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
) -> Result<String, ServiceError> {
    check_git_version().await?;

    let safe_url = redact_url_credentials(url);
    let query_refs = ls_remote_query_refs(branch, tag);
    debug!("Resolving ref for {}: {:?}", safe_url, query_refs);

    let args = build_ls_remote_args(url, &query_refs);
    let ls_remote_timeout = Duration::from_secs(30);
    let output = execute_git_command_with_retry(&args, ls_remote_timeout, None, 3)
        .await
        .map_err(|e| GitError::LsRemoteFailed {
            url: safe_url.clone(),
            stderr: e.to_string(),
        })?;

    let refs = parse_ls_remote_output(&output.stdout);
    resolve_sha_from_refs(&refs, &query_refs, &safe_url).map_err(Into::into)
}

/// The ref patterns to query for a given branch/tag/default selection: exactly
/// one fully-qualified ref for a branch or `HEAD`; two for a tag (the tag ref
/// itself, and its peeled `^{}` form) so an annotated tag resolves to the
/// commit it points at rather than the tag object.
fn ls_remote_query_refs(branch: Option<&str>, tag: Option<&str>) -> Vec<String> {
    match (branch, tag) {
        (Some(b), _) => vec![format!("refs/heads/{b}")],
        (None, Some(t)) => vec![format!("refs/tags/{t}"), format!("refs/tags/{t}^{{}}")],
        (None, None) => vec!["HEAD".to_string()],
    }
}

/// Build the argument vector for `git ls-remote`.
///
/// Hardening (SEC-11): same protocol allowlist as [`build_clone_args`], plus
/// `--` end-of-options before `url`/`refs` so neither can be read as a flag.
pub(crate) fn build_ls_remote_args<'a>(url: &'a str, refs: &'a [String]) -> Vec<&'a str> {
    let mut args = vec![
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.file.allow=never",
        "ls-remote",
        "--",
        url,
    ];
    args.extend(refs.iter().map(String::as_str));
    args
}

/// Parse `git ls-remote` output (`<sha>\t<ref>` per line) into a `ref -> sha` map.
fn parse_ls_remote_output(stdout: &str) -> HashMap<String, String> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let sha = parts.next()?.trim();
            let ref_name = parts.next()?.trim();
            if sha.is_empty() || ref_name.is_empty() {
                return None;
            }
            Some((ref_name.to_string(), sha.to_string()))
        })
        .collect()
}

/// Pick the resolved SHA out of a parsed `ls-remote` ref map. Prefers the
/// peeled form (`query_refs[1]`, e.g. `refs/tags/v1^{}`) when present — the
/// commit an annotated tag points at — falling back to the direct ref.
fn resolve_sha_from_refs(
    refs: &HashMap<String, String>,
    query_refs: &[String],
    safe_url: &str,
) -> Result<String, GitError> {
    if let Some(peeled) = query_refs.get(1).and_then(|r| refs.get(r)) {
        return Ok(peeled.clone());
    }
    if let Some(direct) = query_refs.first().and_then(|r| refs.get(r)) {
        return Ok(direct.clone());
    }
    Err(GitError::RefNotFound {
        url: safe_url.to_string(),
        ref_name: query_refs.first().cloned().unwrap_or_default(),
    })
}

/// Clone a git repository to a temporary directory.
///
/// This function uses the system git binary to clone a repository. It performs shallow clones
/// (depth=1) for optimal performance and supports branch/tag checkout.
///
/// # Arguments
///
/// * `url` - Git repository URL (HTTPS, SSH, or GitHub tree URL)
/// * `branch` - Optional branch name to checkout after clone
/// * `tag` - Optional tag name to checkout after clone (mutually exclusive with `branch`)
/// * `auth` - Not supported: git sources authenticate via the system git credential
///   helper or SSH agent. Passing `Some` returns `GitError::AuthNotSupported` instead
///   of silently proceeding unauthenticated.
///
/// # Returns
///
/// Returns a `TempDir` containing the cloned repository. The directory is automatically
/// cleaned up when the `TempDir` is dropped.
///
/// # Errors
///
/// Returns `ServiceError` if:
/// - Git is not installed or not in PATH
/// - Git version is too old (< 2.0)
/// - Clone operation fails (network error, invalid URL, authentication failure)
/// - Checkout operation fails
/// - Operation times out (5 minute timeout for clone)
///
/// # Examples
///
/// ```no_run
/// use fastskill_core::storage::git::clone_repository;
///
/// # async fn example() -> Result<(), fastskill_core::core::service::ServiceError> {
/// let temp_dir = clone_repository(
///     "https://github.com/example/repo.git",
///     Some("main"),
///     None,
///     None,
/// ).await?;
/// // Use temp_dir.path() to access cloned repository
/// # Ok(())
/// # }
/// ```
pub async fn clone_repository(
    url: &str,
    branch: Option<&str>,
    tag: Option<&str>,
    auth: Option<&SourceAuth>,
) -> Result<TempDir, ServiceError> {
    CLONE_INVOCATIONS.fetch_add(1, Ordering::SeqCst);

    // Fail loudly rather than silently ignoring a configured `auth`: a user
    // who configured it believes their private repo is authenticated, when
    // it would otherwise only "work" by ambient git-credential accident.
    // fastskill has no PAT/basic credential-injection machinery for git
    // operations -- see `GitError::AuthNotSupported` for the actionable
    // alternative (credential helper / SSH remote).
    if auth.is_some() {
        return Err(GitError::AuthNotSupported.into());
    }

    // Check git version first
    check_git_version().await?;

    // Create temporary directory
    let temp_dir = TempDir::new().map_err(|e| {
        ServiceError::Custom(format!("Failed to create temporary directory: {}", e))
    })?;

    // Never log embedded credentials.
    let safe_url = redact_url_credentials(url);
    info!("Cloning repository: {}", safe_url);

    let dest = temp_dir.path().to_str().ok_or_else(|| {
        ServiceError::Custom("Failed to convert temp directory path to string".to_string())
    })?;

    // Build clone command arguments (protocol allowlist + `--` end-of-options, SEC-11).
    let clone_args = build_clone_args(url, dest, branch, tag);

    // Execute clone with retry (5 minute timeout, max 3 attempts).
    // `execute_git_command_with_retry` already returns Err on any non-zero exit,
    // so map that Err into the structured CloneFailed with URL context (BUG-12).
    let clone_timeout = Duration::from_secs(300); // 5 minutes
    match execute_git_command_with_retry(&clone_args, clone_timeout, None, 3).await {
        Ok(_) => {}
        Err(e) => {
            // Clean up on failure
            drop(temp_dir);
            return Err(GitError::CloneFailed {
                url: safe_url,
                stderr: e.to_string(),
            }
            .into());
        }
    }

    // Checkout branch or tag if specified (already handled by --branch flag, but verify)
    if let Some(ref_name) = branch.or(tag) {
        checkout_branch_or_tag(temp_dir.path(), ref_name, branch.is_some()).await?;
        debug!(
            "Checked out {}: {}",
            if branch.is_some() { "branch" } else { "tag" },
            ref_name
        );
    }

    Ok(temp_dir)
}

/// Checkout a specific branch or tag in a git repository.
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository
/// * `ref_name` - Branch or tag name to checkout
/// * `is_branch` - Whether `ref_name` is a branch (`refs/heads/`) or a tag (`refs/tags/`)
///
/// # Errors
///
/// Returns `ServiceError` if:
/// - Checkout operation fails (reference not found, conflicts, etc.)
/// - Operation times out (1 minute timeout)
///
/// # Examples
///
/// ```no_run
/// use fastskill_core::storage::git::checkout_branch_or_tag;
/// use std::path::Path;
///
/// # async fn example() -> Result<(), fastskill_core::core::service::ServiceError> {
/// checkout_branch_or_tag(Path::new("/path/to/repo"), "main", true).await?;
/// # Ok(())
/// # }
/// ```
pub async fn checkout_branch_or_tag(
    repo_path: &Path,
    ref_name: &str,
    is_branch: bool,
) -> Result<(), ServiceError> {
    // Build checkout command (fully-qualified ref, SEC-12 — see build_checkout_args).
    let args = build_checkout_args(ref_name, is_branch);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    // Execute checkout (1 minute timeout)
    let checkout_timeout = Duration::from_secs(60); // 1 minute
    let output = execute_git_command(&args, checkout_timeout, Some(repo_path)).await?;

    if output.exit_code != 0 {
        return Err(GitError::CheckoutFailed {
            ref_name: ref_name.to_string(),
            stderr: output.stderr,
        }
        .into());
    }

    // Note: is_branch parameter is kept for API compatibility but not used
    // (git checkout works the same for branches and tags)

    Ok(())
}

/// Validate that a cloned repository contains a valid skill structure.
///
/// A valid skill structure must contain a `SKILL.md` file either at the repository root
/// or in a subdirectory.
///
/// # Arguments
///
/// * `cloned_path` - Path to the cloned repository directory
///
/// # Returns
///
/// Returns the path to the directory containing `SKILL.md` (may be a subdirectory).
///
/// # Errors
///
/// Returns `ServiceError::Validation` if `SKILL.md` is not found in the repository.
///
/// # Examples
///
/// ```no_run
/// use fastskill_core::storage::git::validate_cloned_skill;
/// use std::path::Path;
///
/// # fn example() -> Result<(), fastskill_core::core::service::ServiceError> {
/// let skill_path = validate_cloned_skill(Path::new("/path/to/cloned/repo"))?;
/// // skill_path points to directory containing SKILL.md
/// # Ok(())
/// # }
/// ```
pub fn validate_cloned_skill(cloned_path: &Path) -> Result<PathBuf, ServiceError> {
    // Check if SKILL.md exists at the root
    let skill_file = cloned_path.join("SKILL.md");
    if skill_file.exists() {
        return Ok(cloned_path.to_path_buf());
    }

    // Check subdirectories for SKILL.md
    let entries = std::fs::read_dir(cloned_path)
        .map_err(|e| ServiceError::Custom(format!("Failed to read cloned directory: {}", e)))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| ServiceError::Custom(format!("Failed to read directory entry: {}", e)))?;
        let path = entry.path();
        if path.is_dir() {
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                return Ok(path);
            }
        }
    }

    Err(ServiceError::Validation(
        "Cloned repository does not contain a valid skill structure (SKILL.md not found)"
            .to_string(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

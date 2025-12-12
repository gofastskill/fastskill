//! Git operations for cloning skill repositories using system git binary

use crate::core::service::ServiceError;
use crate::core::sources::SourceAuth;
use std::path::{Path, PathBuf};
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
    let output = Command::new("git").arg("--version").output().await.map_err(|e| {
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
    let version_part = version_part.split('(').next().unwrap_or(version_part).trim();
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
    let patch = version_numbers.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

    Ok(GitVersion::new(major, minor, patch))
}

/// Command output structure
#[allow(dead_code)] // stdout may be used for future progress parsing
pub(crate) struct CommandOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
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
    lower_stderr.contains("network")
        || lower_stderr.contains("connection")
        || lower_stderr.contains("timeout")
        || lower_stderr.contains("unable to access")
        || lower_stderr.contains("failed to connect")
        || lower_stderr.contains("connection refused")
        || lower_stderr.contains("name resolution")
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
/// * `auth` - **Deprecated**: Ignored, system git handles authentication automatically
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
/// use fastskill::storage::git::clone_repository;
///
/// # async fn example() -> Result<(), fastskill::core::service::ServiceError> {
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
    // Log deprecation warning if auth is provided
    if auth.is_some() {
        warn!(
            "SourceAuth parameter is deprecated. System git handles authentication automatically. \
             Please configure git credentials (SSH keys or credential helper) instead."
        );
    }

    // Check git version first
    check_git_version().await?;

    // Create temporary directory
    let temp_dir = TempDir::new().map_err(|e| {
        ServiceError::Custom(format!("Failed to create temporary directory: {}", e))
    })?;

    info!("Cloning repository: {}", url);

    // Build clone command arguments
    let mut clone_args = vec!["clone", "--depth=1", "--quiet"];

    // Add branch or tag if specified
    if let Some(branch) = branch {
        clone_args.extend(&["--branch", branch]);
    } else if let Some(tag) = tag {
        clone_args.extend(&["--branch", tag]);
    }

    // Add single-branch and no-tags for optimization
    clone_args.push("--single-branch");
    clone_args.push("--no-tags");

    // Add URL and destination
    clone_args.push(url);
    clone_args.push(temp_dir.path().to_str().ok_or_else(|| {
        ServiceError::Custom("Failed to convert temp directory path to string".to_string())
    })?);

    // Execute clone with retry (5 minute timeout, max 3 attempts)
    let clone_timeout = Duration::from_secs(300); // 5 minutes
    let output = execute_git_command_with_retry(&clone_args, clone_timeout, None, 3).await?;

    if output.exit_code != 0 {
        // Clean up on failure
        drop(temp_dir);
        return Err(GitError::CloneFailed {
            url: url.to_string(),
            stderr: output.stderr,
        }
        .into());
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
/// * `_is_branch` - Whether the reference is a branch (kept for API compatibility)
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
/// use fastskill::storage::git::checkout_branch_or_tag;
/// use std::path::Path;
///
/// # async fn example() -> Result<(), fastskill::core::service::ServiceError> {
/// checkout_branch_or_tag(Path::new("/path/to/repo"), "main", true).await?;
/// # Ok(())
/// # }
/// ```
pub async fn checkout_branch_or_tag(
    repo_path: &Path,
    ref_name: &str,
    _is_branch: bool,
) -> Result<(), ServiceError> {
    // Build checkout command
    let args = vec!["checkout", ref_name];

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
/// use fastskill::storage::git::validate_cloned_skill;
/// use std::path::Path;
///
/// # fn example() -> Result<(), fastskill::core::service::ServiceError> {
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

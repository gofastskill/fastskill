//! Path security utilities to prevent directory traversal attacks

use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathSecurityError {
    #[error("Path traversal attempt detected: {0}")]
    TraversalAttempt(String),

    #[error("Invalid path component: {0}")]
    InvalidComponent(String),

    #[error("Path canonicalization failed: {0}")]
    CanonicalizationFailed(String),

    #[error("Path escapes root directory: {0}")]
    EscapesRoot(String),
}

/// Sanitize a path component by removing or replacing dangerous characters
/// Allows only alphanumeric characters, hyphens, underscores, and dots
pub fn sanitize_path_component(component: &str) -> String {
    component
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Normalize a path by resolving . and .. components in memory
/// This function does not access the filesystem and works on non-existent paths
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                result.push(component);
            }
            Component::CurDir => {
                // Skip current directory components
            }
            Component::ParentDir => {
                // Pop from result if possible, otherwise we've escaped root
                if !result.pop() {
                    // Path tries to escape, return empty to indicate invalid
                    return PathBuf::new();
                }
            }
            Component::Normal(s) => {
                result.push(s);
            }
        }
    }
    result
}

/// Validate that a path stays within the allowed root directory
/// This prevents directory traversal attacks
pub fn validate_path_within_root(path: &Path, root: &Path) -> Result<PathBuf, PathSecurityError> {
    // Resolve both paths to absolute paths
    let abs_root = root.canonicalize().map_err(|e| {
        PathSecurityError::CanonicalizationFailed(format!("Failed to canonicalize root: {}", e))
    })?;

    // Resolve `path` into the *same* form as `abs_root` whether or not it
    // exists yet, so the containment test below compares like with like.
    let abs_path = resolve_against_existing_ancestor(path).ok_or_else(|| {
        PathSecurityError::EscapesRoot(format!(
            "Path '{}' attempts to escape root directory '{}'",
            path.display(),
            abs_root.display()
        ))
    })?;

    // Check if the resolved path is within the root
    if !abs_path.starts_with(&abs_root) {
        return Err(PathSecurityError::EscapesRoot(format!(
            "Path '{}' attempts to escape root directory '{}'",
            abs_path.display(),
            abs_root.display()
        )));
    }

    Ok(abs_path)
}

/// Resolve `path` to canonical form, canonicalizing the deepest ancestor that
/// actually exists and re-appending the part that does not.
///
/// Both sides of a containment check must be in the same form.
/// `canonicalize` fails outright when the leaf is missing -- and callers
/// routinely ask about a path that has not been created yet -- so the previous
/// implementation fell back to stripping the *raw* `root`, leaving one side
/// canonical and the other raw. On Windows those two forms never share a
/// prefix (`\\?\C:\Users\runneradmin\...` against `C:\Users\RUNNER~1\...`:
/// verbatim prefix vs 8.3 short name), and a symlinked root reproduces the
/// same split on Unix, so an ordinary in-root path was reported as an escape.
///
/// `..` inside the missing remainder is folded away by [`normalize_path`]
/// rather than re-appended literally -- a literal `..` component would
/// otherwise sail through the component-wise prefix test. Returns `None` when
/// the remainder walks above its own anchor, which is a traversal attempt.
fn resolve_against_existing_ancestor(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = path.canonicalize() {
        return Some(canonical);
    }

    // Peel components off the tail until what is left exists. `..` and `.` go
    // into the remainder like any other component, so they are folded by
    // `normalize_path` below instead of terminating the walk.
    let mut anchor = path.to_path_buf();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    while !anchor.exists() {
        match anchor.components().next_back() {
            Some(Component::Prefix(_)) | Some(Component::RootDir) | None => break,
            Some(component) => suffix.push(component.as_os_str().to_os_string()),
        }
        if !anchor.pop() {
            break;
        }
    }

    // A relative path whose every component was consumed is relative to the
    // process's current directory, which is what the caller means by it.
    let anchor = if anchor.as_os_str().is_empty() {
        std::env::current_dir().ok()?
    } else {
        anchor.canonicalize().ok()?
    };

    let mut remainder = PathBuf::new();
    for component in suffix.iter().rev() {
        remainder.push(component);
    }
    if remainder.as_os_str().is_empty() {
        return Some(anchor);
    }
    let normalized = normalize_path(&remainder);
    if normalized.as_os_str().is_empty() {
        // `normalize_path` returns empty when `..` climbs above the remainder.
        return None;
    }
    Some(anchor.join(normalized))
}

/// Validate and sanitize a user-provided path component
/// Returns an error if the component contains path traversal attempts
pub fn validate_path_component(component: &str) -> Result<String, PathSecurityError> {
    // An empty component is what a leading separator splits into, so accepting
    // it lets an absolute path through a caller that validates componentwise.
    if component.is_empty() {
        return Err(PathSecurityError::InvalidComponent(
            "Path component is empty".to_string(),
        ));
    }

    // Check for obvious traversal attempts
    if component.contains("..") || component.contains('/') || component.contains('\\') {
        return Err(PathSecurityError::TraversalAttempt(format!(
            "Path component '{}' contains directory traversal characters",
            component
        )));
    }

    // Check for absolute paths
    if component.starts_with('/') || (cfg!(windows) && component.contains(':')) {
        return Err(PathSecurityError::InvalidComponent(format!(
            "Path component '{}' appears to be an absolute path",
            component
        )));
    }

    Ok(component.to_string())
}

/// Safely join a user-provided path to a root directory
/// Validates that the result stays within the root
pub fn safe_join(root: &Path, user_path: &str) -> Result<PathBuf, PathSecurityError> {
    // First validate each component
    let components: Vec<&str> = user_path.split('/').collect();
    for component in components {
        validate_path_component(component)?;
    }

    // Join and validate the final path. `validate_path_within_root` resolves
    // against the nearest existing ancestor, so it is correct for targets that
    // do not exist yet -- which is the common case here, since callers are
    // usually about to create the path. Returning early for those would skip
    // containment for exactly the paths that need it.
    let joined = root.join(user_path);

    validate_path_within_root(&joined, root)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn safe_join_rejects_an_absolute_user_path_that_does_not_exist_yet() {
        let tmp = TempDir::new().unwrap();
        // `Path::join` *replaces* the root when its argument is absolute, so
        // `root.join("/etc/x")` is just `/etc/x`. `safe_join` used to
        // short-circuit on `!joined.exists()` and hand that back unchecked, so
        // the containment test never ran for exactly the paths a caller is
        // about to create.
        let escaped = safe_join(tmp.path(), "/tmp/fastskill-safe-join-probe/x");
        assert!(
            escaped.is_err(),
            "safe_join must not return a path outside the root; got {:?}",
            escaped.map(|p| p.display().to_string())
        );
    }

    #[test]
    fn safe_join_still_allows_a_nonexistent_path_inside_the_root() {
        let tmp = TempDir::new().unwrap();
        let joined = safe_join(tmp.path(), "sub/dir/file.md")
            .expect("a relative in-root path that does not exist yet must be allowed");
        let root = tmp.path().canonicalize().unwrap();
        assert!(
            joined.starts_with(&root),
            "{} should be under {}",
            joined.display(),
            root.display()
        );
    }

    #[test]
    fn validate_path_component_rejects_the_empty_component() {
        // `"/etc/passwd".split('/')` yields "" for the leading separator.
        // Accepting it is what let an absolute path through `safe_join`.
        assert!(validate_path_component("").is_err());
    }

    #[test]
    fn test_sanitize_path_component() {
        assert_eq!(sanitize_path_component("valid-name_123"), "valid-name_123");
        assert_eq!(sanitize_path_component("../etc/passwd"), "..etcpasswd");
        assert_eq!(sanitize_path_component("../../"), "...."); // dots are allowed (for file extensions)
        assert_eq!(
            sanitize_path_component("file with spaces"),
            "filewithspaces"
        );
    }

    #[test]
    fn test_validate_path_component() {
        assert!(validate_path_component("valid-name").is_ok());
        assert!(validate_path_component("valid_name_123").is_ok());
        assert!(validate_path_component("..").is_err());
        assert!(validate_path_component("../etc").is_err());
        assert!(validate_path_component("/etc/passwd").is_err());
        assert!(validate_path_component("path/to/file").is_err());
    }

    #[test]
    fn test_validate_path_within_root() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create a test file within root
        let valid_file = root.join("valid.txt");
        fs::write(&valid_file, "test").unwrap();

        // Test valid path
        assert!(validate_path_within_root(&valid_file, root).is_ok());

        // Test path outside root
        let outside_path = temp_dir.path().parent().unwrap().join("outside.txt");
        if outside_path.exists() {
            assert!(validate_path_within_root(&outside_path, root).is_err());
        }
    }

    #[test]
    fn test_safe_join() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Valid join
        assert!(safe_join(root, "subdir/file.txt").is_ok());

        // Invalid traversal
        assert!(safe_join(root, "../etc/passwd").is_err());
        assert!(safe_join(root, "subdir/../../etc").is_err());
    }

    #[test]
    fn test_validated_return_value_is_safe() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Validate path component and get the safe return value
        let safe_component = validate_path_component("valid-name").unwrap();

        // Canonicalize the root FIRST, then join, so both sides of the
        // containment check are in the same form. `path` does not exist, so
        // `path.canonicalize()` fails and falls back to the raw path; on
        // Windows `root.canonicalize()` meanwhile returns an extended-length
        // path (`\\?\C:\...`), and a raw path never starts with that.
        let canonical_root = root.canonicalize().unwrap_or(root.to_path_buf());
        let path = canonical_root.join(&safe_component);

        // Verify the path is safe and doesn't escape root
        let canonical_path = path.canonicalize().unwrap_or(path);
        assert!(canonical_path.starts_with(&canonical_root));

        // Verify the validated string doesn't contain dangerous characters
        assert!(!safe_component.contains(".."));
        assert!(!safe_component.contains('/'));
        assert!(!safe_component.contains('\\'));
    }

    /// The Windows shape, reproduced on Unix with a symlink: the caller holds
    /// the root canonically (`AppState::canonicalize_path` does exactly this)
    /// while the candidate arrives raw -- an absolute `skill_file` read back
    /// from a manifest -- and neither the leaf nor its parent exists yet.
    ///
    /// The missing-parent branch stripped the *raw* `root` from a path that is
    /// in canonical-root form only by accident, so on Windows
    /// (`\\?\C:\Users\runneradmin\...` against `C:\Users\RUNNER~1\...`)
    /// the strip always failed, the whole absolute path was re-joined onto the
    /// canonical root, and an ordinary in-root path was reported as an escape.
    #[cfg(unix)]
    #[test]
    fn nonexistent_path_under_a_symlinked_root_is_not_an_escape() {
        let temp_dir = TempDir::new().unwrap();
        let tmp = temp_dir.path().canonicalize().unwrap();
        let real = tmp.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = tmp.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // Leaf missing, parent missing too: the branch under test.
        let candidate = link.join("pending").join("SKILL.md");
        let resolved = validate_path_within_root(&candidate, &real).unwrap_or_else(|e| {
            panic!("in-root path under a symlinked root reported as an escape: {e}")
        });
        assert_eq!(resolved, real.join("pending").join("SKILL.md"));

        // And the same path expressed through the link as the root resolves
        // identically -- both sides land in canonical form.
        let via_link = validate_path_within_root(&candidate, &link).unwrap();
        assert_eq!(via_link, real.join("pending").join("SKILL.md"));
    }

    /// `..` inside the *missing* remainder must still fold away rather than be
    /// re-appended literally, or a traversal would slip past the component-wise
    /// containment test as a literal `..` component.
    #[test]
    fn nonexistent_path_folds_dotdot_in_the_missing_remainder() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().canonicalize().unwrap();

        let resolved = validate_path_within_root(&root.join("missing/../inner.md"), &root).unwrap();
        assert_eq!(resolved, root.join("inner.md"));
        assert!(
            !resolved.components().any(|c| c.as_os_str() == ".."),
            "a literal `..` survived into the validated path: {}",
            resolved.display()
        );
    }

    #[test]
    fn test_validate_path_within_root_nonexistent_traversal_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Test path that does not exist and would escape when resolved
        let escape_path = root.join("subdir/../../escape");
        let result = validate_path_within_root(&escape_path, root);

        assert!(matches!(result, Err(PathSecurityError::EscapesRoot(_))));
    }
}

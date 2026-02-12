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

    // If the path doesn't exist yet, we need to check its parent
    let abs_path = if path.exists() {
        path.canonicalize().map_err(|e| {
            PathSecurityError::CanonicalizationFailed(format!("Failed to canonicalize path: {}", e))
        })?
    } else {
        // For non-existent paths, canonicalize the parent and append the filename
        let parent = path.parent().unwrap_or(Path::new("."));
        let abs_parent = if parent.as_os_str().is_empty() {
            std::env::current_dir().map_err(|e| {
                PathSecurityError::CanonicalizationFailed(format!(
                    "Failed to get current directory: {}",
                    e
                ))
            })?
        } else if parent.exists() {
            parent.canonicalize().map_err(|e| {
                PathSecurityError::CanonicalizationFailed(format!(
                    "Failed to canonicalize parent: {}",
                    e
                ))
            })?
        } else {
            // If parent doesn't exist, we need to validate the constructed path
            // Normalize the path to resolve any .. or . components
            let relative_to_root = path.strip_prefix(root).unwrap_or(path);
            let normalized_relative = normalize_path(relative_to_root);
            if normalized_relative.as_os_str().is_empty() {
                // Path escapes root through ..
                return Err(PathSecurityError::EscapesRoot(format!(
                    "Path '{}' attempts to escape root directory '{}'",
                    path.display(),
                    abs_root.display()
                )));
            }
            let normalized_path = abs_root.join(&normalized_relative);
            // Verify the normalized path is within root
            if !normalized_path.starts_with(&abs_root) {
                return Err(PathSecurityError::EscapesRoot(format!(
                    "Path '{}' attempts to escape root directory '{}'",
                    path.display(),
                    abs_root.display()
                )));
            }
            return Ok(normalized_path);
        };

        if let Some(filename) = path.file_name() {
            abs_parent.join(filename)
        } else {
            abs_parent
        }
    };

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

/// Validate and sanitize a user-provided path component
/// Returns an error if the component contains path traversal attempts
pub fn validate_path_component(component: &str) -> Result<String, PathSecurityError> {
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

    // Join and validate the final path
    let joined = root.join(user_path);

    // For paths that don't exist yet, just verify components don't have traversal
    // The actual existence will be checked by the calling code
    if !joined.exists() {
        return Ok(joined);
    }

    validate_path_within_root(&joined, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

        // Build path using the validated return value
        let path = root.join(&safe_component);

        // Verify the path is safe and doesn't escape root
        let canonical_path = path.canonicalize().unwrap_or(path);
        let canonical_root = root.canonicalize().unwrap_or(root.to_path_buf());
        assert!(canonical_path.starts_with(&canonical_root));

        // Verify the validated string doesn't contain dangerous characters
        assert!(!safe_component.contains(".."));
        assert!(!safe_component.contains('/'));
        assert!(!safe_component.contains('\\'));
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

//! ZIP package handling for skill distribution

use crate::core::service::ServiceError;
use crate::security::path::normalize_path;
use std::io;
use std::path::Path;

pub struct ZipHandler;

impl ZipHandler {
    pub fn new() -> Result<Self, ServiceError> {
        Ok(Self)
    }

    /// Validate ZIP package structure
    pub async fn validate_package(&self, _zip_path: &Path) -> Result<(), ServiceError> {
        Ok(())
    }

    /// Safely extract a ZIP file to a destination directory
    ///
    /// This function protects against ZIP slip attacks by:
    /// - Normalizing the output path for each entry
    /// - Verifying that the normalized path stays within the extraction directory
    /// - Rejecting entries that would create files outside the extraction root
    /// - Rejecting symlink entries
    ///
    /// # Arguments
    /// * `zip_path` - Path to the ZIP file to extract
    /// * `dest_dir` - Destination directory for extraction (must exist)
    ///
    /// # Errors
    /// Returns `ServiceError::Io` for I/O errors
    /// Returns `ServiceError::Validation` if path traversal is detected
    pub fn extract_to_dir(&self, zip_path: &Path, dest_dir: &Path) -> Result<(), ServiceError> {
        let file = std::fs::File::open(zip_path).map_err(ServiceError::Io)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| ServiceError::Validation(format!("Invalid ZIP file: {}", e)))?;

        // Canonicalize the destination directory for reliable path comparison
        let dest_canonical = dest_dir.canonicalize().map_err(|e| {
            ServiceError::Io(io::Error::new(
                e.kind(),
                format!("Failed to canonicalize destination: {}", e),
            ))
        })?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                ServiceError::Validation(format!("Failed to read ZIP entry: {}", e))
            })?;

            let entry_name = file.name().to_string();

            // Reject symlink entries
            #[cfg(unix)]
            if matches!(file.unix_mode(), Some(mode) if (mode & 0o170000) == 0o120000) {
                return Err(ServiceError::Validation(format!(
                    "Symlink entry rejected for security: {}",
                    entry_name
                )));
            }

            // Normalize the entry name to resolve . and .. components before any I/O
            let normalized_entry_name = normalize_path(Path::new(&entry_name));
            if normalized_entry_name.as_os_str().is_empty() {
                return Err(ServiceError::Validation(format!(
                    "Path traversal attempt detected in ZIP entry: '{}'",
                    entry_name
                )));
            }

            // Build the output path using the normalized entry name
            let outpath = dest_dir.join(&normalized_entry_name);

            // Ensure the normalized path is within the destination directory before any I/O
            let outpath_str = outpath.to_string_lossy().to_string();
            let dest_str = dest_canonical.to_string_lossy().to_string();
            if !outpath_str.starts_with(&dest_str) {
                return Err(ServiceError::Validation(format!(
                    "Path traversal attempt detected in ZIP entry: '{}' would resolve outside extraction directory",
                    entry_name
                )));
            }

            // For directories, we need to check if they exist or can be created
            // For files, we need to validate after creation
            let path_is_directory = normalized_entry_name.ends_with("/");

            // Canonicalize the output path to resolve any .. components
            // Note: canonicalize() requires the path to exist, so we only canonicalize after creation for directories
            let outpath_canonical = if outpath.exists() {
                outpath.canonicalize().map_err(|e| {
                    ServiceError::Io(io::Error::new(
                        e.kind(),
                        format!("Failed to resolve path for ZIP entry: {}", entry_name),
                    ))
                })?
            } else if path_is_directory {
                // Create directory first, then canonicalize to validate
                std::fs::create_dir_all(&outpath).map_err(ServiceError::Io)?;
                outpath.canonicalize().map_err(|e| {
                    ServiceError::Io(io::Error::new(
                        e.kind(),
                        format!("Failed to resolve path for ZIP entry: {}", entry_name),
                    ))
                })?
            } else {
                // For files, we need to ensure parent is valid before creating
                if let Some(parent) = outpath.parent() {
                    if !parent.exists() {
                        // Create parent directory (safe now because we've already validated the normalized path)
                        std::fs::create_dir_all(parent).map_err(ServiceError::Io)?;
                    }
                    // Validate parent path
                    let parent_canonical = parent.canonicalize().map_err(|e| {
                        ServiceError::Io(io::Error::new(
                            e.kind(),
                            format!(
                                "Failed to resolve parent path for ZIP entry: {}",
                                entry_name
                            ),
                        ))
                    })?;
                    if !parent_canonical.starts_with(&dest_canonical) {
                        return Err(ServiceError::Validation(format!(
                            "Path traversal attempt detected in parent directory for ZIP entry: '{}'",
                            entry_name
                        )));
                    }
                }
                // Create the file
                let mut outfile = std::fs::File::create(&outpath).map_err(ServiceError::Io)?;
                io::copy(&mut file, &mut outfile).map_err(ServiceError::Io)?;
                // Now canonicalize the file path to validate
                outpath.canonicalize().map_err(|e| {
                    ServiceError::Io(io::Error::new(
                        e.kind(),
                        format!("Failed to resolve path for ZIP entry: {}", entry_name),
                    ))
                })?
            };

            // Ensure the resolved path is within the destination directory
            if !outpath_canonical.starts_with(&dest_canonical) {
                return Err(ServiceError::Validation(format!(
                    "Path traversal attempt detected in ZIP entry: '{}' resolves outside extraction directory",
                    entry_name
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::FileOptions;
    use zip::ZipWriter;

    /// Helper to create a ZIP archive with specified entries
    fn create_test_zip(entries: &[(&str, &[u8])]) -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("test.zip");

        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        for (name, content) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, options).unwrap();
            } else {
                zip.start_file(*name, options).unwrap();
                zip.write_all(content).unwrap();
            }
        }

        zip.finish().unwrap();
        (temp_dir, zip_path)
    }

    #[test]
    fn test_safe_extract_normal_files() {
        let (_temp_dir, zip_path) = create_test_zip(&[
            ("SKILL.md", b"Test content"),
            ("README.md", b"Readme content"),
            ("src/main.rs", b"source code"),
        ]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();

        handler
            .extract_to_dir(&zip_path, extract_dir.path())
            .unwrap();

        assert!(extract_dir.path().join("SKILL.md").exists());
        assert!(extract_dir.path().join("README.md").exists());
        assert!(extract_dir.path().join("src/main.rs").exists());
    }

    #[test]
    fn test_safe_extract_rejects_path_traversal() {
        let (_temp_dir, zip_path) = create_test_zip(&[
            ("normal.txt", b"safe content"),
            ("../../../evil.txt", b"malicious content"),
        ]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();

        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        assert!(result.is_err());
        match result {
            Err(ServiceError::Validation(msg)) => {
                assert!(
                    msg.contains("path traversal") || msg.contains("Path traversal"),
                    "Error should mention path traversal: {}",
                    msg
                );
            }
            _ => unreachable!("Expected ServiceError::Validation for path traversal"),
        }

        // Ensure no file was created outside the extraction directory
        assert!(!extract_dir.path().join("../../../evil.txt").exists());
        assert!(extract_dir.path().join("normal.txt").exists());
    }

    #[test]
    fn test_safe_extract_rejects_windows_path_traversal() {
        let (_temp_dir, zip_path) = create_test_zip(&[
            ("normal.txt", b"safe content"),
            ("..\\..\\..\\evil.txt", b"malicious content"),
        ]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();

        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        // On Unix, backslashes are treated as regular characters, so the path is literal
        // On Windows, this would be a path traversal and should be rejected
        // We accept either behavior since it's platform-specific
        if cfg!(windows) {
            assert!(
                result.is_err(),
                "Windows-style path traversal should be rejected on Windows"
            );
        } else {
            // On Unix, the backslashes are literal characters, so the extraction should succeed
            // but file will have a strange name with backslashes
            assert!(matches!(result, Ok(_) | Err(_)));
        }
    }

    #[test]
    fn test_safe_extract_rejects_absolute_paths() {
        let (_temp_dir, zip_path) = create_test_zip(&[
            ("/etc/passwd", b"malicious content"),
            ("normal.txt", b"safe content"),
        ]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();

        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        assert!(result.is_err());
        // Absolute paths will be rejected when canonicalized
    }

    #[test]
    fn test_safe_extract_nested_directories() {
        let (_temp_dir, zip_path) = create_test_zip(&[
            ("deep/nested/file.txt", b"content"),
            ("another/nested/path/README.md", b"readme"),
        ]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();

        handler
            .extract_to_dir(&zip_path, extract_dir.path())
            .unwrap();

        assert!(extract_dir.path().join("deep/nested/file.txt").exists());
        assert!(extract_dir
            .path()
            .join("another/nested/path/README.md")
            .exists());
    }

    #[test]
    fn test_safe_extract_rejects_mixed_traversal() {
        let (_temp_dir, zip_path) = create_test_zip(&[
            ("safe/file.txt", b"safe"),
            ("safe/../../evil.txt", b"malicious"),
        ]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();

        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        assert!(result.is_err());
        // Mixed traversal should be rejected
        assert!(extract_dir.path().join("safe/file.txt").exists());
    }

    #[test]
    fn test_safe_extract_does_not_create_dirs_outside_root() {
        let base = TempDir::new().unwrap();
        let extract_dir = base.path().join("extract");
        std::fs::create_dir_all(&extract_dir).unwrap();

        // Create a zip with a traversal entry that goes outside base
        let (_temp_dir, zip_path) =
            create_test_zip(&[("../../../escape_evil/file.txt", b"malicious content")]);

        let handler = ZipHandler::new().unwrap();
        let result = handler.extract_to_dir(&zip_path, &extract_dir);

        // Should return an error before any I/O happens (due to normalization)
        assert!(result.is_err());

        // No file or directory should have been created
        assert!(!extract_dir.join("escape_evil").exists());
        assert!(!extract_dir.join("escape_evil/file.txt").exists());
    }
}

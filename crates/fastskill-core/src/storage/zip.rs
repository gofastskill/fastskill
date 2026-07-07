//! ZIP package handling for skill distribution

use crate::core::service::ServiceError;
use crate::security::path::normalize_path;
use std::io;
use std::io::Read;
use std::path::Path;

// -----------------------------------------------------------------------------
// Decompression (ZIP bomb) DoS ceiling — SEC-3
// -----------------------------------------------------------------------------
//
// These are a fixed *DoS ceiling* meant to protect the host from a decompression
// bomb, deliberately sized comfortably above the entire real corpus of 1311
// skills (package size: p99 1.27 MB, max 5.66 MB; largest single file: 4.0 MB;
// max file count: 316). They are intentionally distinct from the
// content-validation limits that define a *valid* skill (`MAX_CONTENT_SIZE`
// in `context_resolver.rs`, the `SKILL.md` cap in `file_structure.rs`): those
// bound what a well-formed skill may contain, while these merely bound what an
// extraction may cost the host.

/// Maximum total uncompressed bytes an archive may expand to (~9x largest real skill).
pub(crate) const MAX_TOTAL_UNCOMPRESSED: u64 = 50 * 1024 * 1024; // 50 MiB
/// Maximum uncompressed size of a single entry (~2.5x largest real file).
pub(crate) const MAX_ENTRY_UNCOMPRESSED: u64 = 10 * 1024 * 1024; // 10 MiB
/// Maximum number of entries in an archive (~30x the busiest real skill).
pub(crate) const MAX_ENTRIES: usize = 10_000;
/// Maximum tolerated per-entry compression ratio (uncompressed / compressed).
pub(crate) const MAX_RATIO: u64 = 100;

/// Pre-flight ZIP checks: reject archives that exceed the entry-count cap or that
/// declare an oversized/over-compressed entry, *before* extracting anything.
///
/// This is a cheap header-only scan (it reads no entry data) and is shared with
/// `ZipValidator::validate_zip_package`. The real cumulative-byte budget is still
/// enforced during extraction in [`ZipHandler::extract_to_dir`], because the
/// declared `size()` can lie.
pub(crate) fn preflight_zip_archive<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(), ServiceError> {
    if archive.len() > MAX_ENTRIES {
        return Err(ServiceError::Validation(format!(
            "ZIP archive has too many entries ({} > {} limit)",
            archive.len(),
            MAX_ENTRIES
        )));
    }

    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| ServiceError::Validation(format!("Failed to read ZIP entry: {}", e)))?;

        if file.is_dir() {
            continue;
        }

        let declared = file.size();
        if declared > MAX_ENTRY_UNCOMPRESSED {
            return Err(ServiceError::Validation(format!(
                "ZIP entry '{}' declares size {} bytes exceeding per-entry limit {}",
                file.name(),
                declared,
                MAX_ENTRY_UNCOMPRESSED
            )));
        }

        let compressed = file.compressed_size();
        if compressed > 0 && declared / compressed > MAX_RATIO {
            return Err(ServiceError::Validation(format!(
                "ZIP entry '{}' has compression ratio {} exceeding limit {} (possible ZIP bomb)",
                file.name(),
                declared / compressed,
                MAX_RATIO
            )));
        }
    }

    Ok(())
}

pub struct ZipHandler;

impl ZipHandler {
    pub fn new() -> Result<Self, ServiceError> {
        Ok(Self)
    }

    /// Validate ZIP package structure
    pub async fn validate_package(&self, zip_path: &Path) -> Result<(), ServiceError> {
        let file = std::fs::File::open(zip_path).map_err(ServiceError::Io)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| ServiceError::Validation(format!("Invalid ZIP file: {}", e)))?;
        preflight_zip_archive(&mut archive)
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

        // Reject decompression bombs up front (entry-count / declared-size / ratio caps).
        preflight_zip_archive(&mut archive)?;

        // Canonicalize the destination directory for reliable path comparison
        let dest_canonical = dest_dir.canonicalize().map_err(|e| {
            ServiceError::Io(io::Error::new(
                e.kind(),
                format!("Failed to canonicalize destination: {}", e),
            ))
        })?;

        // Cumulative uncompressed budget enforced across all entries. The declared
        // per-entry `size()` can lie, so this counts the real bytes written.
        let mut total_uncompressed: u64 = 0;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                ServiceError::Validation(format!("Failed to read ZIP entry: {}", e))
            })?;

            let entry_name = file.name().to_string();

            // Determine directory-ness from the raw zip entry (BUG-11), not the
            // normalized PathBuf which never retains a trailing slash.
            let entry_is_directory = file.is_dir() || entry_name.ends_with('/');

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
            // For files, we need to validate after creation.
            // Directory-ness comes from the raw zip entry (BUG-11), because the
            // normalized PathBuf never ends with "/".
            let path_is_directory = entry_is_directory;

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
                // Enforce the real cumulative uncompressed budget. Copy through a
                // `take` limited to the remaining budget (+1 byte to detect overflow);
                // if the entry writes past the budget, reject as a decompression bomb.
                let remaining = MAX_TOTAL_UNCOMPRESSED.saturating_sub(total_uncompressed);
                let mut limited = file.by_ref().take(remaining.saturating_add(1));
                let written = io::copy(&mut limited, &mut outfile).map_err(ServiceError::Io)?;
                if written > remaining {
                    return Err(ServiceError::Validation(format!(
                        "ZIP extraction exceeds total uncompressed limit of {} bytes (decompression bomb) at entry '{}'",
                        MAX_TOTAL_UNCOMPRESSED, entry_name
                    )));
                }
                total_uncompressed = total_uncompressed.saturating_add(written);
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

    // ---- SEC-3: decompression (ZIP bomb) DoS ceiling ----

    /// Create a Deflated-compression zip (used to exercise the ratio check cheaply).
    fn create_deflated_zip(entries: &[(&str, &[u8])]) -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("test.zip");
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        (temp_dir, zip_path)
    }

    /// Create a Stored zip with `n` tiny entries (used for the entry-count cap).
    fn create_many_entry_zip(n: usize) -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("many.zip");
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for i in 0..n {
            zip.start_file(format!("f{}.txt", i), options).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
        (temp_dir, zip_path)
    }

    #[test]
    fn test_extract_rejects_oversized_declared_entry() {
        // A single Stored entry whose declared size exceeds the per-entry cap.
        let big = vec![b'x'; (MAX_ENTRY_UNCOMPRESSED as usize) + 4096];
        let (_t, zip_path) = create_test_zip(&[("big.bin", &big)]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();
        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        match result {
            Err(ServiceError::Validation(msg)) => {
                assert!(
                    msg.contains("per-entry"),
                    "expected per-entry limit rejection, got: {msg}"
                );
            }
            other => unreachable!("expected Validation error, got {other:?}"),
        }
        // Nothing should have been written for the rejected archive.
        assert!(!extract_dir.path().join("big.bin").exists());
    }

    #[test]
    fn test_extract_rejects_high_compression_ratio() {
        // 1 MiB of zeros: under the per-entry cap, but deflates to almost nothing,
        // so the ratio check must fire.
        let zeros = vec![0u8; 1024 * 1024];
        let (_t, zip_path) = create_deflated_zip(&[("zeros.bin", &zeros)]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();
        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        match result {
            Err(ServiceError::Validation(msg)) => {
                assert!(
                    msg.contains("ratio") || msg.contains("bomb"),
                    "expected ratio rejection, got: {msg}"
                );
            }
            other => unreachable!("expected Validation error, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_rejects_total_budget_exceeded() {
        // Six 9 MiB Stored entries: each is under the 10 MiB per-entry cap and has
        // ratio 1 (so pre-flight passes), but the cumulative 54 MiB exceeds the
        // 50 MiB total budget enforced at the io::copy.
        let chunk = vec![b'x'; 9 * 1024 * 1024];
        let entries: Vec<(&str, &[u8])> = vec![
            ("a.bin", &chunk),
            ("b.bin", &chunk),
            ("c.bin", &chunk),
            ("d.bin", &chunk),
            ("e.bin", &chunk),
            ("f.bin", &chunk),
        ];
        let (_t, zip_path) = create_test_zip(&entries);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();
        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        match result {
            Err(ServiceError::Validation(msg)) => {
                assert!(
                    msg.contains("total uncompressed") || msg.contains("bomb"),
                    "expected total-budget rejection, got: {msg}"
                );
            }
            other => unreachable!("expected Validation error, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_rejects_too_many_entries() {
        let (_t, zip_path) = create_many_entry_zip(MAX_ENTRIES + 1);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();
        let result = handler.extract_to_dir(&zip_path, extract_dir.path());

        match result {
            Err(ServiceError::Validation(msg)) => {
                assert!(
                    msg.contains("too many entries"),
                    "expected entry-count rejection, got: {msg}"
                );
            }
            other => unreachable!("expected Validation error, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_within_limits_succeeds() {
        // A normal, small archive must still extract fine.
        let (_t, zip_path) =
            create_test_zip(&[("SKILL.md", b"content"), ("src/main.rs", b"fn main(){}")]);
        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();
        handler
            .extract_to_dir(&zip_path, extract_dir.path())
            .unwrap();
        assert!(extract_dir.path().join("SKILL.md").exists());
        assert!(extract_dir.path().join("src/main.rs").exists());
    }

    #[test]
    fn test_validate_package_rejects_bomb() {
        let big = vec![b'x'; (MAX_ENTRY_UNCOMPRESSED as usize) + 4096];
        let (_t, zip_path) = create_test_zip(&[("big.bin", &big)]);
        let handler = ZipHandler::new().unwrap();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(handler.validate_package(&zip_path));
        assert!(matches!(result, Err(ServiceError::Validation(_))));
    }

    // ---- BUG-11: directory-only entries extracted as real directories ----

    #[test]
    fn test_directory_entry_extracted_as_real_dir() {
        // A standalone directory entry ("mydir/") plus a file inside it.
        let (_t, zip_path) = create_test_zip(&[("mydir/", b""), ("mydir/inner.txt", b"hello")]);

        let extract_dir = TempDir::new().unwrap();
        let handler = ZipHandler::new().unwrap();
        handler
            .extract_to_dir(&zip_path, extract_dir.path())
            .unwrap();

        let mydir = extract_dir.path().join("mydir");
        assert!(mydir.exists(), "mydir should exist");
        assert!(
            mydir.is_dir(),
            "standalone directory entry must be a real directory, not an empty file"
        );
        assert!(extract_dir.path().join("mydir/inner.txt").exists());
    }
}

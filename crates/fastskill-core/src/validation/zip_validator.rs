//! ZIP package validation implementation

use crate::core::service::ServiceError;
use crate::storage::zip::preflight_zip_archive;
use std::path::Path;

pub struct ZipValidator;

impl Default for ZipValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipValidator {
    pub fn new() -> Self {
        Self
    }

    /// Validate ZIP package.
    ///
    /// Runs the entry-count / declared-size / compression-ratio pre-flight checks
    /// (SEC-3) so an oversized or bomb-shaped archive is rejected before extraction.
    pub async fn validate_zip_package(&self, zip_path: &Path) -> Result<(), ServiceError> {
        let file = std::fs::File::open(zip_path).map_err(ServiceError::Io)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| ServiceError::Validation(format!("Invalid ZIP file: {}", e)))?;
        preflight_zip_archive(&mut archive)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::storage::zip::{MAX_ENTRIES, MAX_ENTRY_UNCOMPRESSED};
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use zip::write::FileOptions;
    use zip::ZipWriter;

    fn write_zip(entries: &[(&str, &[u8])]) -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("test.zip");
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, content) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        (temp_dir, zip_path)
    }

    #[tokio::test]
    async fn test_validate_zip_package_valid() {
        let (_t, zip_path) = write_zip(&[("SKILL.md", b"content")]);
        let validator = ZipValidator::new();
        let result = validator.validate_zip_package(&zip_path).await;
        assert!(result.is_ok(), "valid small zip should pass: {result:?}");
    }

    #[tokio::test]
    async fn test_validate_zip_package_nonexistent_file() {
        let validator = ZipValidator::new();
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("nonexistent.zip");

        // Now surfaces an I/O error for a missing file.
        let result = validator.validate_zip_package(&zip_path).await;
        assert!(matches!(result, Err(ServiceError::Io(_))));
    }

    #[tokio::test]
    async fn test_validate_zip_package_not_a_zip() {
        let validator = ZipValidator::new();
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("test.zip");
        std::fs::write(&zip_path, b"dummy content").unwrap();

        let result = validator.validate_zip_package(&zip_path).await;
        assert!(matches!(result, Err(ServiceError::Validation(_))));
    }

    #[tokio::test]
    async fn test_validate_zip_package_rejects_oversized_entry() {
        let big = vec![b'x'; (MAX_ENTRY_UNCOMPRESSED as usize) + 4096];
        let (_t, zip_path) = write_zip(&[("big.bin", &big)]);
        let validator = ZipValidator::new();
        let result = validator.validate_zip_package(&zip_path).await;
        match result {
            Err(ServiceError::Validation(msg)) => assert!(msg.contains("per-entry")),
            other => unreachable!("expected Validation error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_validate_zip_package_rejects_too_many_entries() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("many.zip");
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for i in 0..(MAX_ENTRIES + 1) {
            zip.start_file(format!("f{i}.txt"), options).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();

        let validator = ZipValidator::new();
        let result = validator.validate_zip_package(&zip_path).await;
        match result {
            Err(ServiceError::Validation(msg)) => assert!(msg.contains("too many entries")),
            other => unreachable!("expected Validation error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_zip_validator_default() {
        #[allow(clippy::default_constructed_unit_structs)]
        let validator = ZipValidator::default();
        let (_t, zip_path) = write_zip(&[("SKILL.md", b"x")]);
        let result = validator.validate_zip_package(&zip_path).await;
        assert!(result.is_ok());
    }
}

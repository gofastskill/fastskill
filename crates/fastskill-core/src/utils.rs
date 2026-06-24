//! Utility functions for the fastskill-core crate

use fs2::FileExt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Append a suffix to a path's full file name — e.g. `org/web.scraper` + `tmp`
/// → `org/web.scraper.tmp`.
///
/// Unlike [`Path::with_extension`], this does NOT replace the last dotted segment.
/// When a file name is derived from user input that may contain dots (e.g. a scoped
/// skill package), `with_extension("tmp")` is not injective — `org/web.scraper` and
/// `org/web.crawler` both collapse onto `org/web.tmp`. Appending keeps each derived
/// sidecar path distinct.
pub(crate) fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// Write `bytes` to `path` atomically: write to `.tmp` sibling → sync → rename.
/// Advisory-locks the tmp file via `fs2` to prevent concurrent writers.
///
/// Returns `LockError::FileLocked` if another process holds the advisory lock.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp_path = append_suffix(path, "tmp");

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Open (or create) the tmp file
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)?;

    // Acquire advisory lock — fail fast if another writer is active
    file.try_lock_exclusive().map_err(|_| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "Lock file is held by another process: {}",
                tmp_path.display()
            ),
        )
    })?;

    // Write all bytes and flush to storage
    use io::Write;
    let mut writer = io::BufWriter::new(&file);
    writer.write_all(bytes)?;
    writer.flush()?;
    file.sync_all()?;

    // Release the advisory lock before rename
    file.unlock()?;

    // Atomic replace
    fs::rename(&tmp_path, path)?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_atomic_write_creates_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.toml");

        atomic_write(&path, b"hello = true").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello = true");
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.toml");

        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "second");
    }

    #[test]
    fn test_atomic_write_no_tmp_file_remains_on_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.toml");
        let tmp_path = append_suffix(&path, "tmp");

        atomic_write(&path, b"data").unwrap();

        assert!(path.exists(), "target file should exist");
        assert!(
            !tmp_path.exists(),
            ".tmp file must not remain after success"
        );
    }

    #[test]
    fn test_atomic_write_interrupted_leaves_original_unchanged() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("skills.lock");

        // Write initial content
        fs::write(&path, b"original content").unwrap();

        // Simulate an interrupted write: write to .tmp, then leave it without renaming
        let tmp_path = append_suffix(&path, "tmp");
        fs::write(&tmp_path, b"incomplete write").unwrap();

        // The original file should still have its original content
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(
            content, "original content",
            "original file must be unchanged when write is interrupted before rename"
        );

        // Now perform a clean atomic write — should succeed and overwrite
        atomic_write(&path, b"new content").unwrap();
        let new_content = fs::read_to_string(&path).unwrap();
        assert_eq!(new_content, "new content");
    }

    #[test]
    fn test_atomic_write_file_locked_returns_error() {
        use fs2::FileExt;
        use std::fs::OpenOptions;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("skills.lock");
        let tmp_path = append_suffix(&path, "tmp");

        // Acquire exclusive lock on the tmp file from this thread
        let holder = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&tmp_path)
            .unwrap();
        holder.lock_exclusive().unwrap();

        // atomic_write should fail because the .tmp file is already locked
        let result = atomic_write(&path, b"data");
        assert!(
            result.is_err(),
            "atomic_write must fail when .tmp is already locked"
        );

        holder.unlock().unwrap();
    }
}

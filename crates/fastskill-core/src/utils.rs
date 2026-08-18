//! Utility functions for the fastskill-core crate

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Encode `bytes` as lowercase hex, e.g. for rendering a SHA-256 digest as a
/// cache key / integrity checksum string.
///
/// This exists instead of a `format!("{:x}", digest)` call (which relied on
/// `GenericArray`'s `LowerHex` impl) because `digest` 0.11 switched to
/// `hybrid_array::Array`, which does **not** implement `LowerHex`. Pulling in
/// the `hex` crate for one function was not worth a new dependency, so this
/// is a small local equivalent — output is byte-for-byte identical to the
/// old `{:x}` formatting (lowercase, no separators, two hex digits per byte),
/// which matters because these strings are persisted as cache-identity keys
/// (see `core::change_detection` and `core::install`).
pub fn to_hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // `write!` to a `String` is infallible.
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Write `bytes` to `path` atomically: write to a **per-writer unique** temp file
/// in the same directory → sync → atomic rename over `path`.
///
/// Each call owns its own temp file (random suffix, via `tempfile`), so a second
/// concurrent writer can never truncate or overwrite the first writer's temp
/// before it is renamed. `fs::rename` is atomic on POSIX, so a reader always sees
/// either the old or a fully-written new file — never a truncated/partial one.
/// The semantics are **last-writer-wins**, which is correct for a byte-level
/// writer: the published file is always some writer's complete content.
///
/// (Previously this used an `fs2` advisory lock on a shared, deterministic
/// `.tmp` path. That raced: the tmp file was truncated at open time *before* the
/// lock was checked, so a second writer could blow away the first writer's synced
/// tmp between its `sync_all` and its `rename`, publishing an empty file — BUG-8.)
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write;

    // Ensure parent directory exists; the temp file must live in the same
    // directory as the target so the final rename is on one filesystem (atomic).
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => {
            fs::create_dir_all(p)?;
            p.to_path_buf()
        }
        _ => PathBuf::from("."),
    };

    // Unique temp file owned by this writer.
    let mut tmp = tempfile::Builder::new()
        .prefix(".fastskill-tmp-")
        .tempfile_in(&parent)?;

    tmp.write_all(bytes)?;
    tmp.flush()?;
    // Sync file contents to storage before the rename publishes them.
    tmp.as_file().sync_all()?;

    // Atomically move the completed temp file over the target.
    tmp.persist(path).map_err(|e| e.error)?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Regression test for the sha2 0.10 -> 0.11 bump: `hybrid_array::Array`
    /// (which `Sha256::digest`/`finalize` now return) does not implement
    /// `LowerHex`, so every hashing call site switched from
    /// `format!("{:x}", digest)` to `to_hex_lower(&digest)`. This pins the
    /// exact output for a known input so a future change to the hex
    /// encoding (case, padding, separators) fails loudly instead of quietly
    /// invalidating every cache entry that uses this string as a key.
    #[test]
    fn test_to_hex_lower_matches_known_sha256() {
        use sha2::{Digest, Sha256};

        let digest = Sha256::digest(b"hello world");
        let hex = to_hex_lower(&digest);

        assert_eq!(
            hex, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
            "hex encoding of a known SHA-256 digest must be stable"
        );
    }

    #[test]
    fn test_to_hex_lower_empty_input() {
        assert_eq!(to_hex_lower(&[]), "");
    }

    #[test]
    fn test_to_hex_lower_is_lowercase_and_zero_padded() {
        assert_eq!(to_hex_lower(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
    }
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
    fn test_atomic_write_produces_complete_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.toml");

        // A larger payload to make a partial/truncated write observable.
        let payload = "x".repeat(100_000);
        atomic_write(&path, payload.as_bytes()).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content.len(), 100_000, "file must be written completely");
        assert_eq!(content, payload);
    }

    #[test]
    fn test_atomic_write_no_temp_files_remain_on_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.toml");

        atomic_write(&path, b"data").unwrap();

        assert!(path.exists(), "target file should exist");

        // No stray temp files (the unique per-writer temp is renamed/consumed).
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".fastskill-tmp-")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp files must remain after success"
        );
    }

    #[test]
    fn test_atomic_write_two_sequential_writes_both_succeed() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("skills.lock");

        atomic_write(&path, b"first writer content").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first writer content");

        atomic_write(&path, b"second writer content").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second writer content");
    }

    #[test]
    fn test_atomic_write_interrupted_leaves_original_unchanged() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("skills.lock");

        // Write initial content
        fs::write(&path, b"original content").unwrap();

        // Simulate an interrupted write: write to a sibling .tmp, then leave it
        // without renaming
        let tmp_path = path.with_extension("tmp");
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
}

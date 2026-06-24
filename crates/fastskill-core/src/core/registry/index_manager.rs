//! Atomic index update manager with file locking

use crate::core::registry_index::{
    get_skill_index_lock_path, get_skill_index_path, ScopedSkillName, VersionEntry,
};
use crate::core::service::ServiceError;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use tracing::{info, warn};

/// Index manager for atomic updates with file locking
pub struct IndexManager {
    /// Base path to the registry index directory
    registry_path: PathBuf,
    /// Maximum time to wait for file lock acquisition (default: 30 seconds)
    lock_timeout: Duration,
    /// Tracked file metadata for external modification detection (interior mutability)
    file_metadata: Mutex<std::collections::HashMap<PathBuf, IndexFileMetadata>>,
}

/// Metadata for tracking index file state (for external modification detection)
#[derive(Debug, Clone)]
struct IndexFileMetadata {
    /// File modification time
    mtime: SystemTime,
    /// File size in bytes
    size: u64,
}

/// Derive a per-call unique temp path next to the index file (same directory, so the
/// final rename stays atomic on one filesystem).
///
/// Unlike `utils::atomic_write`, whose temp name doubles as its advisory lock token and
/// so must be deterministic, the index writer holds its own sidecar lock across the whole
/// read-modify-write — so its temp file need not lock, and it MUST be unique: a
/// deterministic `org/data.tmp` could equal the real index file of skill `org/data.tmp`
/// and be renamed away. Tagging the name with the process id and a monotonic counter
/// makes that collision impossible in practice.
fn unique_temp_path(index_path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    crate::utils::append_suffix(index_path, &format!("{}.{}.tmp", std::process::id(), seq))
}

/// Returns true if `e` is an out-of-disk-space error (ENOSPC).
fn is_disk_full(e: &std::io::Error) -> bool {
    if e.raw_os_error() == Some(28) {
        return true;
    }
    let msg = e.to_string().to_lowercase();
    msg.contains("no space") || msg.contains("filesystem full")
}

impl IndexManager {
    /// Create a new IndexManager instance
    ///
    /// # Arguments
    /// * `registry_path` - Base path to the registry index directory
    ///
    /// # Returns
    /// Configured IndexManager instance with default 30-second lock timeout
    pub fn new(registry_path: PathBuf) -> Self {
        Self {
            registry_path,
            lock_timeout: Duration::from_secs(30),
            file_metadata: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Atomically update the index file for a skill version
    ///
    /// This method:
    /// 1. Normalizes the skill_id using ScopedSkillName::normalize()
    /// 2. Checks for duplicate versions
    /// 3. Acquires an exclusive file lock with timeout
    /// 4. Reads existing entries
    /// 5. Appends new entry
    /// 6. Writes to temporary file
    /// 7. Atomically renames temporary file to target
    /// 8. Releases lock
    ///
    /// # Arguments
    /// * `skill_id` - The skill identifier (may be scoped, e.g., `@org/package`)
    /// * `version` - The version string (e.g., `1.0.0`)
    /// * `entry` - The version entry to add to the index
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(ServiceError)` if operation fails
    ///
    /// # Errors
    /// - Returns `ServiceError::Custom` if duplicate version is detected
    /// - Returns `ServiceError::Custom` if lock timeout is exceeded
    /// - Returns `ServiceError::Io` for filesystem errors
    pub fn atomic_update(
        &self,
        skill_id: &str,
        version: &str,
        entry: &VersionEntry,
    ) -> Result<(), ServiceError> {
        // Step 1: Normalize scoped name
        let normalized_id = ScopedSkillName::normalize(skill_id);
        info!("Normalized skill_id '{}' to '{}'", skill_id, normalized_id);

        // Step 2: Get index file path
        let index_path = get_skill_index_path(&self.registry_path, &normalized_id)?;

        // Ensure parent directory exists
        if let Some(parent) = index_path.parent() {
            std::fs::create_dir_all(parent).map_err(ServiceError::Io)?;
        }

        // Step 3: Check for duplicate version (before acquiring lock)
        // Read existing entries if file exists
        let existing_entries_before_lock = if index_path.exists() {
            Self::read_entries_from_path(&index_path)?
        } else {
            Vec::new()
        };

        // Check if version already exists
        for existing_entry in &existing_entries_before_lock {
            if existing_entry.vers == version {
                return Err(ServiceError::Custom(format!(
                    "Version {} already exists for skill {}",
                    version, normalized_id
                )));
            }
        }

        // Step 4: Acquire exclusive lock with timeout. Use a sidecar lock file
        // because the index file is replaced by atomic rename during update.
        let lock_path = get_skill_index_lock_path(&self.registry_path, &normalized_id)?;
        let lock_start = Instant::now();
        let file = loop {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .map_err(ServiceError::Io)?;

            // Try to acquire exclusive lock
            match file.try_lock_exclusive() {
                Ok(()) => {
                    let elapsed = lock_start.elapsed();
                    if elapsed.as_millis() > 0 {
                        info!(
                            "Acquired lock for index file: {:?} (waited {}ms)",
                            index_path,
                            elapsed.as_millis()
                        );
                    } else {
                        info!("Acquired lock for index file: {:?}", index_path);
                    }
                    break file;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Lock is held by another process, wait and retry
                    let elapsed = lock_start.elapsed();
                    if elapsed >= self.lock_timeout {
                        warn!(
                            "Lock timeout exceeded for {:?} after {} seconds",
                            lock_path,
                            self.lock_timeout.as_secs()
                        );
                        return Err(ServiceError::Custom(format!(
                            "Timeout waiting for file lock on {:?} (exceeded {} seconds)",
                            lock_path,
                            self.lock_timeout.as_secs()
                        )));
                    }
                    // Log if we've been waiting a while
                    if elapsed.as_secs() > 0 && elapsed.as_secs().is_multiple_of(5) {
                        info!(
                            "Waiting for lock on {:?} ({}s elapsed, timeout: {}s)",
                            lock_path,
                            elapsed.as_secs(),
                            self.lock_timeout.as_secs()
                        );
                    }
                    // Wait a bit before retrying
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => {
                    warn!("Failed to acquire lock on {:?}: {}", lock_path, e);
                    return Err(ServiceError::Io(e));
                }
            }
        };

        // Lock will be released when file is dropped
        // Use a guard to ensure lock is released even on error
        struct LockGuard(File);
        impl Drop for LockGuard {
            fn drop(&mut self) {
                if let Err(e) = self.0.unlock() {
                    warn!("Failed to release file lock: {}", e);
                }
            }
        }
        let _lock_guard = LockGuard(file);

        // Step 5: Check for external modifications (before reading)
        if index_path.exists() {
            let file_metadata = self.file_metadata.lock().map_err(|_| {
                ServiceError::Custom(
                    "Mutex poisoned - another thread panicked while holding the lock".to_string(),
                )
            })?;
            if let Some(prev_metadata) = file_metadata.get(&index_path) {
                match std::fs::metadata(&index_path) {
                    Ok(current_metadata) => {
                        let current_mtime = current_metadata
                            .modified()
                            .unwrap_or(SystemTime::UNIX_EPOCH);
                        let current_size = current_metadata.len();

                        if current_mtime != prev_metadata.mtime
                            || current_size != prev_metadata.size
                        {
                            warn!(
                                "External modification detected for {:?}: mtime changed from {:?} to {:?}, size changed from {} to {}",
                                index_path, prev_metadata.mtime, current_mtime, prev_metadata.size, current_size
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read metadata for {:?}: {}", index_path, e);
                    }
                }
            }
            drop(file_metadata); // Release lock before reading
        }

        // Step 5: Read existing entries again (after acquiring lock)
        let mut existing_entries = if index_path.exists() {
            Self::read_entries_from_path(&index_path)?
        } else {
            Vec::new()
        };

        // Update tracked metadata after successful read
        if index_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&index_path) {
                if let Ok(mtime) = metadata.modified() {
                    let mut file_metadata = self.file_metadata.lock().map_err(|_| {
                        ServiceError::Custom(
                            "Mutex poisoned - another thread panicked while holding the lock"
                                .to_string(),
                        )
                    })?;
                    file_metadata.insert(
                        index_path.clone(),
                        IndexFileMetadata {
                            mtime,
                            size: metadata.len(),
                        },
                    );
                }
            }
        }

        // Double-check for duplicate (in case it was added between initial check and lock)
        for existing_entry in &existing_entries {
            if existing_entry.vers == version {
                warn!(
                    "Duplicate version {} detected for skill {} after acquiring lock",
                    version, normalized_id
                );
                return Err(ServiceError::Custom(format!(
                    "Version {} already exists for skill {} (detected after lock)",
                    version, normalized_id
                )));
            }
        }

        info!(
            "Updating index for {} v{} ({} existing entries)",
            normalized_id,
            version,
            existing_entries.len()
        );

        // Step 6: Append the new entry and serialize the whole index as
        // newline-delimited JSON.
        existing_entries.push(entry.clone());
        let mut buffer = String::new();
        for entry in &existing_entries {
            let line = serde_json::to_string(entry).map_err(|e| {
                ServiceError::Custom(format!("Failed to serialize index entry: {}", e))
            })?;
            buffer.push_str(&line);
            buffer.push('\n');
        }

        // Step 7: Durably replace the index file — write to a unique temp, fsync, then
        // atomically rename. A full filesystem only ever truncates the temp file, so the
        // existing index is preserved on ENOSPC.
        let temp_path = unique_temp_path(&index_path);
        let on_write_err = |e: std::io::Error| {
            let _ = std::fs::remove_file(&temp_path);
            if is_disk_full(&e) {
                warn!("Filesystem full: cannot update index for {normalized_id} v{version}");
                ServiceError::Custom(format!(
                    "Filesystem full: cannot update index for {normalized_id} v{version}. Existing index preserved."
                ))
            } else {
                ServiceError::Io(e)
            }
        };

        let mut temp_file = File::create(&temp_path).map_err(&on_write_err)?;
        temp_file
            .write_all(buffer.as_bytes())
            .map_err(&on_write_err)?;
        temp_file.sync_all().map_err(&on_write_err)?;
        drop(temp_file);

        // Step 8: Atomically rename the temp file into place.
        std::fs::rename(&temp_path, &index_path).map_err(|e| {
            warn!("Failed to rename temp file {temp_path:?} to {index_path:?}: {e}");
            let _ = std::fs::remove_file(&temp_path);
            ServiceError::Io(e)
        })?;

        info!(
            "Successfully updated index for {} v{} (total {} entries)",
            normalized_id,
            version,
            existing_entries.len()
        );

        Ok(())
    }

    /// Read version entries from an index file path
    /// Helper function to read directly from a file path (not using registry_path + skill_id)
    fn read_entries_from_path(index_path: &PathBuf) -> Result<Vec<VersionEntry>, ServiceError> {
        use std::fs;

        if !index_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(index_path).map_err(ServiceError::Io)?;
        let mut entries = Vec::new();

        // Parse line-by-line (newline-delimited JSON)
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<VersionEntry>(line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    // Log error but continue parsing other lines
                    warn!("Failed to parse index entry: {} (line: {})", e, line);
                }
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::registry_index::VersionEntry;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn entry_named(name: &str, version: &str) -> VersionEntry {
        VersionEntry {
            name: name.to_string(),
            vers: version.to_string(),
            deps: Vec::new(),
            cksum: format!("checksum-{version}"),
            features: HashMap::new(),
            yanked: false,
            links: None,
            download_url: format!("https://example.com/test-skill-{version}.zip"),
            published_at: "2024-01-01T00:00:00Z".to_string(),
            metadata: None,
            scoped_name: None,
        }
    }

    fn entry(version: &str) -> VersionEntry {
        entry_named("testorg/test-skill", version)
    }

    fn read_entries(registry_path: PathBuf) -> Vec<VersionEntry> {
        let index_path = get_skill_index_path(&registry_path, "testorg/test-skill").unwrap();
        IndexManager::read_entries_from_path(&index_path).unwrap()
    }

    #[test]
    fn atomic_update_preserves_existing_entries() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().to_path_buf();
        let manager = IndexManager::new(registry_path.clone());

        for i in 0..25 {
            let version = format!("1.0.{i}");
            let update = entry(&version);
            let result = manager.atomic_update("testorg/test-skill", &version, &update);
            assert!(
                result.is_ok(),
                "atomic_update failed for {version}: {result:?}"
            );
        }

        let entries = read_entries(registry_path);
        assert_eq!(entries.len(), 25);
    }

    #[test]
    fn atomic_update_serializes_concurrent_same_skill_writes() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().to_path_buf();
        let manager = Arc::new(IndexManager::new(registry_path.clone()));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let manager = Arc::clone(&manager);
                std::thread::spawn(move || {
                    let version = format!("1.0.{i}");
                    let update = entry(&version);
                    manager.atomic_update("testorg/test-skill", &version, &update)
                })
            })
            .collect();

        for handle in handles {
            let result = handle.join().unwrap();
            assert!(
                result.is_ok(),
                "concurrent atomic_update failed: {result:?}"
            );
        }

        let entries = read_entries(registry_path);
        assert_eq!(entries.len(), 10);
    }

    /// Regression: two skills whose package names differ only after a dot must NOT
    /// share a sidecar lock file. Before the fix, `with_extension("lock")` mapped both
    /// `org/web.scraper` and `org/web.crawler` onto `org/web.lock`.
    #[test]
    fn atomic_update_uses_distinct_lock_files_for_dotted_names() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().to_path_buf();
        let manager = IndexManager::new(registry_path.clone());

        manager
            .atomic_update(
                "org/web.scraper",
                "1.0.0",
                &entry_named("org/web.scraper", "1.0.0"),
            )
            .unwrap();
        manager
            .atomic_update(
                "org/web.crawler",
                "1.0.0",
                &entry_named("org/web.crawler", "1.0.0"),
            )
            .unwrap();

        // Each skill keeps its own sidecar lock (append, not replace-extension).
        assert!(
            registry_path.join("org/web.scraper.lock").exists(),
            "expected distinct lock for web.scraper"
        );
        assert!(
            registry_path.join("org/web.crawler.lock").exists(),
            "expected distinct lock for web.crawler"
        );
    }

    /// Regression: publishing a skill must not clobber another skill whose index file
    /// happens to match the first skill's temp path. Before the fix, writing `org/data`
    /// used temp `org/data.tmp` — the literal index file of skill `org/data.tmp` — and
    /// the atomic rename destroyed it.
    #[test]
    fn atomic_update_does_not_clobber_skill_named_like_temp_path() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().to_path_buf();
        let manager = IndexManager::new(registry_path.clone());

        manager
            .atomic_update(
                "org/data.tmp",
                "1.0.0",
                &entry_named("org/data.tmp", "1.0.0"),
            )
            .unwrap();
        manager
            .atomic_update("org/data", "1.0.0", &entry_named("org/data", "1.0.0"))
            .unwrap();

        let victim_index = get_skill_index_path(&registry_path, "org/data.tmp").unwrap();
        let victim = IndexManager::read_entries_from_path(&victim_index).unwrap();
        assert_eq!(
            victim.len(),
            1,
            "publishing org/data must not destroy the org/data.tmp index"
        );
        assert_eq!(victim[0].name, "org/data.tmp");
    }
}

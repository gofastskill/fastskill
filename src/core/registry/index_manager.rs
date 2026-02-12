//! Atomic index update manager with file locking

use crate::core::registry_index::{get_skill_index_path, ScopedSkillName, VersionEntry};
use crate::core::service::ServiceError;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
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

        // Step 4: Acquire exclusive file lock with timeout
        let lock_start = Instant::now();
        let file = loop {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&index_path)
                .map_err(ServiceError::Io)?;

            // Try to acquire exclusive lock
            match file.try_lock_exclusive() {
                Ok(()) => {
                    let elapsed = lock_start.elapsed();
                    if elapsed.as_millis() > 0 {
                        info!(
                            "Acquired lock on index file: {:?} (waited {}ms)",
                            index_path,
                            elapsed.as_millis()
                        );
                    } else {
                        info!("Acquired lock on index file: {:?}", index_path);
                    }
                    break file;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Lock is held by another process, wait and retry
                    let elapsed = lock_start.elapsed();
                    if elapsed >= self.lock_timeout {
                        warn!(
                            "Lock timeout exceeded for {:?} after {} seconds",
                            index_path,
                            self.lock_timeout.as_secs()
                        );
                        return Err(ServiceError::Custom(format!(
                            "Timeout waiting for file lock on {:?} (exceeded {} seconds)",
                            index_path,
                            self.lock_timeout.as_secs()
                        )));
                    }
                    // Log if we've been waiting a while
                    if elapsed.as_secs() > 0 && elapsed.as_secs().is_multiple_of(5) {
                        info!(
                            "Waiting for lock on {:?} ({}s elapsed, timeout: {}s)",
                            index_path,
                            elapsed.as_secs(),
                            self.lock_timeout.as_secs()
                        );
                    }
                    // Wait a bit before retrying
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => {
                    warn!("Failed to acquire lock on {:?}: {}", index_path, e);
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

        // Step 6: Append new entry
        existing_entries.push(entry.clone());

        // Step 7: Write to temporary file
        let temp_path = index_path.with_extension("tmp");
        let mut temp_file = match File::create(&temp_path) {
            Ok(file) => file,
            Err(e) => {
                // Check if error is due to filesystem being full
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("no space")
                    || error_msg.contains("filesystem full")
                    || e.raw_os_error() == Some(28)
                {
                    warn!(
                        "Filesystem full: cannot write index file for {} v{}",
                        normalized_id, version
                    );
                    return Err(ServiceError::Custom(format!(
                        "Filesystem full: cannot update index for {} v{}. Existing index preserved.",
                        normalized_id, version
                    )));
                }
                return Err(ServiceError::Io(e));
            }
        };

        // Write all entries as newline-delimited JSON
        for entry in &existing_entries {
            let line = serde_json::to_string(entry).map_err(|e| {
                ServiceError::Custom(format!("Failed to serialize index entry: {}", e))
            })?;
            if let Err(e) = writeln!(temp_file, "{}", line) {
                // Check if error is due to filesystem being full
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("no space")
                    || error_msg.contains("filesystem full")
                    || e.raw_os_error() == Some(28)
                {
                    warn!(
                        "Filesystem full: cannot write index entry for {} v{}",
                        normalized_id, version
                    );
                    // Clean up temp file
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(ServiceError::Custom(format!(
                        "Filesystem full: cannot update index for {} v{}. Existing index preserved.",
                        normalized_id, version
                    )));
                }
                return Err(ServiceError::Io(e));
            }
        }

        if let Err(e) = temp_file.sync_all() {
            // Check if error is due to filesystem being full
            let error_msg = e.to_string().to_lowercase();
            if error_msg.contains("no space")
                || error_msg.contains("filesystem full")
                || e.raw_os_error() == Some(28)
            {
                warn!(
                    "Filesystem full: cannot sync index file for {} v{}",
                    normalized_id, version
                );
                // Clean up temp file
                let _ = std::fs::remove_file(&temp_path);
                return Err(ServiceError::Custom(format!(
                    "Filesystem full: cannot update index for {} v{}. Existing index preserved.",
                    normalized_id, version
                )));
            }
            return Err(ServiceError::Io(e));
        }
        drop(temp_file);

        // Step 8: Atomically rename temporary file to target
        std::fs::rename(&temp_path, &index_path).map_err(|e| {
            warn!(
                "Failed to atomically rename temp file {:?} to {:?}: {}",
                temp_path, index_path, e
            );
            // If rename fails, try to clean up temp file
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

//! The quarantine: where blocked, unlisted-but-modified or disallowed content is moved instead
//! of being deleted or overwritten (ADR-0016 decisions 10, 11 and 17).
//!
//! Each item is a timestamped folder holding the moved content under `content/` and a
//! `record.json` with its digest, former location and reason. Unenrolling keeps it.

use super::layout::{create_private_dir, read_record, write_record, ManagedLayout};
use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file that describes one quarantined item.
pub const RECORD_FILE: &str = "record.json";
/// The folder that holds the moved content.
pub const CONTENT_DIR: &str = "content";

/// Why content was quarantined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuarantineReason {
    /// Its digest is blocked.
    Blocked,
    /// `allowed = listed` with `exclusive_targets`, and its digest isn't allowed.
    NotAllowed,
    /// A managed copy that was changed after FastSkill placed it.
    Modified,
}

/// One quarantined item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineRecord {
    /// The content's digest when it was moved, when it could be computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub former_path: PathBuf,
    pub reason: QuarantineReason,
    pub quarantined_at: DateTime<Utc>,
    /// Where the content is now. Filled when listing, not stored.
    #[serde(skip)]
    pub path: PathBuf,
}

impl Default for QuarantineRecord {
    fn default() -> Self {
        Self {
            digest: None,
            former_path: PathBuf::new(),
            reason: QuarantineReason::Blocked,
            quarantined_at: DateTime::<Utc>::MIN_UTC,
            path: PathBuf::new(),
        }
    }
}

/// Move `path` into the quarantine and record why. A link is moved as a link, never followed.
pub fn quarantine(
    layout: &ManagedLayout,
    path: &Path,
    digest: Option<&str>,
    reason: QuarantineReason,
    now: DateTime<Utc>,
) -> Result<QuarantineRecord, ServiceError> {
    let root = layout.quarantine();
    create_private_dir(&root)?;
    let stamp = now.format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let item = (0u32..1000)
        .map(|n| root.join(format!("{stamp}-{n}")))
        .find(|candidate| candidate.symlink_metadata().is_err())
        .ok_or_else(|| {
            ServiceError::Storage(format!(
                "no free quarantine folder name for {stamp} in {}",
                root.display()
            ))
        })?;
    create_private_dir(&item)?;
    let content = item.join(CONTENT_DIR);
    move_entry(path, &content)?;
    let record = QuarantineRecord {
        digest: digest.map(str::to_string),
        former_path: path.to_path_buf(),
        reason,
        quarantined_at: now,
        path: content,
    };
    write_record(&item.join(RECORD_FILE), &record)?;
    Ok(record)
}

/// Every quarantined item, oldest first. Items without a readable record are skipped.
pub fn list(layout: &ManagedLayout) -> Result<Vec<QuarantineRecord>, ServiceError> {
    let root = layout.quarantine();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let item = entry?.path();
        let Ok(mut record) = read_record::<QuarantineRecord>(&item.join(RECORD_FILE)) else {
            continue;
        };
        if record.former_path.as_os_str().is_empty() {
            continue;
        }
        record.path = item.join(CONTENT_DIR);
        items.push(record);
    }
    items.sort_by(|a, b| {
        a.quarantined_at
            .cmp(&b.quarantined_at)
            .then(a.path.cmp(&b.path))
    });
    Ok(items)
}

/// Move `from` to `to` by rename, so content is never half moved. The quarantine is in this
/// user's data folder; content on another file system is reported rather than copied.
fn move_entry(from: &Path, to: &Path) -> Result<(), ServiceError> {
    std::fs::rename(from, to).map_err(|error| {
        ServiceError::Storage(format!(
            "can't move {} into the quarantine: {error}",
            from.display()
        ))
    })
}

/// Remove a file, a link or a folder, never following a link.
pub(crate) fn remove_entry(path: &Path) -> Result<(), ServiceError> {
    let metadata = path.symlink_metadata()?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        #[cfg(windows)]
        if metadata.file_type().is_symlink() {
            // A directory link on Windows is removed with remove_dir.
            if std::fs::remove_dir(path).is_ok() {
                return Ok(());
            }
        }
        std::fs::remove_file(path)?;
    }
    Ok(())
}

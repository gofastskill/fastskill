//! The per-user records an apply reads and writes: enrollment, ownership and the last apply
//! (ADR-0016 decisions 3, 9, 12 and 17).

use super::state::Recorded;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What enrolling recorded for this user.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enrollment {
    /// The managed source enrolled with, as configured then.
    pub source: String,
    /// A random id for this user's reports, never derived from hardware (decision 15).
    pub machine_id: String,
    pub enrolled_at: Option<DateTime<Utc>>,
    /// The sequence number of the last report built for this machine id (decision 15).
    #[serde(default)]
    pub report_sequence: u64,
    #[serde(flatten)]
    pub recorded: Recorded,
}

impl Enrollment {
    /// A new enrollment with a fresh machine id and nothing accepted yet.
    pub fn new(source: &str, now: DateTime<Utc>) -> Self {
        Self {
            source: source.to_string(),
            machine_id: uuid::Uuid::new_v4().to_string(),
            enrolled_at: Some(now),
            report_sequence: 0,
            recorded: Recorded::default(),
        }
    }

    /// Whether this record describes an enrollment.
    pub fn is_enrolled(&self) -> bool {
        !self.machine_id.is_empty()
    }
}

/// How a target entry was placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryMode {
    /// A link into the managed store.
    Link,
    /// A verified copy.
    Copy,
    /// Content that was already there with the listed digest.
    Adopted,
}

/// One target entry FastSkill created or adopted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedEntry {
    /// The Agent target folder.
    pub target: PathBuf,
    pub id: String,
    pub digest: String,
    pub mode: EntryMode,
}

impl OwnedEntry {
    pub fn path(&self) -> PathBuf {
        self.target.join(&self.id)
    }
}

/// Every target entry FastSkill owns. It replaces or removes only these.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ownership {
    #[serde(default)]
    pub entries: Vec<OwnedEntry>,
}

impl Ownership {
    pub fn find(&self, target: &Path, id: &str) -> Option<&OwnedEntry> {
        self.entries
            .iter()
            .find(|entry| entry.target == target && entry.id == id)
    }

    /// Record `entry`, replacing any record of the same target and id.
    pub fn record(&mut self, entry: OwnedEntry) {
        self.forget(&entry.target.clone(), &entry.id.clone());
        self.entries.push(entry);
    }

    pub fn forget(&mut self, target: &Path, id: &str) {
        self.entries
            .retain(|entry| !(entry.target == target && entry.id == id));
    }
}

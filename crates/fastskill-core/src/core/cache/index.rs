//! The on-disk index-cache schema (PRD 006, US-001 — "this story owns the
//! index schema"). These serde types, and the read/write functions below,
//! are the **only** way any code should touch `<cache-root>/index/*.json`.
//! Later stories read/write through here rather than hand-rolling JSON at
//! their own call sites:
//!
//! - US-003/US-005 read and write [`SourceIndex`] — what a source currently
//!   advertises (skills + versions), one file per source.
//! - US-002 reads and writes [`GitResolutions`] — the `url+ref -> sha` map
//!   that lets an offline install proceed from a previously-resolved ref.
//! - Spec 007 (zip-url caching) reads and writes [`ZipValidators`] — the
//!   `url -> {etag/last_modified, content_hash, fetched_at}` map that backs
//!   the conditional-request staleness check for `Origin::ZipUrl`.

use crate::core::cache::validate_component;
use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const INDEX_DIR_NAME: &str = "index";
const GIT_RESOLUTIONS_FILE_NAME: &str = "git-resolutions.json";
const ZIP_VALIDATORS_FILE_NAME: &str = "zip-validators.json";

/// `<cache-root>/index/<source>.json` — what a single configured source
/// currently advertises: which skills, at which versions. Written by a real
/// `repos refresh` (US-005); read by version resolution (US-002/US-003).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceIndex {
    pub fetched_at: DateTime<Utc>,
    #[serde(default)]
    pub entries: Vec<SourceIndexEntry>,
}

/// One skill's advertised versions within a [`SourceIndex`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceIndexEntry {
    pub skill: String,
    #[serde(default)]
    pub versions: Vec<String>,
}

/// A single git ref resolution: the commit SHA `url+ref` resolved to, and
/// when. Stored inside [`GitResolutions`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitResolution {
    pub sha: String,
    pub resolved_at: DateTime<Utc>,
}

/// `<cache-root>/index/git-resolutions.json` — `url+ref -> {sha,
/// resolved_at}`. A newtype over the map (rather than a bare
/// `HashMap<String, GitResolution>` alias) so the `url+ref` key composition
/// lives in exactly one place instead of being reformatted ad hoc at each
/// call site. Serialized `#[serde(transparent)]`, so the file on disk is
/// just the flat JSON object the PRD specifies — no extra wrapper field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitResolutions(HashMap<String, GitResolution>);

impl GitResolutions {
    fn key(url: &str, git_ref: &str) -> String {
        format!("{url}#{git_ref}")
    }

    /// Canonical ref-key encoding for a plain branch/tag/default selection —
    /// shared between `install::fetch_git` (PRD 006 US-002, resolving a
    /// skill's own git origin) and `RepositoryManager::refresh_index` (US-005,
    /// resolving a configured `GitMarketplace` source's branch/tag) so both
    /// read and write compatible entries in this map. A `GitRef::Commit` has
    /// no branch/tag form and is encoded separately by its only caller.
    pub fn branch_or_tag_key(branch: Option<&str>, tag: Option<&str>) -> String {
        match (branch, tag) {
            (Some(b), _) => format!("branch:{b}"),
            (None, Some(t)) => format!("tag:{t}"),
            (None, None) => "HEAD".to_string(),
        }
    }

    /// Look up a previously-recorded resolution for `url` at `git_ref`.
    pub fn get(&self, url: &str, git_ref: &str) -> Option<&GitResolution> {
        self.0.get(&Self::key(url, git_ref))
    }

    /// Record (or replace) the resolution for `url` at `git_ref`.
    pub fn insert(&mut self, url: &str, git_ref: &str, sha: String, resolved_at: DateTime<Utc>) {
        self.0
            .insert(Self::key(url, git_ref), GitResolution { sha, resolved_at });
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A single zip-URL fetch's recorded HTTP validators, plus the content hash
/// they produced (spec 007 FR-2/FR-3). `etag`/`last_modified` are each
/// optional because a server may supply either, both, or neither — with
/// neither, only the "download-then-hash" dedup fallback applies (no
/// fetch-time saving, no offline check; see spec 007's design notes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZipValidator {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    pub content_hash: String,
    pub fetched_at: DateTime<Utc>,
}

/// `<cache-root>/index/zip-validators.json` — `url -> ZipValidator` (spec
/// 007 FR-2). A newtype over the map (rather than a bare `HashMap<String,
/// ZipValidator>` alias), mirroring [`GitResolutions`], so the flat JSON
/// object the spec calls for lives at exactly one call site. Serialized
/// `#[serde(transparent)]` for the same reason.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ZipValidators(HashMap<String, ZipValidator>);

impl ZipValidators {
    /// Look up a previously-recorded validator for `url`.
    pub fn get(&self, url: &str) -> Option<&ZipValidator> {
        self.0.get(url)
    }

    /// Record (or replace) the validator for `url`.
    pub fn insert(&mut self, url: &str, validator: ZipValidator) {
        self.0.insert(url.to_string(), validator);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn index_dir(root: &Path) -> PathBuf {
    root.join(INDEX_DIR_NAME)
}

fn source_index_path(root: &Path, source: &str) -> Result<PathBuf, ServiceError> {
    let source = validate_component(source)?;
    Ok(index_dir(root).join(format!("{source}.json")))
}

pub(super) fn read_source_index(
    root: &Path,
    source: &str,
) -> Result<Option<SourceIndex>, ServiceError> {
    read_json(&source_index_path(root, source)?)
}

pub(super) fn write_source_index(
    root: &Path,
    source: &str,
    idx: &SourceIndex,
) -> Result<(), ServiceError> {
    write_json(&source_index_path(root, source)?, idx)
}

pub(super) fn read_git_resolutions(root: &Path) -> Result<GitResolutions, ServiceError> {
    let path = index_dir(root).join(GIT_RESOLUTIONS_FILE_NAME);
    Ok(read_json(&path)?.unwrap_or_default())
}

pub(super) fn write_git_resolutions(
    root: &Path,
    resolutions: &GitResolutions,
) -> Result<(), ServiceError> {
    let path = index_dir(root).join(GIT_RESOLUTIONS_FILE_NAME);
    write_json(&path, resolutions)
}

pub(super) fn read_zip_validators(root: &Path) -> Result<ZipValidators, ServiceError> {
    let path = index_dir(root).join(ZIP_VALIDATORS_FILE_NAME);
    Ok(read_json(&path)?.unwrap_or_default())
}

pub(super) fn write_zip_validators(
    root: &Path,
    validators: &ZipValidators,
) -> Result<(), ServiceError> {
    let path = index_dir(root).join(ZIP_VALIDATORS_FILE_NAME);
    write_json(&path, validators)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, ServiceError> {
    if !path.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    let value = serde_json::from_str(&content).map_err(|e| {
        ServiceError::Validation(format!("failed to parse {}: {e}", path.display()))
    })?;
    Ok(Some(value))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ServiceError> {
    let json = serde_json::to_vec_pretty(value).map_err(|e| {
        ServiceError::Validation(format!("failed to serialize {}: {e}", path.display()))
    })?;
    crate::utils::atomic_write(path, &json)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn source_index_round_trips_through_disk() {
        let root = TempDir::new().unwrap();
        let idx = SourceIndex {
            fetched_at: Utc::now(),
            entries: vec![SourceIndexEntry {
                skill: "demo".to_string(),
                versions: vec!["1.0.0".to_string(), "1.1.0".to_string()],
            }],
        };

        write_source_index(root.path(), "acme", &idx).unwrap();
        let read_back = read_source_index(root.path(), "acme").unwrap();

        assert_eq!(read_back, Some(idx));
    }

    #[test]
    fn source_index_missing_file_is_none() {
        let root = TempDir::new().unwrap();
        assert_eq!(read_source_index(root.path(), "nope").unwrap(), None);
    }

    #[test]
    fn source_index_rejects_invalid_source_name() {
        let root = TempDir::new().unwrap();
        let idx = SourceIndex {
            fetched_at: Utc::now(),
            entries: vec![],
        };

        let err = write_source_index(root.path(), "../escape", &idx).unwrap_err();
        assert!(matches!(err, ServiceError::Validation(_)));
    }

    #[test]
    fn git_resolutions_round_trip_and_key_on_url_plus_ref() {
        let root = TempDir::new().unwrap();
        let mut resolutions = GitResolutions::default();
        let now = Utc::now();
        resolutions.insert(
            "https://example.com/repo.git",
            "main",
            "abc123".to_string(),
            now,
        );
        resolutions.insert(
            "https://example.com/repo.git",
            "v1.0",
            "def456".to_string(),
            now,
        );

        write_git_resolutions(root.path(), &resolutions).unwrap();
        let read_back = read_git_resolutions(root.path()).unwrap();

        assert_eq!(
            read_back
                .get("https://example.com/repo.git", "main")
                .map(|r| r.sha.as_str()),
            Some("abc123")
        );
        assert_eq!(
            read_back
                .get("https://example.com/repo.git", "v1.0")
                .map(|r| r.sha.as_str()),
            Some("def456")
        );
        assert_eq!(read_back.len(), 2);
    }

    #[test]
    fn git_resolutions_missing_file_is_empty() {
        let root = TempDir::new().unwrap();
        let resolutions = read_git_resolutions(root.path()).unwrap();
        assert!(resolutions.is_empty());
    }

    #[test]
    fn zip_validators_round_trip_and_key_on_url() {
        let root = TempDir::new().unwrap();
        let mut validators = ZipValidators::default();
        let now = Utc::now();
        validators.insert(
            "https://example.com/pkg.zip",
            ZipValidator {
                etag: Some("\"abc123\"".to_string()),
                last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
                content_hash: "deadbeef".to_string(),
                fetched_at: now,
            },
        );
        validators.insert(
            "https://example.com/no-validators.zip",
            ZipValidator {
                etag: None,
                last_modified: None,
                content_hash: "cafef00d".to_string(),
                fetched_at: now,
            },
        );

        write_zip_validators(root.path(), &validators).unwrap();
        let read_back = read_zip_validators(root.path()).unwrap();

        let with_validators = read_back.get("https://example.com/pkg.zip").unwrap();
        assert_eq!(with_validators.etag.as_deref(), Some("\"abc123\""));
        assert_eq!(
            with_validators.last_modified.as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
        assert_eq!(with_validators.content_hash, "deadbeef");

        let without_validators = read_back
            .get("https://example.com/no-validators.zip")
            .unwrap();
        assert_eq!(without_validators.etag, None);
        assert_eq!(without_validators.last_modified, None);
        assert_eq!(without_validators.content_hash, "cafef00d");

        assert_eq!(read_back.len(), 2);
    }

    #[test]
    fn zip_validators_missing_file_is_empty() {
        let root = TempDir::new().unwrap();
        let validators = read_zip_validators(root.path()).unwrap();
        assert!(validators.is_empty());
    }
}

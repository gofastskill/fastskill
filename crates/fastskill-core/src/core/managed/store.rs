//! The user-level managed store: skills by digest, published only after their content and
//! identity check out (ADR-0016 decision 9).

use super::config::ManagedSource;
use super::layout::{create_private_dir, ManagedLayout};
use super::remote::Remote;
use super::state::StateSkill;
use crate::core::content_digest::{content_digest, CONTENT_DIGEST_PREFIX};
use crate::core::service::ServiceError;
use std::path::{Path, PathBuf};

/// The store under one layout.
#[derive(Debug, Clone)]
pub struct ManagedStore {
    layout: ManagedLayout,
}

impl ManagedStore {
    pub fn new(layout: &ManagedLayout) -> Self {
        Self {
            layout: layout.clone(),
        }
    }

    /// Where the skill with `digest` is published. Digests are checked before they get here, so
    /// the folder name is always `v2-` and 64 hex characters.
    pub fn path(&self, digest: &str) -> PathBuf {
        let hex = digest.strip_prefix(CONTENT_DIGEST_PREFIX).unwrap_or(digest);
        self.layout.store().join(format!("v2-{hex}"))
    }

    /// The published entry for `digest`, re-verified. An entry whose content no longer matches
    /// is removed so it's fetched again.
    pub fn verified(&self, digest: &str) -> Result<Option<PathBuf>, ServiceError> {
        let path = self.path(digest);
        if !path.is_dir() {
            return Ok(None);
        }
        if content_digest(&path).ok().as_deref() == Some(digest) {
            return Ok(Some(path));
        }
        tracing::warn!(
            "managed store entry {} no longer matches its digest; fetching it again",
            path.display()
        );
        std::fs::remove_dir_all(&path)?;
        Ok(None)
    }

    /// Make `skill` available in the store, fetching it from its artifact when it isn't there.
    /// An `https://` artifact is downloaded into private staging first.
    pub fn ensure(
        &self,
        skill: &StateSkill,
        source: &ManagedSource,
        remote: Option<&Remote>,
    ) -> Result<PathBuf, ServiceError> {
        if let Some(path) = self.verified(&skill.digest)? {
            return Ok(path);
        }
        if !skill.artifact.contains("://") {
            let archive = artifact_path(&skill.artifact, source)?;
            return self.publish_archive(skill, &archive);
        }
        let remote = remote.ok_or_else(|| {
            ServiceError::InvalidOperation(format!(
                "can't fetch the artifact {:?}: https isn't available",
                skill.artifact
            ))
        })?;
        let staging = self.layout.staging();
        create_private_dir(&staging)?;
        let download = tempfile::Builder::new()
            .prefix("download-")
            .suffix(".zip")
            .tempfile_in(&staging)?;
        remote.download(&skill.artifact, download.path())?;
        self.publish_archive(skill, download.path())
    }

    /// Extract `archive` into private staging, check its digest and identity, then rename it
    /// into the store.
    pub fn publish_archive(
        &self,
        skill: &StateSkill,
        archive: &Path,
    ) -> Result<PathBuf, ServiceError> {
        let staging = self.layout.staging();
        create_private_dir(&staging)?;
        create_private_dir(&self.layout.store())?;
        let work = tempfile::Builder::new()
            .prefix("apply-")
            .tempdir_in(&staging)?;
        crate::storage::zip::ZipHandler::new()?.extract_to_dir(archive, work.path())?;
        let root = crate::storage::git::validate_cloned_skill(work.path())?;
        let digest = content_digest(&root)?;
        if digest != skill.digest {
            return Err(ServiceError::Validation(format!(
                "the artifact of {:?} has digest {digest}, but the managed state lists {}",
                skill.id, skill.digest
            )));
        }
        let (id, _) = crate::core::install::read_skill_identity(&root)?;
        if id.as_str() != skill.id {
            return Err(ServiceError::Validation(format!(
                "the artifact listed as {:?} is the skill {:?}",
                skill.id,
                id.as_str()
            )));
        }
        let path = self.path(&skill.digest);
        if path.is_dir() {
            // Published by an earlier, interrupted run; it was just re-verified or removed.
            return Ok(path);
        }
        std::fs::rename(&root, &path)?;
        Ok(path)
    }

    /// Remove every published entry whose digest isn't in `keep`.
    pub fn retain(&self, keep: &[&str]) -> Result<(), ServiceError> {
        let store = self.layout.store();
        if !store.is_dir() {
            return Ok(());
        }
        let keep: Vec<PathBuf> = keep.iter().map(|digest| self.path(digest)).collect();
        for entry in std::fs::read_dir(&store)? {
            let path = entry?.path();
            if !keep.contains(&path) {
                std::fs::remove_dir_all(&path)?;
            }
        }
        Ok(())
    }
}

/// Where a local artifact is: an absolute path, or one relative to the state file's folder.
/// Only a file source names local artifacts.
pub fn artifact_path(artifact: &str, source: &ManagedSource) -> Result<PathBuf, ServiceError> {
    match source {
        ManagedSource::File(state_file) if !artifact.contains("://") => {
            let path = Path::new(artifact);
            Ok(match state_file.parent() {
                Some(folder) if !path.is_absolute() => folder.join(path),
                _ => path.to_path_buf(),
            })
        }
        _ => Err(ServiceError::InvalidOperation(format!(
            "the artifact {artifact:?} isn't an https:// URL"
        ))),
    }
}

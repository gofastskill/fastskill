//! Where managed data lives for one user, and the per-user apply lock (ADR-0016 decisions 9,
//! 11 and 12).
//!
//! ```text
//! <data>/managed/
//!   lock              held while an apply runs
//!   enrollment.json   source, recorded subject and issued_at, machine id
//!   state.dsse        the last accepted envelope
//!   ownership.json    every target entry FastSkill created
//!   last-apply.json   what the last apply did
//!   refusals.json     refused installs not yet reported, with refusals.lock
//!   staging/          downloads and extraction, private to the user
//!   store/<digest>/   published skills, never modified after publication
//! <data>/quarantine/<time>-<n>/
//! ```

use crate::core::service::ServiceError;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// The folders and files managed data uses, under one root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedLayout {
    root: PathBuf,
}

impl ManagedLayout {
    /// The layout under `root`, FastSkill's per-user data folder.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The layout in this user's platform data folder.
    pub fn for_current_user() -> Result<Self, ServiceError> {
        dirs::data_local_dir()
            .map(|dir| Self::new(dir.join("fastskill")))
            .ok_or_else(|| {
                ServiceError::Config("can't determine this user's data folder".to_string())
            })
    }

    pub fn managed(&self) -> PathBuf {
        self.root.join("managed")
    }
    pub fn lock_file(&self) -> PathBuf {
        self.managed().join("lock")
    }
    pub fn enrollment_file(&self) -> PathBuf {
        self.managed().join("enrollment.json")
    }
    pub fn cached_state(&self) -> PathBuf {
        self.managed().join("state.dsse")
    }
    pub fn ownership_file(&self) -> PathBuf {
        self.managed().join("ownership.json")
    }
    pub fn last_apply_file(&self) -> PathBuf {
        self.managed().join("last-apply.json")
    }
    pub fn refusals_file(&self) -> PathBuf {
        self.managed().join("refusals.json")
    }
    pub fn refusals_lock(&self) -> PathBuf {
        self.managed().join("refusals.lock")
    }
    pub fn staging(&self) -> PathBuf {
        self.managed().join("staging")
    }
    pub fn store(&self) -> PathBuf {
        self.managed().join("store")
    }
    pub fn quarantine(&self) -> PathBuf {
        self.root.join("quarantine")
    }

    /// Take the per-user apply lock, or `None` when another apply holds it.
    pub fn try_lock(&self) -> Result<Option<ApplyLock>, ServiceError> {
        create_private_dir(&self.managed())?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.lock_file())?;
        Ok(file
            .try_lock_exclusive()
            .ok()
            .map(|()| ApplyLock { _file: file }))
    }
}

/// Held while an apply runs; released on drop.
#[derive(Debug)]
pub struct ApplyLock {
    _file: File,
}

/// Create `path` and its parents, the last one accessible only to this user.
pub(crate) fn create_private_dir(path: &Path) -> Result<(), ServiceError> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Read a JSON record, or its default when the file doesn't exist.
pub(crate) fn read_record<T: serde::de::DeserializeOwned + Default>(
    path: &Path,
) -> Result<T, ServiceError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            ServiceError::Validation(format!("{} is damaged: {error}", path.display()))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

/// Write a JSON record atomically.
pub(crate) fn write_record<T: serde::Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), ServiceError> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| ServiceError::Custom(error.to_string()))?;
    bytes.push(b'\n');
    crate::utils::atomic_write(path, &bytes)?;
    Ok(())
}

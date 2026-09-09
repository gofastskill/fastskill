//! Coordination and interrupted-mutation detection for project state writers.

use crate::core::service::ServiceError;
use crate::utils::atomic_write;
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

const STATE_DIRECTORY: &str = ".fastskill";
const LOCK_FILE: &str = "state.lock";
const RECOVERY_MARKER: &str = "recovery-required";
const DESTINATION_RECOVERY_MARKER: &str = ".fastskill-recovery-required";

/// Exclusive writer lease. Dropping an active lease intentionally leaves the
/// marker behind so a process interruption is visible to the next writer.
pub struct StateMutationGuard {
    locks: Vec<File>,
    lock_paths: Vec<PathBuf>,
    markers: Vec<PathBuf>,
    active: bool,
}

impl StateMutationGuard {
    pub fn acquire(project_root: &Path, operation: &str) -> Result<Self, ServiceError> {
        Self::acquire_for(project_root, None, operation)
    }

    /// Coordinate both the owning state and an optional shared destination.
    /// Lock paths are sorted to prevent deadlocks between writers.
    pub fn acquire_for(
        project_root: &Path,
        destination: Option<&Path>,
        operation: &str,
    ) -> Result<Self, ServiceError> {
        let state_directory = project_root.join(STATE_DIRECTORY);
        fs::create_dir_all(&state_directory).map_err(ServiceError::Io)?;
        let lock_path = state_directory.join(LOCK_FILE);
        let mut lock_paths = vec![lock_path];
        let mut markers = vec![state_directory.join(RECOVERY_MARKER)];
        if let Some(destination) = destination {
            fs::create_dir_all(destination).map_err(ServiceError::Io)?;
            lock_paths.push(destination.join(".fastskill-state.lock"));
            markers.push(destination.join(DESTINATION_RECOVERY_MARKER));
        }
        lock_paths.sort();
        lock_paths.dedup();
        markers.sort();
        markers.dedup();
        let mut locks = Vec::with_capacity(lock_paths.len());
        for path in &lock_paths {
            let lock = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(path)
                .map_err(ServiceError::Io)?;
            lock.try_lock_exclusive().map_err(|_| {
                ServiceError::InvalidOperation(format!(
                    "Another FastSkill writer is changing managed state ({})",
                    path.display()
                ))
            })?;
            locks.push(lock);
        }
        let lock_paths = lock_paths
            .into_iter()
            .map(|path| fs::canonicalize(&path).unwrap_or(path))
            .collect();
        if let Some(marker) = markers.iter().find(|marker| marker.exists()) {
            return Err(ServiceError::InvalidOperation(format!(
                "A previous FastSkill mutation was interrupted; inspect '{}' before changing managed state",
                marker.display()
            )));
        }
        let marker_content = format!(
            "operation = {operation:?}\nproject = {:?}\ndestination = {:?}\n",
            project_root, destination
        );
        let mut written_markers = Vec::new();
        for marker in &markers {
            if let Err(error) = atomic_write(marker, marker_content.as_bytes()) {
                for written in written_markers {
                    let _ = fs::remove_file(written);
                }
                return Err(ServiceError::Io(error));
            }
            written_markers.push(marker);
        }
        Ok(Self {
            locks,
            lock_paths,
            markers,
            active: true,
        })
    }

    /// Verify that a caller-owned lease coordinates the requested state and destination.
    pub fn ensure_covers(
        &self,
        project_root: &Path,
        destination: Option<&Path>,
    ) -> Result<(), ServiceError> {
        let mut required = vec![project_root.join(STATE_DIRECTORY).join(LOCK_FILE)];
        if let Some(destination) = destination {
            required.push(destination.join(".fastskill-state.lock"));
        }
        if required.into_iter().all(|path| {
            let canonical = fs::canonicalize(&path).unwrap_or(path);
            self.lock_paths.contains(&canonical)
        }) {
            Ok(())
        } else {
            Err(ServiceError::InvalidOperation(
                "The supplied writer lease does not cover this project and destination".to_string(),
            ))
        }
    }

    /// Finish after the state and files have committed successfully.
    pub fn commit(mut self) -> Result<(), ServiceError> {
        self.clear_marker()?;
        self.active = false;
        Ok(())
    }

    /// Finish after the caller has restored every captured value.
    pub fn recovered(mut self) -> Result<(), ServiceError> {
        self.clear_marker()?;
        self.active = false;
        Ok(())
    }

    fn clear_marker(&self) -> Result<(), ServiceError> {
        for marker in &self.markers {
            match fs::remove_file(marker) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(ServiceError::Io(error)),
            }
        }
        Ok(())
    }
}

impl Drop for StateMutationGuard {
    fn drop(&mut self) {
        if !self.active {
            for lock in &self.locks {
                let _ = FileExt::unlock(lock);
            }
        }
    }
}

/// Either an operation-owned guard or a guard retained by a larger lifecycle transaction.
pub(crate) enum StateMutationLease<'a> {
    Owned(StateMutationGuard),
    Borrowed { _guard: &'a StateMutationGuard },
}

impl<'a> StateMutationLease<'a> {
    pub(crate) fn acquire_or_borrow(
        project_root: &Path,
        destination: Option<&Path>,
        operation: &str,
        existing: Option<&'a StateMutationGuard>,
    ) -> Result<Self, ServiceError> {
        if let Some(existing) = existing {
            existing.ensure_covers(project_root, destination)?;
            Ok(Self::Borrowed { _guard: existing })
        } else {
            StateMutationGuard::acquire_for(project_root, destination, operation).map(Self::Owned)
        }
    }

    pub(crate) fn recovered(self) -> Result<(), ServiceError> {
        match self {
            Self::Owned(guard) => guard.recovered(),
            Self::Borrowed { .. } => Ok(()),
        }
    }

    pub(crate) fn commit(self) -> Result<(), ServiceError> {
        match self {
            Self::Owned(guard) => guard.commit(),
            Self::Borrowed { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn committed_and_recovered_operations_clear_the_marker() {
        let root = TempDir::new().unwrap();
        let marker = root.path().join(STATE_DIRECTORY).join(RECOVERY_MARKER);
        StateMutationGuard::acquire(root.path(), "install")
            .unwrap()
            .commit()
            .unwrap();
        assert!(!marker.exists());
        StateMutationGuard::acquire(root.path(), "remove")
            .unwrap()
            .recovered()
            .unwrap();
        assert!(!marker.exists());
    }

    #[test]
    fn dropped_operation_blocks_the_next_writer() {
        let root = TempDir::new().unwrap();
        drop(StateMutationGuard::acquire(root.path(), "bundle update").unwrap());
        let error = StateMutationGuard::acquire(root.path(), "remove")
            .err()
            .expect("interrupted marker must block the next writer");
        assert!(error.to_string().contains("was interrupted"));
        assert!(root
            .path()
            .join(STATE_DIRECTORY)
            .join(RECOVERY_MARKER)
            .exists());
    }

    #[test]
    fn concurrent_writer_is_rejected() {
        let root = TempDir::new().unwrap();
        let first = StateMutationGuard::acquire(root.path(), "first").unwrap();
        let error = StateMutationGuard::acquire(root.path(), "second")
            .err()
            .expect("exclusive state lock must reject another writer");
        assert!(error.to_string().contains("Another FastSkill writer"));
        first.recovered().unwrap();
    }

    #[test]
    fn projects_sharing_a_destination_contend_on_the_same_lock() {
        let first_root = TempDir::new().unwrap();
        let second_root = TempDir::new().unwrap();
        let destination = TempDir::new().unwrap();
        let first =
            StateMutationGuard::acquire_for(first_root.path(), Some(destination.path()), "first")
                .unwrap();
        let error =
            StateMutationGuard::acquire_for(second_root.path(), Some(destination.path()), "second")
                .err()
                .expect("shared destination must serialize project writers");
        assert!(error.to_string().contains("Another FastSkill writer"));
        first.recovered().unwrap();
    }

    #[test]
    fn interrupted_shared_destination_blocks_a_different_project() {
        let first_root = TempDir::new().unwrap();
        let second_root = TempDir::new().unwrap();
        let destination = TempDir::new().unwrap();
        drop(
            StateMutationGuard::acquire_for(first_root.path(), Some(destination.path()), "first")
                .unwrap(),
        );

        let error =
            StateMutationGuard::acquire_for(second_root.path(), Some(destination.path()), "second")
                .err()
                .expect("destination recovery marker must survive the first project");
        assert!(error.to_string().contains("was interrupted"));
        assert!(destination
            .path()
            .join(DESTINATION_RECOVERY_MARKER)
            .is_file());
    }

    #[test]
    fn borrowed_lease_must_cover_the_project_and_stays_active_for_its_owner() {
        let root = TempDir::new().unwrap();
        let other = TempDir::new().unwrap();
        let destination = TempDir::new().unwrap();
        let marker = root.path().join(STATE_DIRECTORY).join(RECOVERY_MARKER);
        let guard =
            StateMutationGuard::acquire_for(root.path(), Some(destination.path()), "combined")
                .unwrap();
        assert!(guard
            .ensure_covers(root.path(), Some(destination.path()))
            .is_ok());
        assert!(guard
            .ensure_covers(other.path(), Some(destination.path()))
            .is_err());

        StateMutationLease::acquire_or_borrow(
            root.path(),
            Some(destination.path()),
            "phase",
            Some(&guard),
        )
        .unwrap()
        .commit()
        .unwrap();

        assert!(marker.exists());
        guard.commit().unwrap();
        assert!(!marker.exists());
    }
}

//! Applying a managed state to this user's Agent targets (ADR-0016 decisions 8 to 12 and 17).
//!
//! One apply, under the per-user lock:
//!
//! 1. reads the enrollment, enrolling first when the caller allows it;
//! 2. fetches the state, falling back to the last accepted one when the source can't be read
//!    or what it serves is refused;
//! 3. unless the state has expired, publishes each listed skill into the store and deploys it
//!    to every target, adopting identical content, quarantining what must go, reporting
//!    collisions and removing owned entries that are no longer listed;
//! 4. quarantines blocked content in the targets and the project skills folder, even when the
//!    state has expired;
//! 5. records ownership, the accepted state and what happened.

use super::config::{ManagedSettings, ManagedSource};
use super::layout::{read_record, write_record, ManagedLayout};
use super::quarantine::{quarantine, remove_entry, QuarantineReason, QuarantineRecord};
use super::records::{Enrollment, EntryMode, OwnedEntry, Ownership};
use super::state::{Allowed, ManagedState};
use super::store::ManagedStore;
use super::OpenedState;
use crate::core::content_digest::content_digest;
use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What an apply works on.
#[derive(Debug, Clone)]
pub struct ApplyContext {
    pub layout: ManagedLayout,
    pub settings: ManagedSettings,
    /// The user's home folder, under which Agent targets are found.
    pub home: PathBuf,
    /// The current project's skills folder, checked for blocked content and id clashes.
    pub project_skills: Option<PathBuf>,
    /// Enroll when this user isn't enrolled yet.
    pub may_enroll: bool,
    pub now: DateTime<Utc>,
}

/// Whether an apply ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResult {
    /// Another apply holds the lock.
    Busy,
    Done(Box<ApplyOutcome>),
}

/// One target entry an apply changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryChange {
    pub path: PathBuf,
    pub id: String,
    pub digest: String,
}

/// What an apply did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyOutcome {
    /// Every step succeeded.
    pub completed: bool,
    pub applied_at: Option<DateTime<Utc>>,
    pub subject: String,
    pub issued_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    /// The state has expired, so no content was added, changed or removed for being unlisted.
    pub expired: bool,
    /// Why the source's state wasn't used, when the last accepted one was used instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_problem: Option<String>,
    pub enrolled_now: bool,
    pub targets: Vec<PathBuf>,
    pub deployed: Vec<EntryChange>,
    pub adopted: Vec<EntryChange>,
    pub removed: Vec<EntryChange>,
    pub quarantined: Vec<QuarantineRecord>,
    /// Entries that aren't FastSkill's and hold other content than the state lists.
    pub collisions: Vec<PathBuf>,
    /// Project skills with the id of a managed skill.
    pub project_clashes: Vec<PathBuf>,
    /// Skills or entries that failed, with why.
    pub failures: Vec<(String, String)>,
}

/// Apply the configured managed state.
pub fn apply(context: &ApplyContext) -> Result<ApplyResult, ServiceError> {
    let source = context
        .settings
        .source
        .clone()
        .ok_or_else(|| ServiceError::Config("no managed source is configured".to_string()))?;
    let layout = &context.layout;
    let Some(_lock) = layout.try_lock()? else {
        return Ok(ApplyResult::Busy);
    };
    let mut outcome = ApplyOutcome {
        applied_at: Some(context.now),
        ..ApplyOutcome::default()
    };

    let mut enrollment: Enrollment = read_record(&layout.enrollment_file())?;
    if !enrollment.is_enrolled() {
        if !context.may_enroll {
            return Err(ServiceError::InvalidOperation(
                "this user isn't enrolled; run `fastskill managed enroll`".to_string(),
            ));
        }
        enrollment = Enrollment::new(source.as_str(), context.now);
        outcome.enrolled_now = true;
    } else if !source.matches(&enrollment.source) {
        return Err(ServiceError::InvalidOperation(format!(
            "this user is enrolled with {:?}, but the configured source is {:?}; run \
             `fastskill managed enroll` again",
            enrollment.source,
            source.as_str()
        )));
    }

    let opened = fetch_and_open(context, &source, &enrollment, &mut outcome)?;
    enrollment.recorded = enrollment.recorded.accept(&opened.state);
    write_record(&layout.enrollment_file(), &enrollment)?;

    let state = &opened.state;
    outcome.subject = state.subject.clone();
    outcome.issued_at = Some(state.issued_at);
    outcome.expires_at = Some(state.expires_at);
    outcome.expired = state.is_expired(context.now);
    outcome.targets = targets(context);

    let mut ownership: Ownership = read_record(&layout.ownership_file())?;
    let mut run = Run {
        context,
        state,
        ownership: &mut ownership,
        outcome: &mut outcome,
    };
    if !run.outcome.expired {
        run.deploy_listed(&source);
        run.remove_unlisted();
    }
    run.sweep_targets();
    run.check_project();
    if !outcome.expired {
        let keep: Vec<&str> = state
            .skills
            .iter()
            .map(|skill| skill.digest.as_str())
            .collect();
        if let Err(error) = ManagedStore::new(layout).retain(&keep) {
            outcome
                .failures
                .push(("store".to_string(), error.to_string()));
        }
    }

    write_record(&layout.ownership_file(), &ownership)?;
    outcome.completed = outcome.failures.is_empty();
    write_record(&layout.last_apply_file(), &outcome)?;
    Ok(ApplyResult::Done(Box::new(outcome)))
}

/// Open the source's state and cache it, or open the cached one when the source's can't be
/// used.
fn fetch_and_open(
    context: &ApplyContext,
    source: &ManagedSource,
    enrollment: &Enrollment,
    outcome: &mut ApplyOutcome,
) -> Result<OpenedState, ServiceError> {
    let fetched = fetch(source).and_then(|bytes| {
        super::open(&bytes, &context.settings, &enrollment.recorded, context.now)
            .map(|opened| (bytes, opened))
            .map_err(ServiceError::from)
    });
    let cached = context.layout.cached_state();
    match fetched {
        Ok((bytes, opened)) => {
            crate::utils::atomic_write(&cached, &bytes)?;
            Ok(opened)
        }
        Err(problem) => {
            let Ok(bytes) = std::fs::read(&cached) else {
                return Err(problem);
            };
            // The cached state was accepted before; check it again against today's settings.
            match super::open(&bytes, &context.settings, &enrollment.recorded, context.now) {
                Ok(opened) => {
                    outcome.state_problem = Some(problem.to_string());
                    Ok(opened)
                }
                Err(_) => Err(problem),
            }
        }
    }
}

/// Read the state's envelope from the source.
pub fn fetch(source: &ManagedSource) -> Result<Vec<u8>, ServiceError> {
    match source {
        ManagedSource::File(path) => std::fs::read(path).map_err(|error| {
            ServiceError::Io(std::io::Error::new(
                error.kind(),
                format!("can't read the managed state {}: {error}", path.display()),
            ))
        }),
        ManagedSource::Https(url) => Err(ServiceError::InvalidOperation(format!(
            "reading the managed state from {url} isn't supported by this FastSkill yet"
        ))),
    }
}

/// The Agent targets: the fewest per-user skills folders that reach the configured agents, or
/// the agents present for this user.
pub fn targets(context: &ApplyContext) -> Vec<PathBuf> {
    let detected;
    let keys: Vec<&str> = match &context.settings.targets {
        Some(targets) => targets.iter().map(String::as_str).collect(),
        None => {
            detected = aikit_sdk::detect_present_agents(&context.home);
            detected.clone()
        }
    };
    aikit_sdk::fewest_skill_dirs(&context.home, &keys)
        .into_iter()
        .map(|(dir, _)| dir)
        .collect()
}

/// The digest of a target entry's content, following one link at the entry itself.
pub(crate) fn entry_digest(path: &Path) -> Option<String> {
    let resolved = std::fs::canonicalize(path).ok()?;
    resolved
        .is_dir()
        .then(|| content_digest(&resolved).ok())
        .flatten()
}

struct Run<'a> {
    context: &'a ApplyContext,
    state: &'a ManagedState,
    ownership: &'a mut Ownership,
    outcome: &'a mut ApplyOutcome,
}

impl Run<'_> {
    fn fail(&mut self, what: impl Into<String>, error: impl ToString) {
        self.outcome.failures.push((what.into(), error.to_string()));
    }

    fn quarantine(&mut self, path: &Path, digest: Option<&str>, reason: QuarantineReason) -> bool {
        match quarantine(&self.context.layout, path, digest, reason, self.context.now) {
            Ok(record) => {
                self.outcome.quarantined.push(record);
                true
            }
            Err(error) => {
                self.fail(path.display().to_string(), error);
                false
            }
        }
    }

    /// Whether content with `digest` found in a target must not stay there.
    fn must_go(&self, digest: &str) -> Option<QuarantineReason> {
        if self.state.blocked(digest).is_some() {
            Some(QuarantineReason::Blocked)
        } else if self.state.allowed == Allowed::Listed
            && self.state.exclusive_targets
            && !self.state.allows(digest)
        {
            Some(QuarantineReason::NotAllowed)
        } else {
            None
        }
    }

    fn deploy_listed(&mut self, source: &ManagedSource) {
        let store = ManagedStore::new(&self.context.layout);
        let targets = self.outcome.targets.clone();
        for skill in &self.state.skills {
            let published = match store.ensure(skill, source) {
                Ok(path) => path,
                Err(error) => {
                    self.fail(skill.id.clone(), error);
                    continue;
                }
            };
            for target in &targets {
                self.deploy(&published, target, &skill.id, &skill.digest);
            }
        }
    }

    fn deploy(&mut self, published: &Path, target: &Path, id: &str, digest: &str) {
        let entry = target.join(id);
        let present = entry.symlink_metadata().is_ok();
        let current = present.then(|| entry_digest(&entry)).flatten();
        let change = EntryChange {
            path: entry.clone(),
            id: id.to_string(),
            digest: digest.to_string(),
        };
        match self.ownership.find(target, id).cloned() {
            Some(owned) if present => {
                if current.as_deref() == Some(digest) {
                    if owned.digest != digest {
                        self.ownership.record(OwnedEntry {
                            digest: digest.to_string(),
                            ..owned
                        });
                    }
                    return;
                }
                if current.as_deref() != Some(owned.digest.as_str())
                    && !self.quarantine(&entry, current.as_deref(), QuarantineReason::Modified)
                {
                    return;
                }
            }
            Some(_) => {}
            None if !present => {}
            None => {
                if current.as_deref() == Some(digest) {
                    self.ownership.record(OwnedEntry {
                        target: target.to_path_buf(),
                        id: id.to_string(),
                        digest: digest.to_string(),
                        mode: EntryMode::Adopted,
                    });
                    self.outcome.adopted.push(change);
                    return;
                }
                let reason = current.as_deref().and_then(|found| self.must_go(found));
                match reason {
                    Some(reason) => {
                        if !self.quarantine(&entry, current.as_deref(), reason) {
                            return;
                        }
                    }
                    None => {
                        self.outcome.collisions.push(entry);
                        return;
                    }
                }
            }
        }
        match aikit_sdk::deploy_skill_entry(published, target, id, aikit_sdk::DeployMode::Auto) {
            Ok(deployed) => {
                let mode = match deployed.mode {
                    aikit_sdk::DeployMode::Link => EntryMode::Link,
                    _ => EntryMode::Copy,
                };
                self.ownership.record(OwnedEntry {
                    target: target.to_path_buf(),
                    id: id.to_string(),
                    digest: digest.to_string(),
                    mode,
                });
                self.outcome.deployed.push(change);
            }
            Err(error) => self.fail(entry.display().to_string(), error),
        }
    }

    /// Remove owned entries the state no longer lists, or that are in a folder that's no longer
    /// a target. A changed copy is quarantined instead.
    fn remove_unlisted(&mut self) {
        let listed = |entry: &OwnedEntry, targets: &[PathBuf], state: &ManagedState| {
            targets.contains(&entry.target) && state.skills.iter().any(|skill| skill.id == entry.id)
        };
        let stale: Vec<OwnedEntry> = self
            .ownership
            .entries
            .iter()
            .filter(|entry| !listed(entry, &self.outcome.targets, self.state))
            .cloned()
            .collect();
        for owned in stale {
            let path = owned.path();
            if path.symlink_metadata().is_ok() {
                let current = entry_digest(&path);
                let removed = if current.as_deref() == Some(owned.digest.as_str()) {
                    match remove_entry(&path) {
                        Ok(()) => {
                            self.outcome.removed.push(EntryChange {
                                path: path.clone(),
                                id: owned.id.clone(),
                                digest: owned.digest.clone(),
                            });
                            true
                        }
                        Err(error) => {
                            self.fail(path.display().to_string(), error);
                            false
                        }
                    }
                } else {
                    self.quarantine(&path, current.as_deref(), QuarantineReason::Modified)
                };
                if !removed {
                    continue;
                }
            }
            self.ownership.forget(&owned.target, &owned.id);
        }
    }

    /// Quarantine blocked content in every target, and with `allowed = listed` plus
    /// `exclusive_targets` any content whose digest isn't allowed.
    fn sweep_targets(&mut self) {
        for target in self.outcome.targets.clone() {
            for (path, digest) in entries(&target) {
                if let Some(reason) = self.must_go(&digest) {
                    if self.quarantine(&path, Some(&digest), reason) {
                        if let Some(id) = path.file_name().and_then(|name| name.to_str()) {
                            self.ownership.forget(&target, id);
                        }
                    }
                }
            }
        }
    }

    /// Quarantine blocked project skills, and report project skills that share a managed id.
    fn check_project(&mut self) {
        let Some(folder) = self.context.project_skills.clone() else {
            return;
        };
        for (path, digest) in entries(&folder) {
            if self.state.blocked(&digest).is_some() {
                self.quarantine(&path, Some(&digest), QuarantineReason::Blocked);
            }
        }
        for skill in &self.state.skills {
            let path = folder.join(&skill.id);
            if path.symlink_metadata().is_ok() {
                self.outcome.project_clashes.push(path);
            }
        }
    }
}

/// The skill entries of a folder with their digests, leaving out hidden entries such as a
/// deploy's temporary ones and anything that isn't a readable skill folder.
fn entries(folder: &Path) -> Vec<(PathBuf, String)> {
    let Ok(read) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(PathBuf, String)> = read
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.starts_with('.'))
        })
        .filter_map(|path| entry_digest(&path).map(|digest| (path, digest)))
        .collect();
    found.sort();
    found
}

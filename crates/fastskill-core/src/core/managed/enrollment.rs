//! Enrolling, unenrolling and the status of managed state for this user (ADR-0016 decision
//! 17).

use super::apply::{apply, entry_digest, ApplyContext, ApplyOutcome, ApplyResult};
use super::config::{ManagedSettings, ManagedSource};
use super::layout::{read_record, write_record, ManagedLayout};
use super::quarantine::{self, quarantine, remove_entry, QuarantineReason, QuarantineRecord};
use super::records::{Enrollment, OwnedEntry, Ownership};
use super::report::{self, Report, ReportInput};
use super::state::{ManagedState, Recorded};
use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// Enroll this user with the configured source, then apply its first state.
///
/// Enrolling again with the same source keeps the machine id and the newest accepted
/// `issued_at`, and forgets the recorded subject, so the next state may name another one.
pub fn enroll(context: &ApplyContext) -> Result<ApplyResult, ServiceError> {
    let source = context
        .settings
        .source
        .as_ref()
        .ok_or_else(|| ServiceError::Config("no managed source is configured".to_string()))?;
    let layout = &context.layout;
    {
        let Some(_lock) = layout.try_lock()? else {
            return Ok(ApplyResult::Busy);
        };
        let previous: Enrollment = read_record(&layout.enrollment_file())?;
        let mut enrollment = Enrollment::new(source.as_str(), context.now);
        if previous.is_enrolled() && source.matches(&previous.source) {
            enrollment.machine_id = previous.machine_id;
            enrollment.report_sequence = previous.report_sequence;
            enrollment.recorded = Recorded {
                subject: None,
                highest_issued_at: previous.recorded.highest_issued_at,
            };
        }
        write_record(&layout.enrollment_file(), &enrollment)?;
    }
    apply(context)
}

/// What unenrolling did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnenrollOutcome {
    pub removed: Vec<PathBuf>,
    pub quarantined: Vec<QuarantineRecord>,
    /// The user managed settings file, when it was removed.
    pub removed_settings: Option<PathBuf>,
}

/// Remove what managed state put on this machine for this user: every owned target entry (a
/// changed copy is quarantined instead), the store, the cached state, the records with the
/// machine id, and the user managed settings file. The quarantine is kept.
///
/// Refused when the system file sets `required = true`, and while an apply runs.
pub fn unenroll(
    layout: &ManagedLayout,
    settings: &ManagedSettings,
    user_settings_file: Option<&Path>,
    now: DateTime<Utc>,
) -> Result<UnenrollOutcome, ServiceError> {
    if settings.required_by_system {
        return Err(ServiceError::InvalidOperation(
            "this machine's managed settings require managed state, so this user can't \
             unenroll; ask whoever manages this machine"
                .to_string(),
        ));
    }
    let lock = layout.try_lock()?.ok_or_else(|| {
        ServiceError::InvalidOperation(
            "an apply is running; try again when it finishes".to_string(),
        )
    })?;
    let mut outcome = UnenrollOutcome::default();
    let ownership: Ownership = read_record(&layout.ownership_file())?;
    for owned in &ownership.entries {
        remove_owned(layout, owned, now, &mut outcome)?;
    }
    for entry in std::fs::read_dir(layout.managed())? {
        let path = entry?.path();
        if path != layout.lock_file() {
            remove_entry(&path)?;
        }
    }
    drop(lock);
    // The lock file may still be open elsewhere on some platforms; leaving it is harmless.
    let _ = std::fs::remove_file(layout.lock_file());
    let _ = std::fs::remove_dir(layout.managed());
    if let Some(file) = user_settings_file.filter(|file| file.is_file()) {
        std::fs::remove_file(file)?;
        outcome.removed_settings = Some(file.to_path_buf());
    }
    Ok(outcome)
}

fn remove_owned(
    layout: &ManagedLayout,
    owned: &OwnedEntry,
    now: DateTime<Utc>,
    outcome: &mut UnenrollOutcome,
) -> Result<(), ServiceError> {
    let path = owned.path();
    if path.symlink_metadata().is_err() {
        return Ok(());
    }
    let current = entry_digest(&path);
    if current.as_deref() == Some(owned.digest.as_str()) {
        remove_entry(&path)?;
        outcome.removed.push(path);
    } else {
        let record = quarantine(
            layout,
            &path,
            current.as_deref(),
            QuarantineReason::Modified,
            now,
        )?;
        outcome.quarantined.push(record);
    }
    Ok(())
}

/// What `managed status` shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedStatus {
    /// The configured source, if any.
    pub source: Option<String>,
    pub enrollment: Option<Enrollment>,
    pub subject: Option<String>,
    pub issued_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub expired: bool,
    /// Why the cached state can't be used, when it can't.
    pub state_problem: Option<String>,
    pub owned: Vec<OwnedEntry>,
    pub last_apply: Option<ApplyOutcome>,
    pub quarantine: Vec<QuarantineRecord>,
    /// The last apply found that the user must sign in (decision 6).
    pub sign_in_needed: bool,
    /// State fields this source ignores, each with why (decision 21).
    pub ignored: Vec<String>,
    /// The body the next report would carry, built from the last apply.
    pub report: Option<Report>,
    /// Where reports go, or why none is sent.
    pub report_note: Option<String>,
}

/// Read the status from the records and the cached state, changing nothing.
pub fn status(
    layout: &ManagedLayout,
    settings: &ManagedSettings,
    now: DateTime<Utc>,
) -> Result<ManagedStatus, ServiceError> {
    let enrollment: Enrollment = read_record(&layout.enrollment_file())?;
    let ownership: Ownership = read_record(&layout.ownership_file())?;
    let last_apply: ApplyOutcome = read_record(&layout.last_apply_file())?;
    let mut status = ManagedStatus {
        source: settings
            .source
            .as_ref()
            .map(|source| source.as_str().to_string()),
        owned: ownership.entries,
        sign_in_needed: last_apply.sign_in_needed,
        last_apply: last_apply.applied_at.is_some().then_some(last_apply),
        quarantine: quarantine::list(layout)?,
        ..ManagedStatus::default()
    };
    let Some(source) = settings.source.as_ref() else {
        return Ok(status);
    };
    if enrollment.is_enrolled() {
        match open_cached(layout, settings, &enrollment, now) {
            Ok(opened) => {
                status.subject = Some(opened.state.subject.clone());
                status.issued_at = Some(opened.state.issued_at);
                status.expires_at = Some(opened.state.expires_at);
                status.expired = opened.state.is_expired(now);
                status.ignored = ignored_fields(&opened.state, source);
                status.report_note = Some(
                    match report::destination(opened.state.report_url.as_deref(), source) {
                        Ok(Some(url)) => format!("sent to {url}"),
                        Ok(None) => "none: the state names no report_url".to_string(),
                        Err(problem) => format!("none: {problem}"),
                    },
                );
            }
            Err(problem) => status.state_problem = Some(problem),
        }
        let last = status.last_apply.clone().unwrap_or_default();
        status.report = Some(Report::build(ReportInput {
            source,
            enrollment: &enrollment,
            issued_at: last.issued_at,
            completed: last.completed,
            snapshot: &last.snapshot,
            quarantine: &status.quarantine,
            refusals: &report::refusals(layout)?,
        }));
        status.enrollment = Some(enrollment);
    }
    Ok(status)
}

/// The fields a file source's state sets that only an `https://` source honors (decision 21).
fn ignored_fields(state: &ManagedState, source: &ManagedSource) -> Vec<String> {
    if !matches!(source, ManagedSource::File(_)) {
        return Vec::new();
    }
    let mut ignored = Vec::new();
    if state.editable.is_some() {
        ignored.push(
            "editable: a file source always uses blocked-only, so editable local skills skip \
             the allow-list"
                .to_string(),
        );
    }
    if state.request_url.is_some() {
        ignored.push("request_url: a file source has no origin to send people to".to_string());
    }
    if state.report_url.is_some() {
        ignored.push("report_url: a file source never gets a report".to_string());
    }
    ignored
}

/// Verify the cached state again, or say why it can't be used.
pub(crate) fn open_cached(
    layout: &ManagedLayout,
    settings: &ManagedSettings,
    enrollment: &Enrollment,
    now: DateTime<Utc>,
) -> Result<super::OpenedState, String> {
    let bytes = std::fs::read(layout.cached_state())
        .map_err(|_| "no state has been accepted yet".to_string())?;
    // The cached state is the newest accepted, so its own issued_at is allowed.
    super::open(&bytes, settings, &enrollment.recorded, now).map_err(|error| error.to_string())
}

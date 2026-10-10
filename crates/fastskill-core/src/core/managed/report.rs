//! The report an apply sends to the state's `report_url`, and the refusals it carries
//! (ADR-0016 decisions 6, 15 and 21).
//!
//! The body is fixed: ids, digests, outcomes, agent names and counts, never prompts, file
//! contents, paths, usernames, project names, repository URLs or usage. `managed status --json
//! --report` prints the same body without sending it.

use super::apply::{entries, ApplyContext, ApplyOutcome};
use super::config::ManagedSource;
use super::layout::{create_private_dir, read_record, write_record, ManagedLayout};
use super::quarantine::{self, QuarantineReason, QuarantineRecord};
use super::records::{Enrollment, EntryMode, Ownership};
use super::remote::Remote;
use super::state::ManagedState;
use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The report format this FastSkill sends.
pub const REPORT_FORMAT_VERSION: u32 = 1;

/// What happened to a skill entry in an Agent target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillOutcome {
    Deployed,
    Adopted,
    Collision,
    Quarantined,
    /// Not FastSkill's, and left alone.
    Unmanaged,
}

/// One skill in an Agent target, without its path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ReportSkill {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// `managed` for what FastSkill placed; `None` when FastSkill can't know.
    pub origin_kind: Option<String>,
    pub editable: bool,
    pub outcome: SkillOutcome,
}

/// A covered agent, by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportAgent {
    pub name: String,
    pub hook: bool,
}

/// One quarantined item, without its paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportQuarantine {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub reason: QuarantineReason,
    pub quarantined_at: DateTime<Utc>,
}

impl From<&QuarantineRecord> for ReportQuarantine {
    fn from(record: &QuarantineRecord) -> Self {
        Self {
            id: record
                .former_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            digest: record.digest.clone(),
            reason: record.reason,
            quarantined_at: record.quarantined_at,
        }
    }
}

/// A skill in the current project's skills folder.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProjectSkill {
    pub id: String,
    pub digest: String,
}

/// An install the managed state refused: the digest and the command, never its arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Refusal {
    pub digest: String,
    pub command: String,
}

/// The refusals kept for this user until a report carrying them is accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusals {
    #[serde(default)]
    pub entries: Vec<Refusal>,
}

/// What an apply saw in the targets and the project, kept with its outcome so the report can
/// be built again later.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSnapshot {
    #[serde(default)]
    pub agents: Vec<ReportAgent>,
    #[serde(default)]
    pub skills: Vec<ReportSkill>,
    #[serde(default)]
    pub quarantined_now: Vec<ReportQuarantine>,
    #[serde(default)]
    pub project_skills: Vec<ProjectSkill>,
}

/// The report body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub format_version: u32,
    pub machine_id: String,
    pub sequence: u64,
    pub issued_at: Option<DateTime<Utc>>,
    pub completed: bool,
    pub agents: Vec<ReportAgent>,
    pub skills: Vec<ReportSkill>,
    pub quarantine: Vec<ReportQuarantine>,
    pub quarantined_now: Vec<ReportQuarantine>,
    pub refusals: Vec<Refusal>,
    pub project_skills: Vec<ProjectSkill>,
}

/// Everything a report is built from.
pub struct ReportInput<'a> {
    pub source: &'a ManagedSource,
    pub enrollment: &'a Enrollment,
    pub issued_at: Option<DateTime<Utc>>,
    pub completed: bool,
    pub snapshot: &'a ReportSnapshot,
    pub quarantine: &'a [QuarantineRecord],
    pub refusals: &'a Refusals,
}

impl Report {
    /// The next report from this machine id. Refusals and project skills are left out unless
    /// the source is `https://` (decision 21).
    pub fn build(input: ReportInput<'_>) -> Self {
        let https = matches!(input.source, ManagedSource::Https(_));
        Self {
            format_version: REPORT_FORMAT_VERSION,
            machine_id: input.enrollment.machine_id.clone(),
            sequence: input.enrollment.report_sequence + 1,
            issued_at: input.issued_at,
            completed: input.completed,
            agents: input.snapshot.agents.clone(),
            skills: input.snapshot.skills.clone(),
            quarantine: input
                .quarantine
                .iter()
                .map(ReportQuarantine::from)
                .collect(),
            quarantined_now: input.snapshot.quarantined_now.clone(),
            refusals: if https {
                input.refusals.entries.clone()
            } else {
                Vec::new()
            },
            project_skills: if https {
                input.snapshot.project_skills.clone()
            } else {
                Vec::new()
            },
        }
    }
}

/// Where a report goes, or why none is sent: no `report_url`, a file source, or a URL on
/// another origin (decisions 6 and 21).
pub fn destination(
    report_url: Option<&str>,
    source: &ManagedSource,
) -> Result<Option<String>, String> {
    let Some(url) = report_url else {
        return Ok(None);
    };
    match source {
        ManagedSource::File(_) => Err("a file source never gets a report".to_string()),
        ManagedSource::Https(_) if source.same_origin(url) => Ok(Some(url.to_string())),
        ManagedSource::Https(_) => Err(format!(
            "report_url {url:?} isn't on the managed source's origin, so no report was sent"
        )),
    }
}

/// What happened to the report of one apply.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportDelivery {
    pub sequence: u64,
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// What an apply saw, for the report: the covered agents, every skill entry in the targets,
/// this run's quarantine actions and the project's skills.
pub(crate) fn snapshot(
    context: &ApplyContext,
    covered: &[(PathBuf, Vec<&'static str>)],
    ownership: &Ownership,
    outcome: &ApplyOutcome,
) -> ReportSnapshot {
    let mut names: Vec<&str> = covered
        .iter()
        .flat_map(|(_, agents)| agents.iter().copied())
        .collect();
    names.sort_unstable();
    names.dedup();
    let store = context.layout.store();
    let mut skills = Vec::new();
    for (target, _) in covered {
        for (path, digest) in entries(target) {
            let id = file_name(&path);
            let owned = ownership.find(target, &id);
            let (outcome, origin_kind, editable) = if outcome.collisions.contains(&path) {
                (SkillOutcome::Collision, None, false)
            } else if let Some(owned) = owned {
                let placed = if owned.mode == EntryMode::Adopted {
                    SkillOutcome::Adopted
                } else {
                    SkillOutcome::Deployed
                };
                (placed, Some("managed".to_string()), false)
            } else {
                (SkillOutcome::Unmanaged, None, links_outside(&path, &store))
            };
            skills.push(ReportSkill {
                id,
                digest: Some(digest),
                origin_kind,
                editable,
                outcome,
            });
        }
    }
    for record in &outcome.quarantined {
        let in_target = record
            .former_path
            .parent()
            .is_some_and(|parent| covered.iter().any(|(target, _)| target == parent));
        if in_target {
            skills.push(ReportSkill {
                id: file_name(&record.former_path),
                digest: record.digest.clone(),
                origin_kind: None,
                editable: false,
                outcome: SkillOutcome::Quarantined,
            });
        }
    }
    skills.sort();
    skills.dedup();
    let mut project_skills: Vec<ProjectSkill> = context
        .project_skills
        .as_deref()
        .map(entries)
        .unwrap_or_default()
        .into_iter()
        .map(|(path, digest)| ProjectSkill {
            id: file_name(&path),
            digest,
        })
        .collect();
    project_skills.sort();
    project_skills.dedup();
    ReportSnapshot {
        agents: names
            .into_iter()
            .map(|name| ReportAgent {
                name: name.to_string(),
                hook: false,
            })
            .collect(),
        skills,
        quarantined_now: outcome
            .quarantined
            .iter()
            .map(ReportQuarantine::from)
            .collect(),
        project_skills,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether the entry is a link to a folder outside the managed store, as an editable local
/// skill is.
fn links_outside(path: &Path, store: &Path) -> bool {
    let is_link = path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    let store = std::fs::canonicalize(store).unwrap_or_else(|_| store.to_path_buf());
    is_link && std::fs::canonicalize(path).is_ok_and(|resolved| !resolved.starts_with(&store))
}

/// Warnings about fields of an `https://` source's state that point at another origin.
pub(crate) fn state_warnings(state: &ManagedState, source: &ManagedSource) -> Vec<String> {
    let mut warnings = Vec::new();
    if let (ManagedSource::Https(_), Some(template)) = (source, state.request_url.as_deref()) {
        if state.request_link(source, "x").is_none() {
            warnings.push(format!(
                "request_url {template:?} isn't on the managed source's origin, so refusals \
                 show no request link"
            ));
        }
    }
    warnings
}

/// Send the report when the state names one on the source's origin, and record what
/// happened. The sequence number goes up with each report sent; an accepted report clears the
/// refusals it carried.
pub(crate) fn send(
    context: &ApplyContext,
    source: &ManagedSource,
    state: &ManagedState,
    remote: Option<&Remote>,
    enrollment: &mut Enrollment,
    outcome: &mut ApplyOutcome,
) -> Result<(), ServiceError> {
    let url = match destination(state.report_url.as_deref(), source) {
        Ok(Some(url)) => url,
        Ok(None) => return Ok(()),
        Err(problem) => {
            if matches!(source, ManagedSource::Https(_)) {
                outcome.warnings.push(problem);
            }
            return Ok(());
        }
    };
    let layout = &context.layout;
    let Some(remote) = remote.filter(|_| !outcome.sign_in_needed) else {
        outcome.report = Some(ReportDelivery {
            sequence: enrollment.report_sequence,
            accepted: false,
            problem: Some("not sent: sign-in is needed".to_string()),
        });
        return Ok(());
    };
    let refusals = refusals(layout)?;
    let quarantine = quarantine::list(layout)?;
    let report = Report::build(ReportInput {
        source,
        enrollment,
        issued_at: outcome.issued_at,
        completed: outcome.completed,
        snapshot: &outcome.snapshot,
        quarantine: &quarantine,
        refusals: &refusals,
    });
    enrollment.report_sequence = report.sequence;
    write_record(&layout.enrollment_file(), enrollment)?;
    let body =
        serde_json::to_vec(&report).map_err(|error| ServiceError::Custom(error.to_string()))?;
    let mut delivery = ReportDelivery {
        sequence: report.sequence,
        accepted: false,
        problem: None,
    };
    match remote.post_json(&url, body) {
        Ok(()) => {
            delivery.accepted = true;
            clear_refusals(layout, &report.refusals)?;
        }
        Err(error) => {
            outcome.sign_in_needed |= error.sign_in;
            delivery.problem = Some(error.message);
        }
    }
    outcome.report = Some(delivery);
    Ok(())
}

static CURRENT_COMMAND: Mutex<Option<String>> = Mutex::new(None);

/// Name the command this process runs (`skill add`, `bundle update`, …), so a refusal can be
/// recorded with it.
pub fn set_current_command(command: &str) {
    if let Ok(mut current) = CURRENT_COMMAND.lock() {
        *current = Some(command.to_string());
    }
}

fn current_command() -> String {
    CURRENT_COMMAND
        .lock()
        .ok()
        .and_then(|current| current.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Keep a refusal of `digest` by the current command, once per digest and command. Only an
/// enrolled user keeps refusals.
pub fn record_refusal(layout: &ManagedLayout, digest: &str) -> Result<(), ServiceError> {
    if !layout.enrollment_file().is_file() {
        return Ok(());
    }
    let refusal = Refusal {
        digest: digest.to_string(),
        command: current_command(),
    };
    update_refusals(layout, |refusals| {
        if !refusals.entries.contains(&refusal) {
            refusals.entries.push(refusal);
            refusals.entries.sort();
        }
    })
}

/// The kept refusals.
pub fn refusals(layout: &ManagedLayout) -> Result<Refusals, ServiceError> {
    read_record(&layout.refusals_file())
}

/// Forget the refusals an accepted report carried; newer ones stay.
pub fn clear_refusals(layout: &ManagedLayout, sent: &[Refusal]) -> Result<(), ServiceError> {
    update_refusals(layout, |refusals| {
        refusals.entries.retain(|refusal| !sent.contains(refusal));
    })
}

fn update_refusals(
    layout: &ManagedLayout,
    change: impl FnOnce(&mut Refusals),
) -> Result<(), ServiceError> {
    create_private_dir(&layout.managed())?;
    let lock = lock_file(&layout.refusals_lock())?;
    lock.lock_exclusive()?;
    let mut refusals: Refusals = read_record(&layout.refusals_file())?;
    change(&mut refusals);
    let written = write_record(&layout.refusals_file(), &refusals);
    let _ = lock.unlock();
    written
}

fn lock_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;

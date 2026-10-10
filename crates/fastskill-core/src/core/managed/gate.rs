//! The install gate and the managed statuses (ADR-0016 decisions 4, 13, 14, 21 to 23).
//!
//! Every operation that adds or replaces skill content asks [`ManagedGate`] first, before it
//! changes any state: the CLI, `server serve` and `mcp serve` share the core install seam, so
//! they all inherit it. `skill list` asks [`Situation::status_of`] for the two managed statuses,
//! and the CLI asks [`Situation::command_policy`] before a command that reads or changes skills.
//!
//! With no managed source configured every answer is "as today".

use super::config::{ManagedSettings, ManagedSource};
use super::enrollment::open_cached;
use super::layout::{read_record, ManagedLayout};
use super::records::Enrollment;
use super::state::{Allowed, Editable, ManagedState};
use super::ManagedError;
use crate::core::content_digest::content_digest;
use crate::core::service::ServiceError;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

/// Where this user stands with managed state, read once and used for every check in one
/// operation.
#[derive(Debug, Clone, PartialEq)]
pub enum Situation {
    /// No managed source is configured: nothing changes.
    NotConfigured,
    /// The managed settings can't be used, for instance an untrusted system file.
    Unusable(String),
    /// A source is configured but no valid state is cached.
    NoState { required: bool, problem: String },
    /// A verified state, which may have expired.
    State {
        state: Box<ManagedState>,
        source: ManagedSource,
        expired: bool,
    },
}

/// One of the two reconciliation statuses that only a managed state emits (decision 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedSkillStatus {
    /// The content's digest is blocked.
    Blocked,
    /// The state allows only listed content, and this isn't listed.
    NotAllowed,
}

/// What a command that reads or changes skills may do (decision 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPolicy {
    Run,
    /// Run, after printing this warning.
    Warn(String),
    /// Refuse with this message.
    Refuse(String),
}

/// One piece of content an operation is about to install.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub id: &'a str,
    /// The content's digest, when the caller already has it.
    pub digest: Option<&'a str>,
    /// Where the content is, to compute the digest when the caller doesn't have it.
    pub path: Option<&'a Path>,
    /// An editable local skill (ADR-0005), whose digest changes with every edit.
    pub editable: bool,
}

impl<'a> Candidate<'a> {
    /// Content whose digest is known.
    pub fn digest(id: &'a str, digest: &'a str) -> Self {
        Self {
            id,
            digest: Some(digest),
            path: None,
            editable: false,
        }
    }
}

const APPLY_HINT: &str = "run `fastskill managed apply`";

impl Situation {
    /// Read the situation of the current user from the trusted settings and the managed store.
    pub fn for_current_user(now: DateTime<Utc>) -> Self {
        match ManagedLayout::for_current_user() {
            Ok(layout) => Self::load(&layout, ManagedSettings::load(), now),
            Err(error) => match ManagedSettings::load() {
                Ok(settings) if !settings.is_configured() => Self::NotConfigured,
                _ => Self::Unusable(error.to_string()),
            },
        }
    }

    /// Read the situation from `settings` and the records in `layout`.
    pub fn load(
        layout: &ManagedLayout,
        settings: Result<ManagedSettings, ManagedError>,
        now: DateTime<Utc>,
    ) -> Self {
        let settings = match settings {
            Ok(settings) => settings,
            Err(error) => return Self::Unusable(error.to_string()),
        };
        let Some(source) = settings.source.clone() else {
            return Self::NotConfigured;
        };
        let no_state = |problem: String| Self::NoState {
            required: settings.required,
            problem,
        };
        let enrollment: Enrollment = match read_record(&layout.enrollment_file()) {
            Ok(enrollment) => enrollment,
            Err(error) => return no_state(error.to_string()),
        };
        if !enrollment.is_enrolled() {
            let hint = if settings.source_from_system {
                APPLY_HINT
            } else {
                "run `fastskill managed enroll`"
            };
            return no_state(format!("this user isn't enrolled; {hint}"));
        }
        match open_cached(layout, &settings, &enrollment, now) {
            Ok(opened) => Self::State {
                expired: opened.state.is_expired(now),
                state: Box::new(opened.state),
                source,
            },
            Err(problem) => no_state(format!("{problem}; {APPLY_HINT}")),
        }
    }

    /// Whether `candidate` may be installed, refusing with the reason when it may not
    /// (decisions 4, 13, 22 and 23).
    pub fn check(&self, candidate: Candidate<'_>) -> Result<(), ServiceError> {
        match self {
            Self::NotConfigured
            | Self::NoState {
                required: false, ..
            } => Ok(()),
            Self::Unusable(problem) => Err(refused(format!(
                "the managed settings can't be used: {problem}"
            ))),
            Self::NoState {
                required: true,
                problem,
            } => Err(refused(format!(
                "a valid managed state is required to change skills: {problem}"
            ))),
            Self::State {
                expired: true,
                state,
                ..
            } => Err(refused(format!(
                "the managed state expired at {}, so skills can't be added or changed; \
                 {APPLY_HINT}",
                state.expires_at
            ))),
            Self::State { state, source, .. } => {
                check_state(state, source, candidate).map_err(|refusal| refused(refusal.message))
            }
        }
    }

    /// [`Situation::check`], keeping a refusal of a known digest in `layout` for the next
    /// report when the source is `https://` (decisions 15 and 21).
    pub fn check_recording(
        &self,
        candidate: Candidate<'_>,
        layout: &ManagedLayout,
    ) -> Result<(), ServiceError> {
        let Self::State {
            state,
            source: source @ ManagedSource::Https(_),
            expired: false,
        } = self
        else {
            return self.check(candidate);
        };
        check_state(state, source, candidate).map_err(|refusal| {
            if let Some(digest) = &refusal.digest {
                if let Err(error) = super::report::record_refusal(layout, digest) {
                    tracing::warn!("couldn't keep the refusal for the managed report: {error}");
                }
            }
            refused(refusal.message)
        })
    }

    /// Check several candidates, stopping at the first refusal.
    pub fn check_all<'a>(
        &self,
        candidates: impl IntoIterator<Item = Candidate<'a>>,
    ) -> Result<(), ServiceError> {
        candidates
            .into_iter()
            .try_for_each(|candidate| self.check(candidate))
    }

    /// The managed status of installed content, or `None` when it has none. The digest is only
    /// computed when a verified state is present.
    pub fn status_of(
        &self,
        path: &Path,
        editable: bool,
    ) -> Result<Option<ManagedSkillStatus>, ServiceError> {
        let Self::State { state, source, .. } = self else {
            return Ok(None);
        };
        let digest = digest_of(path)?;
        Ok(if state.blocked(&digest).is_some() {
            Some(ManagedSkillStatus::Blocked)
        } else if not_allowed(state, source, &digest, editable) {
            Some(ManagedSkillStatus::NotAllowed)
        } else {
            None
        })
    }

    /// What a command that reads or changes skills may do (decision 4). Refusals for content
    /// are left to [`Situation::check`].
    pub fn command_policy(&self) -> CommandPolicy {
        match self {
            Self::NotConfigured => CommandPolicy::Run,
            Self::State { expired: false, .. } => CommandPolicy::Run,
            Self::Unusable(problem) => {
                CommandPolicy::Refuse(format!("the managed settings can't be used: {problem}"))
            }
            Self::NoState {
                required: true,
                problem,
            } => CommandPolicy::Refuse(format!(
                "a valid managed state is required to read or change skills: {problem}"
            )),
            Self::NoState {
                required: false,
                problem,
            } => CommandPolicy::Warn(format!("no valid managed state: {problem}")),
            Self::State { state, .. } => CommandPolicy::Warn(format!(
                "the managed state expired at {}; skills can't be added or changed until \
                 `fastskill managed apply` gets a newer one",
                state.expires_at
            )),
        }
    }
}

/// Why a candidate was refused, with its digest when it was known.
struct Refusal {
    message: String,
    digest: Option<String>,
}

impl Refusal {
    fn new(message: String, digest: Option<&str>) -> Self {
        Self {
            message,
            digest: digest.map(str::to_string),
        }
    }
}

fn check_state(
    state: &ManagedState,
    source: &ManagedSource,
    candidate: Candidate<'_>,
) -> Result<(), Refusal> {
    if candidate.editable
        && state.allowed == Allowed::Listed
        && state.editable(source) == Editable::Refused
    {
        return Err(Refusal::new(
            format!(
                "{} is an editable local skill, and the managed state refuses those",
                candidate.id
            ),
            None,
        ));
    }
    let digest = match (candidate.digest, candidate.path) {
        (Some(digest), _) => digest.to_string(),
        (None, Some(path)) => {
            digest_of(path).map_err(|error| Refusal::new(error.to_string(), None))?
        }
        (None, None) => {
            return Err(Refusal::new(
                format!(
                    "{} has no content digest to check against the managed state",
                    candidate.id
                ),
                None,
            ))
        }
    };
    if let Some(blocked) = state.blocked(&digest) {
        let message = blocked
            .message
            .as_deref()
            .map(|message| format!(": {message}"))
            .unwrap_or_default();
        return Err(Refusal::new(
            format!(
                "{} ({digest}) is blocked by the managed state{message}",
                candidate.id
            ),
            Some(&digest),
        ));
    }
    if not_allowed(state, source, &digest, candidate.editable) {
        let link = state
            .request_link(source, &digest)
            .map(|link| format!("; request it at {link}"))
            .unwrap_or_default();
        return Err(Refusal::new(
            format!(
                "{} ({digest}) isn't allowed by the managed state{link}",
                candidate.id
            ),
            Some(&digest),
        ));
    }
    Ok(())
}

/// Not allowed under `allowed = listed`. Editable local skills skip the list unless the state
/// refuses them (decision 22).
fn not_allowed(state: &ManagedState, source: &ManagedSource, digest: &str, editable: bool) -> bool {
    if state.allowed != Allowed::Listed || state.allows(digest) {
        return false;
    }
    !editable || state.editable(source) == Editable::Refused
}

/// The current-form digest of a skill folder, following a link at the top (an editable skill
/// is a link to its source).
fn digest_of(path: &Path) -> Result<String, ServiceError> {
    let folder = std::fs::canonicalize(path)?;
    content_digest(&folder)
}

fn refused(message: String) -> ServiceError {
    ServiceError::Validation(message)
}

/// The gate a service consults: the current user's situation, read again at each operation so a
/// long-running server follows new states, or a fixed one for tests and embedding callers.
#[derive(Debug, Clone, Default)]
pub struct ManagedGate {
    fixed: Option<Arc<Situation>>,
}

impl ManagedGate {
    /// Always use `situation`.
    pub fn fixed(situation: Situation) -> Self {
        Self {
            fixed: Some(Arc::new(situation)),
        }
    }

    /// The situation now.
    pub fn situation(&self) -> Arc<Situation> {
        match &self.fixed {
            Some(situation) => situation.clone(),
            None => Arc::new(Situation::for_current_user(Utc::now())),
        }
    }

    /// Check one candidate against the situation now. The current user's gate keeps refusals
    /// for the next report; a fixed one doesn't.
    pub fn check(&self, candidate: Candidate<'_>) -> Result<(), ServiceError> {
        self.check_all([candidate])
    }

    /// Check several candidates against one reading of the situation.
    pub fn check_all<'a>(
        &self,
        candidates: impl IntoIterator<Item = Candidate<'a>>,
    ) -> Result<(), ServiceError> {
        let situation = self.situation();
        let layout = match &self.fixed {
            Some(_) => None,
            None => ManagedLayout::for_current_user().ok(),
        };
        candidates
            .into_iter()
            .try_for_each(|candidate| match &layout {
                Some(layout) => situation.check_recording(candidate, layout),
                None => situation.check(candidate),
            })
    }
}

/// A verified, unexpired state from a local file that blocks `blocked` and, when `listed` is
/// given, allows only that digest; for tests elsewhere in the crate.
#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) fn test_situation(blocked: &str, listed: Option<&str>) -> Situation {
    let source = ManagedSource::File(std::env::temp_dir().join("state.dsse"));
    let value = serde_json::json!({
        "format_version": 1,
        "issued_at": "2026-10-09T11:00:00Z",
        "expires_at": "2026-10-16T11:00:00Z",
        "source": source.as_str(),
        "subject": "team-a",
        "skills": listed
            .map(|digest| vec![serde_json::json!({ "id": "listed", "digest": digest, "artifact": "a.zip" })])
            .unwrap_or_default(),
        "allowed": if listed.is_some() { "listed" } else { "any" },
        "blocked": [{ "digest": blocked, "message": "withdrawn" }],
    });
    Situation::State {
        state: Box::new(
            ManagedState::parse(&serde_json::to_vec(&value).expect("state json"))
                .expect("test state"),
        ),
        source,
        expired: false,
    }
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;

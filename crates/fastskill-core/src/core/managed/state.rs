//! The managed state format and the rules that decide whether FastSkill may use a state
//! (ADR-0016 decisions 1, 3 and 4).
//!
//! A state is resolved content, like a Lock: each skill is an id, a content digest and an
//! artifact location. Fields may be added within a format version, so unknown fields are
//! ignored; a format version FastSkill doesn't know is refused before anything else is read.

use super::config::ManagedSource;
use super::ManagedError;
use crate::core::content_digest::is_current_digest;
use crate::core::service::SkillId;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The managed state format version this FastSkill reads.
pub const FORMAT_VERSION: u32 = 1;

/// How far ahead of the local clock `issued_at` may be.
pub const MAX_CLOCK_SKEW: Duration = Duration::minutes(5);

/// The placeholder a `request_url` template carries once.
pub const DIGEST_PLACEHOLDER: &str = "{digest}";

/// One skill the state installs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSkill {
    /// The skill's id: one safe path component.
    pub id: String,
    /// Its content digest, in the current form.
    pub digest: String,
    /// Where its archive is: an `https://` URL, or for a file source also a local path.
    pub artifact: String,
}

/// Which digests may be installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Allowed {
    /// Only digests in `skills` or `also_allowed`.
    Listed,
    /// Any digest that isn't blocked.
    Any,
}

/// A digest that must not be present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedDigest {
    pub digest: String,
    /// What to tell the user, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Whether editable local skills skip the allow-list (decision 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editable {
    /// They skip it; blocked digests are still refused. The default.
    BlockedOnly,
    /// Adding one is refused.
    Refused,
}

/// A managed state, as its payload carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedState {
    pub format_version: u32,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// The managed source it was issued for.
    pub source: String,
    /// An opaque label, displayed and compared for equality, never interpreted.
    pub subject: String,
    #[serde(default)]
    pub skills: Vec<StateSkill>,
    pub allowed: Allowed,
    #[serde(default)]
    pub also_allowed: Vec<String>,
    #[serde(default)]
    pub blocked: Vec<BlockedDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_url: Option<String>,
    #[serde(default)]
    pub exclusive_targets: bool,
    /// `blocked-only` or `refused`; see [`ManagedState::editable`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editable: Option<String>,
    /// A link template with one `{digest}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_url: Option<String>,
}

/// What this user accepted before: the subject recorded at enrollment and the newest
/// `issued_at`. Both are empty right after enrollment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highest_issued_at: Option<DateTime<Utc>>,
}

impl Recorded {
    /// What to record once `state` is accepted.
    pub fn accept(&self, state: &ManagedState) -> Self {
        Self {
            subject: Some(state.subject.clone()),
            highest_issued_at: Some(
                self.highest_issued_at
                    .map_or(state.issued_at, |highest| highest.max(state.issued_at)),
            ),
        }
    }
}

#[derive(Deserialize)]
struct VersionOnly {
    format_version: u32,
}

impl ManagedState {
    /// Parse a payload, refusing a format version FastSkill doesn't read before anything else.
    pub fn parse(payload: &[u8]) -> Result<Self, ManagedError> {
        let version: VersionOnly = serde_json::from_slice(payload)
            .map_err(|error| ManagedError::Invalid(format!("no format_version: {error}")))?;
        if version.format_version != FORMAT_VERSION {
            return Err(ManagedError::Invalid(format!(
                "format version {} isn't supported (this FastSkill reads {FORMAT_VERSION}); \
                 update FastSkill",
                version.format_version
            )));
        }
        serde_json::from_slice(payload)
            .map_err(|error| ManagedError::Invalid(format!("malformed: {error}")))
    }

    /// The rules that refuse a whole state (decision 1).
    pub fn validate(&self, source: &ManagedSource) -> Result<(), ManagedError> {
        let invalid = |message: String| Err(ManagedError::Invalid(message));
        let mut ids = HashSet::new();
        for skill in &self.skills {
            if !is_safe_id(&skill.id) {
                return invalid(format!(
                    "skill id {:?} isn't one safe path component",
                    skill.id
                ));
            }
            if !ids.insert(skill.id.as_str()) {
                return invalid(format!("two skills share the id {:?}", skill.id));
            }
            check_digest(&skill.digest)?;
            check_artifact(&skill.id, &skill.artifact, source)?;
        }
        for digest in &self.also_allowed {
            check_digest(digest)?;
        }
        let listed: HashSet<&str> = self
            .skills
            .iter()
            .map(|skill| skill.digest.as_str())
            .chain(self.also_allowed.iter().map(String::as_str))
            .collect();
        for blocked in &self.blocked {
            check_digest(&blocked.digest)?;
            if listed.contains(blocked.digest.as_str()) {
                return invalid(format!(
                    "{} is blocked and also listed in skills or also_allowed",
                    blocked.digest
                ));
            }
        }
        if let Some(url) = &self.report_url {
            check_https("report_url", url)?;
        }
        if let Some(template) = &self.request_url {
            if template.matches(DIGEST_PLACEHOLDER).count() != 1 {
                return invalid(format!(
                    "request_url must contain {DIGEST_PLACEHOLDER} exactly once"
                ));
            }
            check_https("request_url", &template.replace(DIGEST_PLACEHOLDER, "x"))?;
        }
        if let Some(value) = &self.editable {
            if parse_editable(value).is_none() {
                return invalid(format!(
                    "editable is {value:?}; it must be \"blocked-only\" or \"refused\""
                ));
            }
        }
        Ok(())
    }

    /// Bind the state to the configured source and the recorded subject, refuse rollback and
    /// a future `issued_at` (decision 3).
    pub fn check_binding(
        &self,
        source: &ManagedSource,
        recorded: &Recorded,
        now: DateTime<Utc>,
    ) -> Result<(), ManagedError> {
        let refused = |message: String| Err(ManagedError::Binding(message));
        if !source.matches(&self.source) {
            return refused(format!(
                "it was issued for the source {:?}, not the configured {:?}",
                self.source,
                source.as_str()
            ));
        }
        if let Some(subject) = &recorded.subject {
            if subject != &self.subject {
                return refused(format!(
                    "it was issued for the subject {:?}, not {subject:?}; run \
                     `fastskill managed enroll` again to follow another subject",
                    self.subject
                ));
            }
        }
        if let Some(highest) = recorded.highest_issued_at {
            if self.issued_at < highest {
                return refused(format!(
                    "it was issued at {}, before the state already accepted ({highest})",
                    self.issued_at
                ));
            }
        }
        if self.issued_at > now + MAX_CLOCK_SKEW {
            return refused(format!(
                "it was issued at {}, more than five minutes ahead of this machine's clock ({now})",
                self.issued_at
            ));
        }
        Ok(())
    }

    /// Whether the state has expired by the local clock.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now >= self.expires_at
    }

    /// The `editable` setting in force: `blocked-only` unless an `https://` source says
    /// `refused` (decisions 21 and 22).
    pub fn editable(&self, source: &ManagedSource) -> Editable {
        match (source, self.editable.as_deref().and_then(parse_editable)) {
            (ManagedSource::Https(_), Some(editable)) => editable,
            _ => Editable::BlockedOnly,
        }
    }

    /// The request link for a digest that isn't allowed, when an `https://` source names one on
    /// its own origin (decisions 6, 21 and 23).
    pub fn request_link(&self, source: &ManagedSource, digest: &str) -> Option<String> {
        let template = self.request_url.as_deref()?;
        let link = template.replace(DIGEST_PLACEHOLDER, &percent_encode(digest));
        source.same_origin(&link).then_some(link)
    }

    /// The report URL, when an `https://` source names one on its own origin (decisions 6, 15
    /// and 21).
    pub fn report_url(&self, source: &ManagedSource) -> Option<&str> {
        let url = self.report_url.as_deref()?;
        source.same_origin(url).then_some(url)
    }

    /// The message for a blocked digest, or `None` when it isn't blocked.
    pub fn blocked(&self, digest: &str) -> Option<&BlockedDigest> {
        self.blocked.iter().find(|blocked| blocked.digest == digest)
    }

    /// Whether `digest` may be installed, ignoring the `editable` exemption.
    pub fn allows(&self, digest: &str) -> bool {
        if self.blocked(digest).is_some() {
            return false;
        }
        match self.allowed {
            Allowed::Any => true,
            Allowed::Listed => {
                self.skills.iter().any(|skill| skill.digest == digest)
                    || self.also_allowed.iter().any(|allowed| allowed == digest)
            }
        }
    }
}

fn parse_editable(value: &str) -> Option<Editable> {
    match value {
        "blocked-only" => Some(Editable::BlockedOnly),
        "refused" => Some(Editable::Refused),
        _ => None,
    }
}

/// One path component under the skill id rules (ADR-0014): no scope, no separators.
fn is_safe_id(id: &str) -> bool {
    !id.contains('/') && SkillId::new(id.to_string()).is_ok()
}

fn check_digest(digest: &str) -> Result<(), ManagedError> {
    if is_current_digest(digest) {
        Ok(())
    } else {
        Err(ManagedError::Invalid(format!(
            "{digest:?} isn't a content digest in the current form (sha256-tree-v2)"
        )))
    }
}

fn check_https(field: &str, value: &str) -> Result<(), ManagedError> {
    match url::Url::parse(value) {
        Ok(url) if url.scheme() == "https" && url.host_str().is_some() => Ok(()),
        _ => Err(ManagedError::Invalid(format!(
            "{field} {value:?} isn't an https:// URL"
        ))),
    }
}

/// An artifact is an `https://` URL; a file source may also name a local path.
fn check_artifact(id: &str, artifact: &str, source: &ManagedSource) -> Result<(), ManagedError> {
    if artifact.contains("://") || matches!(source, ManagedSource::Https(_)) {
        return check_https(&format!("the artifact of {id:?}"), artifact);
    }
    if artifact.trim().is_empty() {
        return Err(ManagedError::Invalid(format!(
            "the artifact of {id:?} is empty"
        )));
    }
    Ok(())
}

/// Percent-encode everything but RFC 3986's unreserved characters.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

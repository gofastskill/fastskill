//! Managed state: a signed, resolved description of what one user's agents on one machine
//! should have, decided somewhere else and applied by FastSkill
//! ([ADR-0016](../../../../../docs/adr/0016-machines-follow-a-signed-managed-state.md)).
//!
//! This module holds what every later step relies on:
//!
//! - [`envelope`]: the DSSE envelope and its Ed25519 signatures, checked against keys pinned in
//!   trusted configuration (decision 2).
//! - [`state`]: the managed state format, the rules that refuse a whole state (decision 1), its
//!   binding to the configured source and the recorded subject, rollback and clock skew
//!   (decision 3), and expiry (decision 4).
//! - [`config`]: the managed settings, read only from the system file and the user's
//!   `managed.toml` (decision 7).
//!
//! - [`apply`]: applying a state to this user's Agent targets, with the [`store`], the
//!   [`quarantine`], the per-user [`layout`] and lock, and the [`records`] it keeps
//!   (decisions 8 to 12 and 17).
//!
//! - [`gate`]: the install gate, the two managed reconciliation statuses and what a command may
//!   do without a valid state (decisions 4, 13 and 14).
//!
//! [`open`] is the one entry point that turns envelope bytes into a state FastSkill may use.

pub mod apply;
pub mod config;
pub mod enrollment;
pub mod envelope;
pub mod gate;
pub mod layout;
pub mod quarantine;
pub mod records;
pub mod state;
pub mod store;

#[cfg(test)]
mod apply_tests;
#[cfg(test)]
mod tests;

pub use apply::{apply, ApplyContext, ApplyOutcome, ApplyResult, EntryChange};
pub use config::{ManagedSettings, ManagedSource, PinnedKey, SYSTEM_FILE_NAME, USER_FILE_NAME};
pub use enrollment::{enroll, status, unenroll, ManagedStatus, UnenrollOutcome};
pub use envelope::{verify_envelope, Verified, PAYLOAD_TYPE};
pub use gate::{Candidate, CommandPolicy, ManagedGate, ManagedSkillStatus, Situation};
pub use layout::{ApplyLock, ManagedLayout};
pub use quarantine::{QuarantineReason, QuarantineRecord};
pub use records::{Enrollment, EntryMode, OwnedEntry, Ownership};
pub use state::{
    Allowed, BlockedDigest, Editable, ManagedState, Recorded, StateSkill, FORMAT_VERSION,
    MAX_CLOCK_SKEW,
};

use chrono::{DateTime, Utc};

/// Why a managed state, or the settings that would read one, can't be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManagedError {
    /// The envelope isn't a well-formed DSSE envelope of a managed state.
    #[error("managed state envelope: {0}")]
    Envelope(String),
    /// No signature verifies against a pinned key.
    #[error("managed state is not signed by a pinned key: {0}")]
    Untrusted(String),
    /// The state breaks a rule that refuses it whole.
    #[error("managed state refused: {0}")]
    Invalid(String),
    /// The state is valid but not for this machine now: another source or subject, older than
    /// one already accepted, or issued in the future.
    #[error("managed state refused: {0}")]
    Binding(String),
    /// The managed settings can't be used.
    #[error("managed settings: {0}")]
    Config(String),
}

impl From<ManagedError> for crate::core::service::ServiceError {
    fn from(error: ManagedError) -> Self {
        match error {
            ManagedError::Config(_) => Self::Config(error.to_string()),
            _ => Self::Validation(error.to_string()),
        }
    }
}

/// A state that passed every check, and the pinned key that vouched for it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedState {
    pub state: ManagedState,
    pub key_id: String,
}

/// Verify `envelope` against the pinned keys, parse and validate the state it carries, and bind
/// it to the configured source and what this user already accepted.
///
/// An expired state still opens: FastSkill keeps using it to read skills and refuses only what
/// would add or change content (decision 4). Ask [`ManagedState::is_expired`].
pub fn open(
    envelope: &[u8],
    settings: &ManagedSettings,
    recorded: &Recorded,
    now: DateTime<Utc>,
) -> Result<OpenedState, ManagedError> {
    let source = settings
        .source
        .as_ref()
        .ok_or_else(|| ManagedError::Config("no managed source is configured".to_string()))?;
    let verified = verify_envelope(envelope, &settings.keys)?;
    let state = ManagedState::parse(&verified.payload)?;
    state.validate(source)?;
    state.check_binding(source, recorded, now)?;
    Ok(OpenedState {
        state,
        key_id: verified.key_id,
    })
}

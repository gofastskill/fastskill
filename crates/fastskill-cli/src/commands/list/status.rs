//! The reconciliation status vocabulary reported by `skill list`.
//!
//! `skill list --check` exits nonzero on any status other than `ok` or
//! `excluded`, and the serialized form is what JSON and XML consumers read, so
//! this vocabulary is a compatibility surface rather than an internal detail.
//! Holding it in one enum keeps that surface exhaustive: adding a state forces
//! the renderers and the `--check` gate to account for it instead of silently
//! treating it as a failure.

/// A single skill's status from comparing manifest (desired), lock (pinned) and
/// skills directory (actual).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReconciliationStatus {
    /// Desired, pinned and actual agree.
    Ok,
    /// Owned by a bundle or override that the current selection excludes.
    Excluded,
    /// Declared in the manifest with nothing pinned in the lock.
    MissingLock,
    /// Selected but not present in the skills directory.
    MissingContent,
    /// The manifest's origin no longer matches the origin the lock pinned.
    IntentMismatch,
    /// Installed version differs from the pinned version.
    RevisionMismatch,
    /// Installed content does not match the pinned checksum.
    ContentMismatch,
    /// The installed tree could not be digested to check it.
    IntegrityError,
    /// Nothing recorded a checksum, so content cannot be verified.
    InsufficientIntegrity,
    /// Several owners claim the skill with different content.
    OwnershipConflict,
    /// Installed but nothing selects it.
    Extraneous,
}

impl ReconciliationStatus {
    /// Every status, so tests can assert over the whole vocabulary.
    #[cfg(test)]
    pub(super) const ALL: [Self; 11] = [
        Self::Ok,
        Self::Excluded,
        Self::MissingLock,
        Self::MissingContent,
        Self::IntentMismatch,
        Self::RevisionMismatch,
        Self::ContentMismatch,
        Self::IntegrityError,
        Self::InsufficientIntegrity,
        Self::OwnershipConflict,
        Self::Extraneous,
    ];

    /// The wire form. `Serialize` and `Display` both route through this, so the
    /// table, JSON and XML renderings cannot drift apart.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Excluded => "excluded",
            Self::MissingLock => "missing-lock",
            Self::MissingContent => "missing-content",
            Self::IntentMismatch => "intent-mismatch",
            Self::RevisionMismatch => "revision-mismatch",
            Self::ContentMismatch => "content-mismatch",
            Self::IntegrityError => "integrity-error",
            Self::InsufficientIntegrity => "insufficient-integrity",
            Self::OwnershipConflict => "ownership-conflict",
            Self::Extraneous => "extraneous",
        }
    }

    /// Whether `skill list --check` accepts this status. `excluded` counts as
    /// settled because the skill is deliberately outside the current selection.
    pub(super) fn is_settled(self) -> bool {
        matches!(self, Self::Ok | Self::Excluded)
    }
}

impl std::fmt::Display for ReconciliationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for ReconciliationStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_status_renders_a_distinct_kebab_case_string() {
        let mut seen = std::collections::HashSet::new();
        for status in ReconciliationStatus::ALL {
            let rendered = status.as_str();
            assert!(
                seen.insert(rendered),
                "two statuses render as {rendered:?}; --check and the JSON \
                 output would conflate them"
            );
            assert!(
                rendered.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{rendered:?} is not kebab-case"
            );
        }
    }

    #[test]
    fn display_and_serialize_agree_with_as_str() {
        for status in ReconciliationStatus::ALL {
            assert_eq!(status.to_string(), status.as_str());
            assert_eq!(
                serde_json::to_string(&status).unwrap(),
                format!("\"{}\"", status.as_str())
            );
        }
    }

    #[test]
    fn only_ok_and_excluded_are_settled() {
        let settled: Vec<_> = ReconciliationStatus::ALL
            .into_iter()
            .filter(|status| status.is_settled())
            .collect();
        assert_eq!(
            settled,
            vec![ReconciliationStatus::Ok, ReconciliationStatus::Excluded]
        );
    }
}

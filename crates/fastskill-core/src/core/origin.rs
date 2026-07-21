//! `Origin` — the single canonical model of *where an installed skill came from*.
//!
//! `Origin` captures install **intent** (what the user asked for). The **resolved**
//! facts a fetch produces — the exact commit, the concrete version, a checksum —
//! live in [`Resolved`], which is stored only in the Lock, never in `Origin`.
//!
//! This type replaces the former six overlapping provenance representations
//! (`SkillSource` ×2, `SourceType`, `SourceSpecificFields`, the flat `source_*`
//! fields on the lock entries, and the nine on `SkillDefinition`). See
//! [ADR-0005](../../../../docs/adr/0005-install-seam-and-origin-model.md) and the
//! `Origin` entry in `CONTEXT.md`.

use crate::core::version::VersionConstraint;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where a single installed skill came from (install intent + persisted provenance).
///
/// Serialized internally-tagged by `type`, so a default-branch git origin is just
/// `{"type":"git","url":"…"}` — the `ref`/`subdir`/`version` fields are omitted when
/// unset, keeping the common "install latest" case minimal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Origin {
    /// A git repository at a ref (default branch unless pinned), optionally a subdir.
    Git {
        url: String,
        #[serde(default, skip_serializing_if = "GitRef::is_default")]
        r#ref: GitRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdir: Option<PathBuf>,
    },
    /// A path on the local filesystem — a directory or a `.zip` archive.
    /// `editable` (symlink-in-place) is only valid for a directory.
    Local {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "is_false")]
        editable: bool,
    },
    /// A remote zip archive fetched over HTTP(S).
    ZipUrl { url: String },
    /// A reference *into* a configured [`Repository`](crate::core::manifest). `repo`
    /// is the concrete Repository name; `version` is the only place ADR-0004
    /// versioning applies (`None` = newest allowed).
    Repository {
        repo: String,
        skill: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<VersionConstraint>,
    },
}

/// The git ref an [`Origin::Git`] points at. A sum type so illegal combinations
/// (a branch *and* a tag) are unrepresentable; `Default` means the repository's
/// default branch.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitRef {
    /// The repository's default branch (whatever `HEAD` points at on clone).
    #[default]
    Default,
    Branch(String),
    Tag(String),
    Commit(String),
}

impl GitRef {
    /// True for [`GitRef::Default`]; drives `skip_serializing_if` so the ref field
    /// is omitted entirely for the common default-branch case.
    pub fn is_default(&self) -> bool {
        matches!(self, GitRef::Default)
    }
}

/// The concrete facts a fetch resolved an [`Origin`] to. Stored only in the Lock —
/// never in `Origin` — so "what was asked for" stays separate from "what it became".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    /// The concrete version installed (`SKILL.md` version, or the registry version).
    pub version: String,
    /// The exact git commit, when the origin is a git clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    /// Content checksum, when the fetch produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn roundtrip(o: &Origin) -> Origin {
        let json = serde_json::to_string(o).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn git_latest_is_minimal() {
        let o = Origin::Git {
            url: "https://github.com/x/y".into(),
            r#ref: GitRef::Default,
            subdir: None,
        };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(json, r#"{"type":"git","url":"https://github.com/x/y"}"#);
        assert_eq!(roundtrip(&o), o);
    }

    #[test]
    fn git_branch_ref_is_subobject() {
        let o = Origin::Git {
            url: "u".into(),
            r#ref: GitRef::Branch("main".into()),
            subdir: Some(PathBuf::from("sub")),
        };
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains(r#""ref":{"branch":"main"}"#), "{json}");
        assert!(json.contains(r#""subdir":"sub""#), "{json}");
        assert_eq!(roundtrip(&o), o);
    }

    #[test]
    fn local_dir_omits_editable_when_false() {
        let o = Origin::Local {
            path: PathBuf::from("/tmp/s"),
            editable: false,
        };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(json, r#"{"type":"local","path":"/tmp/s"}"#);
        assert_eq!(roundtrip(&o), o);
    }

    #[test]
    fn zip_url_roundtrips() {
        let o = Origin::ZipUrl {
            url: "https://x/y.zip".into(),
        };
        assert_eq!(roundtrip(&o), o);
    }

    #[test]
    fn repository_latest_omits_version() {
        let o = Origin::Repository {
            repo: "main-registry".into(),
            skill: "acme/widget".into(),
            version: None,
        };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(
            json,
            r#"{"type":"repository","repo":"main-registry","skill":"acme/widget"}"#
        );
        assert_eq!(roundtrip(&o), o);
    }

    #[test]
    fn repository_bare_version_normalizes_to_exact_pin() {
        // ADR-0004 enforced at the serde boundary via VersionConstraint.
        let o = Origin::Repository {
            repo: "r".into(),
            skill: "s".into(),
            version: Some(VersionConstraint::parse("1.2.3").unwrap()),
        };
        let json = serde_json::to_string(&o).unwrap();
        assert!(json.contains(r#""version":"=1.2.3""#), "{json}");
        assert_eq!(roundtrip(&o), o);
    }

    #[test]
    fn resolved_omits_empty_optionals() {
        let r = Resolved {
            version: "1.0.0".into(),
            commit_hash: None,
            checksum: None,
        };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"version":"1.0.0"}"#);
    }
}

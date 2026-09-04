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
use std::path::{Component, Path, PathBuf};

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

/// A local path that could not be made portable: it lives outside the project
/// tree, so no path relative to the Manifest can name it on another machine.
/// Returned by [`Origin::to_manifest_relative`] so the caller can warn, naming
/// the Manifest field it is about to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnportableLocalPath {
    /// The absolute path that will be persisted as-is.
    pub path: PathBuf,
    /// The Manifest directory it is not reachable from.
    pub manifest_dir: PathBuf,
}

impl UnportableLocalPath {
    /// The warning to show a user, naming the Manifest field being written so
    /// the message points at the thing that will fail rather than at a later,
    /// unrelated-looking install error. `field_owner` is the dependency id.
    pub fn warning(&self, skill_id: &str) -> String {
        format!(
            "dependencies.{skill_id}.origin.path is outside the project ({}), so it is \
             recorded as the absolute path {} — this Manifest will not resolve on another \
             machine or in CI. Move the skill inside the project to make it portable.",
            self.manifest_dir.display(),
            self.path.display()
        )
    }
}

impl Origin {
    /// The form of this origin to **write** into a Manifest or Lock living in
    /// `manifest_dir`.
    ///
    /// A local path inside the project tree becomes relative to `manifest_dir`,
    /// so a committed `skill-project.toml` + `skills.lock` names the same skill
    /// on every checkout (README's "every teammate and CI gets the identical
    /// skill set"). A path outside that tree is left absolute — nothing can make
    /// it portable — and reported back as [`UnportableLocalPath`] so the caller
    /// warns instead of silently writing a machine-local path. Every other
    /// variant is already location-independent and passes through untouched.
    pub fn to_manifest_relative(
        &self,
        manifest_dir: &Path,
    ) -> (Origin, Option<UnportableLocalPath>) {
        let Origin::Local { path, editable } = self else {
            return (self.clone(), None);
        };
        // Already relative: by construction it is read back against the Manifest
        // directory, so it is already the portable form.
        if path.is_relative() {
            return (self.clone(), None);
        }

        let base = canonical_or_owned(manifest_dir);
        let target = canonical_or_owned(path);
        match target.strip_prefix(&base) {
            Ok(relative) => {
                // An empty remainder means the target *is* the project root;
                // `.` names it relatively.
                let relative = if relative.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    relative.to_path_buf()
                };
                (
                    Origin::Local {
                        path: relative,
                        editable: *editable,
                    },
                    None,
                )
            }
            Err(_) => (
                Origin::Local {
                    path: target.clone(),
                    editable: *editable,
                },
                Some(UnportableLocalPath {
                    path: target,
                    manifest_dir: base,
                }),
            ),
        }
    }

    /// The form of this origin to **use** after reading it from a Manifest or
    /// Lock that lives in `manifest_dir`: a relative local path is resolved
    /// against that directory — never against the process's current directory,
    /// which has nothing to do with where the Manifest is.
    ///
    /// An absolute path is returned unchanged, so Manifests written before local
    /// paths were relativized keep working.
    pub fn resolved_against(&self, manifest_dir: &Path) -> Origin {
        match self {
            Origin::Local { path, editable } if path.is_relative() => Origin::Local {
                path: lexical_join(manifest_dir, path),
                editable: *editable,
            },
            other => other.clone(),
        }
    }
}

/// Canonicalize as much of `path` as exists on disk, then re-append the part
/// that does not. Both sides of the relative/absolute comparison go through
/// this so a symlinked project root (`/tmp` on macOS, a symlinked checkout)
/// does not defeat the prefix test.
///
/// Canonicalizing the *whole* path is not enough: `canonicalize` fails outright
/// when the leaf is missing, and a Manifest routinely names a local skill that
/// has not been fetched yet. The fallback then left one side of the comparison
/// canonical and the other raw, and on Windows those two forms never share a
/// prefix — `\\?\C:\Users\runneradmin\...` against `C:\Users\RUNNER~1\...`
/// (verbatim prefix, 8.3 short name) — so an in-tree path was misjudged
/// unportable and recorded absolute, which is the very defect this seam exists
/// to prevent. Walking up to the nearest existing ancestor keeps both sides in
/// the same form regardless of whether the leaf is there yet.
fn canonical_or_owned(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    // Peel off trailing components until something canonicalizes, then put
    // them back. `suffix` is built leaf-first, so it is replayed in reverse.
    let mut suffix: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor = path;
    while let (Some(parent), Some(name)) = (cursor.parent(), cursor.file_name()) {
        suffix.push(name);
        if let Ok(mut canonical) = parent.canonicalize() {
            for component in suffix.iter().rev() {
                canonical.push(component);
            }
            return canonical;
        }
        cursor = parent;
    }
    path.to_path_buf()
}

/// Join `relative` onto `base` without touching the filesystem, folding away
/// `.` and `..` components so the result is a clean path to show and to open.
fn lexical_join(base: &Path, relative: &Path) -> PathBuf {
    let mut joined = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                joined.pop();
            }
            other => joined.push(other.as_os_str()),
        }
    }
    joined
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
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

    // ── manifest-relative local paths ──────────────────────────────────────

    fn local(path: &str) -> Origin {
        Origin::Local {
            path: PathBuf::from(path),
            editable: false,
        }
    }

    fn local_path(origin: &Origin) -> PathBuf {
        match origin {
            Origin::Local { path, .. } => path.clone(),
            other => panic!("expected Local, got {other:?}"),
        }
    }

    #[test]
    fn in_tree_absolute_path_is_written_relative() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("src/alpha-skill")).unwrap();

        let (origin, unportable) =
            local(root.join("src/alpha-skill").to_str().unwrap()).to_manifest_relative(&root);

        assert_eq!(local_path(&origin), PathBuf::from("src/alpha-skill"));
        assert!(unportable.is_none());
    }

    #[test]
    fn editable_flag_survives_relativization() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("skills/x")).unwrap();

        let origin = Origin::Local {
            path: root.join("skills/x"),
            editable: true,
        };
        let (rewritten, _) = origin.to_manifest_relative(&root);
        assert_eq!(
            rewritten,
            Origin::Local {
                path: PathBuf::from("skills/x"),
                editable: true,
            }
        );
    }

    #[test]
    fn out_of_tree_path_stays_absolute_and_is_reported() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        let elsewhere = tempfile::TempDir::new().unwrap();
        let elsewhere = elsewhere.path().canonicalize().unwrap().join("beta");
        std::fs::create_dir_all(&elsewhere).unwrap();

        let (origin, unportable) = local(elsewhere.to_str().unwrap()).to_manifest_relative(&root);

        assert_eq!(local_path(&origin), elsewhere);
        let unportable = unportable.expect("out-of-tree path must be reported");
        let warning = unportable.warning("beta-skill");
        assert!(
            warning.contains("dependencies.beta-skill.origin.path"),
            "warning must name the manifest field: {warning}"
        );
    }

    #[test]
    fn sibling_directory_with_shared_prefix_is_not_in_tree() {
        // `/x/proj-extra` must not be seen as living inside `/x/proj` — the
        // prefix test is per-component, not per-byte.
        let base = tempfile::TempDir::new().unwrap();
        let base = base.path().canonicalize().unwrap();
        let root = base.join("proj");
        let sibling = base.join("proj-extra/skill");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();

        let (origin, unportable) = local(sibling.to_str().unwrap()).to_manifest_relative(&root);
        assert_eq!(local_path(&origin), sibling);
        assert!(unportable.is_some());
    }

    #[test]
    fn already_relative_path_is_left_alone() {
        let (origin, unportable) =
            local("src/alpha-skill").to_manifest_relative(Path::new("/some/project"));
        assert_eq!(local_path(&origin), PathBuf::from("src/alpha-skill"));
        assert!(unportable.is_none());
    }

    #[test]
    fn non_local_origins_pass_through_untouched() {
        let git = Origin::Git {
            url: "https://github.com/x/y".into(),
            r#ref: GitRef::Default,
            subdir: None,
        };
        let (rewritten, unportable) = git.to_manifest_relative(Path::new("/some/project"));
        assert_eq!(rewritten, git);
        assert!(unportable.is_none());
        assert_eq!(git.resolved_against(Path::new("/other")), git);
    }

    #[test]
    fn relative_path_resolves_against_the_manifest_directory() {
        let resolved = local("src/alpha-skill").resolved_against(Path::new("/home/teammate/proj"));
        assert_eq!(
            local_path(&resolved),
            PathBuf::from("/home/teammate/proj/src/alpha-skill")
        );
    }

    #[test]
    fn dot_segments_are_folded_when_resolving() {
        let resolved = local("./src/alpha-skill").resolved_against(Path::new("/proj"));
        assert_eq!(
            local_path(&resolved),
            PathBuf::from("/proj/src/alpha-skill")
        );

        let up = local("../shared/skill").resolved_against(Path::new("/proj/nested"));
        assert_eq!(local_path(&up), PathBuf::from("/proj/shared/skill"));
    }

    #[test]
    fn absolute_path_is_unchanged_on_read() {
        // Back-compat: a Manifest written before relativization still works.
        let resolved = local("/opt/skills/legacy").resolved_against(Path::new("/proj"));
        assert_eq!(local_path(&resolved), PathBuf::from("/opt/skills/legacy"));
    }

    #[test]
    fn write_then_read_round_trips_to_the_original_location() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        let skill = root.join("src/alpha-skill");
        std::fs::create_dir_all(&skill).unwrap();

        let (written, _) = local(skill.to_str().unwrap()).to_manifest_relative(&root);
        // A different checkout of the same committed project.
        let elsewhere = Path::new("/home/teammate/checkout");
        assert_eq!(
            local_path(&written.resolved_against(elsewhere)),
            elsewhere.join("src/alpha-skill")
        );
    }

    #[test]
    fn in_tree_path_relativizes_before_the_skill_exists() {
        let root = tempfile::TempDir::new().unwrap();
        let root = root.path().canonicalize().unwrap();
        // Deliberately never created. A Manifest routinely names a skill that
        // has not been fetched into the tree yet; that must not change whether
        // the path is judged portable.
        let skill = root.join("local-skill");

        let (written, unportable) = local(skill.to_str().unwrap()).to_manifest_relative(&root);

        assert_eq!(local_path(&written), PathBuf::from("local-skill"));
        assert_eq!(unportable, None);
    }

    /// The load-bearing one: `manifest_dir` canonicalizes to something other
    /// than the string it was given, *and* the target leaf is missing. That is
    /// the shape Windows CI hits -- the manifest dir canonicalizes to
    /// `\\?\C:\Users\runneradmin\...` while the joined target keeps the 8.3
    /// short name `C:\Users\RUNNER~1\...` -- and it is reachable on Unix
    /// through a symlinked project root. Canonicalizing only the side that
    /// exists leaves the two in different forms, so the prefix test fails and
    /// an in-tree path is recorded absolute.
    #[cfg(unix)]
    #[test]
    fn in_tree_path_relativizes_through_a_symlinked_project_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let tmp = tmp.path().canonicalize().unwrap();
        let real = tmp.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = tmp.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let target = link.join("local-skill");
        let (written, unportable) = local(target.to_str().unwrap()).to_manifest_relative(&link);

        assert_eq!(local_path(&written), PathBuf::from("local-skill"));
        assert_eq!(
            unportable, None,
            "an in-tree path under a symlinked project root was reported unportable"
        );
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

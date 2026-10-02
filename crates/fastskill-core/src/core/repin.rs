//! `project repin`: replace the legacy content digests a project's records still hold with
//! the current form, computed from the same installed content. See the "Retiring legacy
//! digests" amendment of
//! [ADR-0017](../../../../docs/adr/0017-versioned-content-digests.md).
//!
//! Re-pinning never changes content and never re-resolves anything. A legacy value is
//! rewritten only after the installed content is checked against it; a value that does not
//! match, or whose content is not installed, is left as it is and reported. Bundle
//! artifacts are immutable, so an artifact that declares legacy digests is only reported.
//!
//! Values are rewritten in place with `toml_edit`, so every other byte of each record,
//! including the order of entries and the version that last wrote it, is kept.

use crate::core::bundle::{
    BundleArchiveLockMember, BundleDescriptor, BundleManifestTables, PreparedBundle, BUNDLE_FORMAT,
    BUNDLE_HISTORY_FILE, BUNDLE_STATE_DIRECTORY,
};
use crate::core::bundle_archive::artifact_declares_legacy_digests;
use crate::core::bundle_persistence::{digest_release, BundleHistory};
use crate::core::content_digest::{content_digest, is_legacy_digest, legacy_content_digest};
use crate::core::lock::{GlobalSkillsLock, ProjectLockedBundleEntry, ProjectSkillsLock};
use crate::core::service::{ServiceError, SkillId};
use crate::core::state_guard::StateMutationGuard;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, TableLike, Value};

const OPERATION: &str = "repin content digests";
const PROJECT_FILE: &str = "skill-project.toml";
const PROJECT_LOCK_FILE: &str = "skills.lock";

/// The kind of record a legacy value was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepinRecord {
    /// A `[[skills]]` checksum in `skills.lock` or `global-skills.lock`.
    Skill,
    /// A `[[bundles]]` release digest and its member digests in `skills.lock`.
    Bundle,
    /// An `[[overrides]]` digest in `skills.lock`.
    Override,
    /// A release recorded in `.fastskill/bundle-history.toml`.
    BundleHistory,
}

impl RepinRecord {
    /// A short label for messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Bundle => "bundle",
            Self::Override => "override",
            Self::BundleHistory => "bundle history",
        }
    }
}

/// A bundle member digest that re-pinning a bundle entry replaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepinMemberDigest {
    pub id: String,
    pub legacy: String,
    pub current: String,
}

/// One recorded legacy value, and what re-pinning does with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepinEntry {
    pub record: RepinRecord,
    /// The skill, override or bundle id; `id@version` for a bundle history release.
    pub id: String,
    /// The bundle version, for a bundle entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The file that records the value.
    pub file: PathBuf,
    /// The recorded legacy value.
    pub legacy: String,
    /// The current-form value that replaces it, when the installed content matched.
    pub current: Option<String>,
    /// Why the value is left as it is, when it is.
    pub reason: Option<String>,
    /// The member digests a bundle entry's re-pin also replaces.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<RepinMemberDigest>,
}

/// What `repin_project` or `repin_global` found, and whether it wrote the re-pinned values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RepinReport {
    pub entries: Vec<RepinEntry>,
    /// Bundle artifacts that declare legacy digests. They are immutable; `bundle build`
    /// produces a current one.
    pub legacy_artifacts: Vec<PathBuf>,
    /// Whether the re-pinned values were written.
    pub written: bool,
}

impl RepinReport {
    /// Entries whose installed content matched, so their value is (or would be) re-pinned.
    pub fn repinnable(&self) -> impl Iterator<Item = &RepinEntry> {
        self.entries.iter().filter(|entry| entry.current.is_some())
    }

    /// Entries left as they are, each with its reason.
    pub fn retained(&self) -> impl Iterator<Item = &RepinEntry> {
        self.entries.iter().filter(|entry| entry.current.is_none())
    }

    /// How many legacy values remain in the records and artifacts after this run.
    pub fn legacy_remaining(&self) -> usize {
        let unwritten = if self.written {
            0
        } else {
            self.repinnable().count()
        };
        self.retained().count() + self.legacy_artifacts.len() + unwritten
    }
}

/// Re-pin the legacy digests of the project at `project_root`, whose skills are installed
/// in `skills_directory`. With `check`, nothing is written.
pub fn repin_project(
    project_root: &Path,
    skills_directory: &Path,
    check: bool,
) -> Result<RepinReport, ServiceError> {
    let report = plan_project(project_root, skills_directory)?;
    if check || report.repinnable().next().is_none() {
        return Ok(report);
    }
    let guard = StateMutationGuard::acquire_for(project_root, Some(skills_directory), OPERATION)?;
    // Plan again under the lease, so the values written are the ones on disk now.
    let outcome = plan_project(project_root, skills_directory).and_then(write);
    finish(guard, outcome)
}

/// Re-pin the legacy checksums in the global Lock at `lock_path`, whose skills are installed
/// in `skills_directory`. With `check`, nothing is written.
pub fn repin_global(
    lock_path: &Path,
    skills_directory: &Path,
    check: bool,
) -> Result<RepinReport, ServiceError> {
    let report = plan_global(lock_path, skills_directory)?;
    if check || report.repinnable().next().is_none() {
        return Ok(report);
    }
    let state_root = lock_path.parent().ok_or_else(|| {
        ServiceError::Config(format!("{} has no parent directory", lock_path.display()))
    })?;
    let guard = StateMutationGuard::acquire_for(state_root, Some(skills_directory), OPERATION)?;
    let outcome = plan_global(lock_path, skills_directory).and_then(write);
    finish(guard, outcome)
}

fn finish(
    guard: StateMutationGuard,
    outcome: Result<RepinReport, ServiceError>,
) -> Result<RepinReport, ServiceError> {
    match outcome {
        Ok(report) => {
            guard.commit()?;
            Ok(report)
        }
        Err(error) => {
            // Each file is replaced atomically, and a re-pinned value names the same
            // content as the value it replaced, so every state left behind is valid.
            guard.recovered()?;
            Err(error)
        }
    }
}

fn plan_project(project_root: &Path, skills: &Path) -> Result<RepinReport, ServiceError> {
    let lock_path = project_root.join(PROJECT_LOCK_FILE);
    let lock = if lock_path.exists() {
        Some(
            ProjectSkillsLock::load_from_file(&lock_path).map_err(|error| {
                ServiceError::Config(format!("Failed to read {}: {error}", lock_path.display()))
            })?,
        )
    } else {
        None
    };
    let mut entries = Vec::new();
    if let Some(lock) = &lock {
        for skill in &lock.skills {
            if let Some(checksum) = legacy(skill.resolved.checksum.as_deref()) {
                let outcome = check_installed(checksum, skills, &skill.id);
                entries.push(entry(
                    RepinRecord::Skill,
                    &skill.id,
                    &lock_path,
                    checksum,
                    outcome,
                ));
            }
        }
        for bundle in &lock.bundles {
            if let Some(planned) = plan_bundle(project_root, skills, lock, bundle, &lock_path) {
                entries.push(planned);
            }
        }
        for local in &lock.overrides {
            if let Some(digest) = legacy(Some(&local.digest)) {
                let outcome = check_installed(digest, skills, &local.id);
                entries.push(entry(
                    RepinRecord::Override,
                    &local.id,
                    &lock_path,
                    digest,
                    outcome,
                ));
            }
        }
    }
    let history = plan_history(project_root, &entries)?;
    entries.extend(history);
    Ok(RepinReport {
        entries,
        legacy_artifacts: legacy_artifacts(project_root, lock.as_ref())?,
        written: false,
    })
}

fn plan_global(lock_path: &Path, skills: &Path) -> Result<RepinReport, ServiceError> {
    if !lock_path.exists() {
        return Ok(RepinReport::default());
    }
    let lock = GlobalSkillsLock::load_from_file(lock_path).map_err(|error| {
        ServiceError::Config(format!("Failed to read {}: {error}", lock_path.display()))
    })?;
    let entries = lock
        .skills
        .iter()
        .filter_map(|skill| {
            let checksum = legacy(skill.resolved.checksum.as_deref())?;
            let outcome = check_installed(checksum, skills, &skill.id);
            Some(entry(
                RepinRecord::Skill,
                &skill.id,
                lock_path,
                checksum,
                outcome,
            ))
        })
        .collect();
    Ok(RepinReport {
        entries,
        legacy_artifacts: Vec::new(),
        written: false,
    })
}

fn legacy(value: Option<&str>) -> Option<&str> {
    value.filter(|value| is_legacy_digest(value))
}

fn entry(
    record: RepinRecord,
    id: &str,
    file: &Path,
    legacy: &str,
    outcome: Result<String, String>,
) -> RepinEntry {
    let (current, reason) = match outcome {
        Ok(current) => (Some(current), None),
        Err(reason) => (None, Some(reason)),
    };
    RepinEntry {
        record,
        id: id.to_string(),
        version: None,
        file: file.to_path_buf(),
        legacy: legacy.to_string(),
        current,
        reason,
        members: Vec::new(),
    }
}

/// The current-form digest of the skill `id` installed under `skills`, when its content
/// matches `legacy`; otherwise why it is left as it is.
fn check_installed(legacy: &str, skills: &Path, id: &str) -> Result<String, String> {
    SkillId::new(id.to_string()).map_err(|_| format!("'{id}' is not a valid skill id"))?;
    check_content(legacy, &skills.join(id))
}

fn check_content(legacy: &str, directory: &Path) -> Result<String, String> {
    if !directory.is_dir() {
        return Err("content is not installed".to_string());
    }
    let unreadable = |error: ServiceError| format!("content could not be read: {error}");
    if legacy_content_digest(directory).map_err(unreadable)? != legacy {
        return Err("installed content does not match the legacy digest".to_string());
    }
    content_digest(directory).map_err(unreadable)
}

/// A bundle entry is re-pinned as a whole: its release digest hashes the member digests,
/// so it changes when they do. It is re-pinned only when the recorded release digest is the
/// one its recorded members produce, and every legacy member matches its content.
fn plan_bundle(
    project_root: &Path,
    skills: &Path,
    lock: &ProjectSkillsLock,
    bundle: &ProjectLockedBundleEntry,
    lock_path: &Path,
) -> Option<RepinEntry> {
    if !bundle
        .members
        .iter()
        .any(|member| is_legacy_digest(&member.digest))
    {
        return None;
    }
    let mut planned = entry(
        RepinRecord::Bundle,
        &bundle.id,
        lock_path,
        &bundle.digest,
        Err(String::new()),
    );
    planned.version = Some(bundle.version.clone());
    let descriptor = BundleDescriptor {
        format_marker: BUNDLE_FORMAT.to_string(),
        id: bundle.id.clone(),
        version: bundle.version.clone(),
        members: BTreeMap::new(),
    };
    let mut members: BTreeMap<String, BundleArchiveLockMember> = bundle
        .members
        .iter()
        .map(|member| {
            let digest = member.digest.clone();
            let overridable = member.overridable;
            (
                member.id.clone(),
                BundleArchiveLockMember {
                    digest,
                    overridable,
                },
            )
        })
        .collect();
    if digest_release(&descriptor, &members) != bundle.digest {
        planned.reason = Some("release digest does not match its member digests".to_string());
        return Some(planned);
    }
    let mut cached: Option<Result<PreparedBundle, String>> = None;
    for member in bundle
        .members
        .iter()
        .filter(|member| is_legacy_digest(&member.digest))
    {
        let outcome = if lock.overrides.iter().any(|local| local.id == member.id) {
            // The installed content is the override's, so check the packaged content the
            // member digest names, from the cached artifact.
            let prepared = cached.get_or_insert_with(|| {
                PreparedBundle::load(&project_root.join(&bundle.artifact))
                    .map_err(|error| format!("cached artifact could not be read: {error}"))
            });
            packaged_digest(prepared, &member.id, &member.digest)
        } else {
            check_installed(&member.digest, skills, &member.id)
        };
        match outcome {
            Ok(current) => {
                if let Some(locked) = members.get_mut(&member.id) {
                    locked.digest = current.clone();
                }
                planned.members.push(RepinMemberDigest {
                    id: member.id.clone(),
                    legacy: member.digest.clone(),
                    current,
                });
            }
            Err(reason) => {
                planned.members.clear();
                planned.reason = Some(format!("member '{}': {reason}", member.id));
                return Some(planned);
            }
        }
    }
    planned.reason = None;
    planned.current = Some(digest_release(&descriptor, &members));
    Some(planned)
}

fn packaged_digest(
    prepared: &Result<PreparedBundle, String>,
    id: &str,
    legacy: &str,
) -> Result<String, String> {
    let prepared = prepared.as_ref().map_err(|reason| {
        format!("the member is overridden, so its packaged content is checked instead, but the {reason}")
    })?;
    match prepared.members.get(id) {
        Some(member) if member.digest.legacy == legacy => Ok(member.digest.current.clone()),
        _ => Err(
            "the member is overridden, and its packaged content in the cached artifact does \
             not match the legacy digest"
                .to_string(),
        ),
    }
}

/// A bundle history release digest is bare hex in either form, so its form is decided by
/// the release it names: by the cached artifact of that release when it is present, or
/// else by the Lock entry for the same release when that entry recorded the same value.
/// A value neither can classify is left alone and not reported.
fn plan_history(
    project_root: &Path,
    planned: &[RepinEntry],
) -> Result<Vec<RepinEntry>, ServiceError> {
    let path = project_root.join(BUNDLE_HISTORY_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)?;
    let history: BundleHistory = toml::from_str(&content)
        .map_err(|error| ServiceError::Config(format!("Invalid {}: {error}", path.display())))?;
    let mut entries = Vec::new();
    for (release, value) in &history.releases {
        let Some((id, version)) = release.rsplit_once('@') else {
            continue;
        };
        if SkillId::new(id.to_string()).is_err() || semver::Version::parse(version).is_err() {
            continue;
        }
        let artifact = project_root
            .join(BUNDLE_STATE_DIRECTORY)
            .join(format!("{id}-{version}.zip"));
        let outcome = match PreparedBundle::load(&artifact) {
            Ok(prepared)
                if prepared.release_digest.legacy == *value
                    && prepared.release_digest.current != *value =>
            {
                Ok(prepared.release_digest.current.clone())
            }
            Ok(_) => continue,
            Err(_) => {
                let Some(locked) = planned.iter().find(|entry| {
                    entry.record == RepinRecord::Bundle
                        && entry.id == id
                        && entry.version.as_deref() == Some(version)
                        && entry.legacy == *value
                }) else {
                    continue;
                };
                locked.current.clone().ok_or_else(|| {
                    "the cached artifact is missing and the bundle's Lock entry is left as is"
                        .to_string()
                })
            }
        };
        entries.push(entry(
            RepinRecord::BundleHistory,
            release,
            &path,
            value,
            outcome,
        ));
    }
    Ok(entries)
}

/// Bundle artifacts the project refers to or caches that declare legacy digests: each
/// `*.zip` under `.fastskill/bundles/`, each Lock entry's artifact, and each artifact the
/// Manifest declares. An artifact that cannot be read is not reported here; commands that
/// use it report the problem.
fn legacy_artifacts(
    project_root: &Path,
    lock: Option<&ProjectSkillsLock>,
) -> Result<Vec<PathBuf>, ServiceError> {
    let mut candidates = Vec::new();
    let cache = project_root.join(BUNDLE_STATE_DIRECTORY);
    if cache.is_dir() {
        let mut cached: Vec<PathBuf> = fs::read_dir(&cache)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "zip"))
            .collect();
        cached.sort();
        candidates.extend(cached);
    }
    if let Some(lock) = lock {
        candidates.extend(
            lock.bundles
                .iter()
                .map(|bundle| project_root.join(&bundle.artifact)),
        );
    }
    let manifest = project_root.join(PROJECT_FILE);
    if manifest.is_file() {
        let tables: BundleManifestTables = toml::from_str(&fs::read_to_string(&manifest)?)
            .map_err(|error| {
                ServiceError::Config(format!("Invalid {}: {error}", manifest.display()))
            })?;
        candidates.extend(
            tables
                .bundles
                .values()
                .map(|dependency| project_root.join(&dependency.artifact)),
        );
    }
    let mut seen = BTreeSet::new();
    let mut legacy = Vec::new();
    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        let canonical = fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
        if !seen.insert(canonical) {
            continue;
        }
        if artifact_declares_legacy_digests(&candidate).unwrap_or(false) {
            let shown = candidate.strip_prefix(project_root).unwrap_or(&candidate);
            legacy.push(shown.to_path_buf());
        }
    }
    Ok(legacy)
}

/// Write every re-pinnable value into its file, one atomic replacement per file.
fn write(mut report: RepinReport) -> Result<RepinReport, ServiceError> {
    let mut files: BTreeMap<&Path, Vec<Edit<'_>>> = BTreeMap::new();
    for entry in report.repinnable() {
        files
            .entry(entry.file.as_path())
            .or_default()
            .extend(edits(entry));
    }
    for (file, edits) in files {
        let mut document: DocumentMut = fs::read_to_string(file)?.parse().map_err(|error| {
            ServiceError::Config(format!("Failed to parse {}: {error}", file.display()))
        })?;
        for edit in edits {
            if !set_string(
                document.as_table_mut(),
                &edit.steps,
                edit.expected,
                edit.replacement,
            ) {
                return Err(ServiceError::InvalidOperation(format!(
                    "{} changed while its digests were re-pinned; run the command again",
                    file.display()
                )));
            }
        }
        crate::utils::atomic_write(file, document.to_string().as_bytes())?;
    }
    report.written = true;
    Ok(report)
}

/// One step on the path to a value: a table key, or the entry of an array of tables whose
/// `id` is the given value.
enum Step<'a> {
    Key(&'a str),
    Entry(&'static str, &'a str),
}

struct Edit<'a> {
    steps: Vec<Step<'a>>,
    expected: &'a str,
    replacement: &'a str,
}

fn edits(entry: &RepinEntry) -> Vec<Edit<'_>> {
    let Some(current) = entry.current.as_deref() else {
        return Vec::new();
    };
    let edit = |steps, expected, replacement| Edit {
        steps,
        expected,
        replacement,
    };
    let id = entry.id.as_str();
    match entry.record {
        RepinRecord::Skill => vec![edit(
            vec![
                Step::Entry("skills", id),
                Step::Key("resolved"),
                Step::Key("checksum"),
            ],
            &entry.legacy,
            current,
        )],
        RepinRecord::Override => vec![edit(
            vec![Step::Entry("overrides", id), Step::Key("digest")],
            &entry.legacy,
            current,
        )],
        RepinRecord::BundleHistory => vec![edit(
            vec![Step::Key("releases"), Step::Key(id)],
            &entry.legacy,
            current,
        )],
        RepinRecord::Bundle => {
            let mut edits = vec![edit(
                vec![Step::Entry("bundles", id), Step::Key("digest")],
                &entry.legacy,
                current,
            )];
            edits.extend(entry.members.iter().map(|member| {
                edit(
                    vec![
                        Step::Entry("bundles", id),
                        Step::Entry("members", &member.id),
                        Step::Key("digest"),
                    ],
                    &member.legacy,
                    &member.current,
                )
            }));
            edits
        }
    }
}

/// Replace the string at `steps` with `replacement` when it is `expected`, keeping its
/// surrounding whitespace and comments. Returns whether the value was found and replaced.
fn set_string(
    table: &mut dyn TableLike,
    steps: &[Step<'_>],
    expected: &str,
    replacement: &str,
) -> bool {
    match steps {
        [Step::Key(key)] => {
            let Some(value) = table.get_mut(key).and_then(Item::as_value_mut) else {
                return false;
            };
            if value.as_str() != Some(expected) {
                return false;
            }
            let decor = value.decor().clone();
            *value = Value::from(replacement);
            *value.decor_mut() = decor;
            true
        }
        [Step::Key(key), rest @ ..] => table
            .get_mut(key)
            .and_then(Item::as_table_like_mut)
            .is_some_and(|inner| set_string(inner, rest, expected, replacement)),
        [Step::Entry(key, id), rest @ ..] => match table.get_mut(key) {
            Some(Item::ArrayOfTables(array)) => array
                .iter_mut()
                .find(|inner| inner.get("id").and_then(Item::as_str) == Some(*id))
                .is_some_and(|inner| set_string(inner, rest, expected, replacement)),
            Some(Item::Value(Value::Array(array))) => array
                .iter_mut()
                .filter_map(Value::as_inline_table_mut)
                .find(|inner| inner.get("id").and_then(Value::as_str) == Some(*id))
                .is_some_and(|inner| set_string(inner, rest, expected, replacement)),
            _ => false,
        },
        [] => false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "repin_tests.rs"]
mod tests;

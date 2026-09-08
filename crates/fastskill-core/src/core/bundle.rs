//! Self-contained, versioned bundle lifecycle.
//!
//! A bundle is deliberately handled as one core operation. The command layer
//! supplies paths and reports the outcome; this module validates the complete
//! archive, plans all member changes, and persists the Manifest and Lock with
//! the installed membership.

use crate::core::bundle_archive::{write_bundle_archive, BundleArchiveLock};
use crate::core::bundle_persistence::{
    apply_personal_override, digest_directory, parse_bundle_descriptor, parse_bundle_project,
    prepare_members, remove_skill_directory, replace_skill_directory, restore_personal_overrides,
    save_bundle_declarations, BundleHistory, BundleOverrideDeclaration, BundleTransaction,
};
use crate::core::lock::{ProjectLockedBundleEntry, ProjectLockedBundleMember, ProjectSkillsLock};
use crate::core::manifest::SkillProjectToml;
use crate::core::service::{ServiceError, SkillId};
use crate::storage::zip::ZipHandler;
use crate::utils::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// The marker that prevents a bundle archive from being mistaken for an
/// existing single-skill ZIP.
pub const BUNDLE_FORMAT: &str = "fastskill-bundle-v1";
const BUNDLE_STATE_DIRECTORY: &str = ".fastskill/bundles";
const BUNDLE_HISTORY_FILE: &str = ".fastskill/bundle-history.toml";

/// Result of creating a portable bundle artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleBuildResult {
    pub id: String,
    pub version: String,
    pub artifact: PathBuf,
    pub digest: String,
}

/// A bundle currently recorded in a project's Lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledBundle {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub members: Vec<String>,
}

/// The result of adding or updating a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleApplyResult {
    pub id: String,
    pub version: String,
    pub changed_members: Vec<String>,
    pub unchanged: bool,
}

/// The public lifecycle seam for bundle build, installation, update, removal,
/// inspection, and manifest restoration.
#[derive(Debug, Clone)]
pub struct BundleService {
    pub(crate) project_root: PathBuf,
    pub(crate) skills_directory: PathBuf,
}

impl BundleService {
    pub fn new(project_root: impl Into<PathBuf>, skills_directory: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            skills_directory: skills_directory.into(),
        }
    }

    /// Build `<bundle-id>-<version>.zip` from the current project's declared
    /// members. Only members named by `[bundle.members]` are packaged.
    pub fn build(&self, output_directory: &Path) -> Result<BundleBuildResult, ServiceError> {
        let manifest_path = self.project_root.join("skill-project.toml");
        let manifest_bytes = fs::read(&manifest_path).map_err(ServiceError::Io)?;
        let (descriptor, dependencies) = parse_bundle_project(&manifest_bytes)?;
        let members = prepare_members(&self.skills_directory, &descriptor, &dependencies)?;
        fs::create_dir_all(output_directory).map_err(ServiceError::Io)?;

        let artifact =
            output_directory.join(format!("{}-{}.zip", descriptor.id, descriptor.version));
        let archive_lock = BundleArchiveLock::from_members(&descriptor, &members);
        write_bundle_archive(&artifact, &manifest_bytes, &archive_lock, &members)?;

        Ok(BundleBuildResult {
            id: descriptor.id,
            version: descriptor.version,
            artifact,
            digest: archive_lock.release_digest().to_string(),
        })
    }

    /// Identify a bundle archive before the single-skill ZIP path handles it.
    /// A file that declares a `[bundle]` table but fails bundle validation is an
    /// error, never a fallback to a single-skill archive.
    pub fn is_bundle_artifact(artifact: &Path) -> Result<bool, ServiceError> {
        if artifact
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("zip")
        {
            return Ok(false);
        }
        let temporary = TempDir::new().map_err(ServiceError::Io)?;
        ZipHandler::new()?.extract_to_dir(artifact, temporary.path())?;
        let manifest_path = temporary.path().join("skill-project.toml");
        if !manifest_path.exists() {
            return Ok(false);
        }
        let manifest = fs::read(&manifest_path).map_err(ServiceError::Io)?;
        let value: toml::Value = toml::from_slice(&manifest).map_err(|error| {
            ServiceError::Validation(format!("Invalid archive skill-project.toml: {error}"))
        })?;
        if value.get("bundle").is_none() {
            return Ok(false);
        }
        parse_bundle_descriptor(&manifest)?;
        Ok(true)
    }

    /// Add a local bundle. Adding a different release for an installed identity
    /// is refused; callers must use [`Self::update`] to make replacement explicit.
    pub fn install(&self, artifact: &Path) -> Result<BundleApplyResult, ServiceError> {
        self.apply(artifact, ApplyMode::Add)
    }

    /// Preview a selected replacement artifact without changing project state.
    pub fn preview_update(&self, id: &str, artifact: &Path) -> Result<Vec<String>, ServiceError> {
        let prepared = PreparedBundle::load(artifact)?;
        if prepared.descriptor.id != id {
            return Err(ServiceError::InvalidOperation(format!(
                "Bundle artifact identifies '{}', not requested bundle '{}'",
                prepared.descriptor.id, id
            )));
        }
        let (_, lock) = self.load_project_state()?;
        let current = lock
            .bundles
            .iter()
            .find(|bundle| bundle.id == id)
            .ok_or_else(|| {
                ServiceError::SkillNotFound(format!("Bundle '{id}' is not installed"))
            })?;
        let current_ids: BTreeSet<_> = current
            .members
            .iter()
            .map(|member| member.id.as_str())
            .collect();
        let proposed_ids: BTreeSet<_> = prepared.members.keys().map(String::as_str).collect();

        let mut changes = Vec::new();
        for member in proposed_ids.difference(&current_ids) {
            changes.push(format!("add {member}"));
        }
        for member in current_ids.difference(&proposed_ids) {
            changes.push(format!("remove {member}"));
        }
        for member in current_ids.intersection(&proposed_ids) {
            let current_digest = current
                .members
                .iter()
                .find(|candidate| candidate.id == *member)
                .map(|candidate| candidate.digest.as_str());
            let proposed_digest = prepared
                .members
                .get(*member)
                .map(|candidate| candidate.digest.as_str());
            if current_digest != proposed_digest {
                changes.push(format!("replace {member}"));
            }
        }
        changes.sort();
        Ok(changes)
    }

    /// Update one installed bundle from an explicitly selected artifact.
    pub fn update(&self, id: &str, artifact: &Path) -> Result<BundleApplyResult, ServiceError> {
        self.apply(artifact, ApplyMode::Update { id })
    }

    pub fn override_member(&self, id: &str, source: &Path) -> Result<(), ServiceError> {
        apply_personal_override(self, id, source)
    }

    /// Remove a bundle's ownership. Members remain when another bundle or a
    /// direct project dependency still owns them.
    pub fn remove(&self, id: &str) -> Result<(), ServiceError> {
        let (mut manifest, mut lock) = self.load_project_state()?;
        let current = lock
            .bundles
            .iter()
            .find(|bundle| bundle.id == id)
            .cloned()
            .ok_or_else(|| {
                ServiceError::SkillNotFound(format!("Bundle '{id}' is not installed"))
            })?;

        let mut deletes = Vec::new();
        for member in &current.members {
            if self.has_other_owner(&lock, id, &member.id)
                || self.has_direct_dependency(&manifest.project, &member.id)
                || self.has_personal_override(&lock, &member.id)
            {
                continue;
            }
            self.ensure_unmodified(&member.id, &member.digest)?;
            deletes.push(member.id.clone());
        }

        lock.bundles.retain(|bundle| bundle.id != id);
        manifest.bundles.remove(id);

        self.persist_transaction(&deletes, &[], &manifest, &lock, None, None)
    }

    pub fn list(&self) -> Result<Vec<InstalledBundle>, ServiceError> {
        let (_, lock) = self.load_project_state()?;
        let mut bundles: Vec<_> = lock
            .bundles
            .into_iter()
            .map(|bundle| InstalledBundle {
                id: bundle.id,
                version: bundle.version,
                digest: bundle.digest,
                members: bundle.members.into_iter().map(|member| member.id).collect(),
            })
            .collect();
        bundles.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(bundles)
    }

    /// Restore all declared bundle artifacts. The declared artifact is always
    /// relative to the project root, making an installed local release portable
    /// with the project when its artifact directory is retained.
    pub fn install_declared(&self) -> Result<Vec<BundleApplyResult>, ServiceError> {
        let (manifest, _) = self.load_project_state()?;
        let mut results = Vec::new();
        for (id, dependency) in manifest.bundles {
            let artifact = self.project_root.join(&dependency.artifact);
            results.push(self.apply(&artifact, ApplyMode::Restore { id: &id })?);
        }
        restore_personal_overrides(self)?;
        Ok(results)
    }

    /// Restore the exact bundle releases recorded in `skills.lock`.
    pub fn install_declared_locked(&self) -> Result<Vec<BundleApplyResult>, ServiceError> {
        let (_, lock) = self.load_project_state()?;
        let mut results = Vec::new();
        for expected in lock.bundles {
            let artifact = self.project_root.join(&expected.artifact);
            results.push(self.apply(
                &artifact,
                ApplyMode::RestoreLocked {
                    expected: &expected,
                },
            )?);
        }
        restore_personal_overrides(self)?;
        Ok(results)
    }

    /// Reject ordinary skill removal while an installed bundle owns the skill.
    pub fn ensure_individual_removal_allowed(&self, id: &str) -> Result<(), ServiceError> {
        let (_, lock) = self.load_project_state()?;
        let mut owners: Vec<_> = lock
            .bundles
            .iter()
            .filter(|bundle| bundle.members.iter().any(|member| member.id == id))
            .map(|bundle| bundle.id.as_str())
            .collect();
        owners.sort_unstable();
        if owners.is_empty() {
            return Ok(());
        }
        Err(ServiceError::InvalidOperation(format!(
            "Skill '{id}' is managed by installed bundle(s): {}. Remove the owning bundle with 'fastskill remove --bundle <bundle-id>'",
            owners.join(", ")
        )))
    }

    fn apply(
        &self,
        artifact: &Path,
        mode: ApplyMode<'_>,
    ) -> Result<BundleApplyResult, ServiceError> {
        let prepared = PreparedBundle::load(artifact)?;
        let (mut manifest, mut lock) = self.load_project_state()?;
        let mut history = self.load_history()?;
        let existing = lock
            .bundles
            .iter()
            .find(|bundle| bundle.id == prepared.descriptor.id)
            .cloned();

        if let ApplyMode::RestoreLocked { expected } = mode {
            prepared.verify_locked_release(expected)?;
        }

        if let Some(installed) = &existing {
            if installed.version == prepared.descriptor.version
                && installed.digest != prepared.release_digest
            {
                return Err(ServiceError::Validation(format!(
                    "Bundle release '{}@{}' is immutable: its contents differ from the installed digest",
                    installed.id, installed.version
                )));
            }
        }

        match mode {
            ApplyMode::Add => {
                if let Some(installed) = &existing {
                    if installed.version != prepared.descriptor.version {
                        return Err(ServiceError::InvalidOperation(format!(
                            "Bundle '{}' is already installed at {}; use 'fastskill update --bundle {} --from <artifact>'",
                            installed.id, installed.version, installed.id
                        )));
                    }
                    if installed.digest == prepared.release_digest {
                        return Ok(BundleApplyResult {
                            id: installed.id.clone(),
                            version: installed.version.clone(),
                            changed_members: Vec::new(),
                            unchanged: true,
                        });
                    }
                }
            }
            ApplyMode::Update { id } => {
                if prepared.descriptor.id != id {
                    return Err(ServiceError::InvalidOperation(format!(
                        "Bundle artifact identifies '{}', not requested bundle '{id}'",
                        prepared.descriptor.id
                    )));
                }
                if existing.is_none() {
                    return Err(ServiceError::SkillNotFound(format!(
                        "Bundle '{id}' is not installed"
                    )));
                }
            }
            ApplyMode::Restore { id } => {
                if prepared.descriptor.id != id {
                    return Err(ServiceError::InvalidOperation(format!(
                        "Declared bundle '{id}' does not match artifact identity '{}'",
                        prepared.descriptor.id
                    )));
                }
                if let Some(installed) = &existing {
                    if installed.version == prepared.descriptor.version
                        && installed.digest == prepared.release_digest
                        && self.bundle_members_match(installed)
                    {
                        return Ok(BundleApplyResult {
                            id: installed.id.clone(),
                            version: installed.version.clone(),
                            changed_members: Vec::new(),
                            unchanged: true,
                        });
                    }
                }
            }
            ApplyMode::RestoreLocked { expected } => {
                let declaration_matches =
                    manifest
                        .bundles
                        .get(&expected.id)
                        .is_some_and(|dependency| {
                            dependency.version == expected.version
                                && dependency.artifact == expected.artifact
                        });
                if let Some(installed) = &existing {
                    if installed == expected
                        && declaration_matches
                        && self.bundle_members_match(installed)
                    {
                        return Ok(BundleApplyResult {
                            id: installed.id.clone(),
                            version: installed.version.clone(),
                            changed_members: Vec::new(),
                            unchanged: true,
                        });
                    }
                }
            }
        }

        let release_key = format!("{}@{}", prepared.descriptor.id, prepared.descriptor.version);
        if let Some(known_digest) = history.releases.get(&release_key) {
            if known_digest != &prepared.release_digest {
                return Err(ServiceError::Validation(format!(
                    "Bundle release '{}' is immutable: its contents differ from the previously known digest",
                    release_key
                )));
            }
        }

        self.preflight_apply(&prepared, existing.as_ref(), &manifest, &lock)?;
        let changes = self.plan_changes(&prepared, existing.as_ref(), &manifest, &lock)?;
        let artifact_relative = self.artifact_relative(&prepared.descriptor)?;
        let next = ProjectLockedBundleEntry {
            id: prepared.descriptor.id.clone(),
            version: prepared.descriptor.version.clone(),
            artifact: artifact_relative.clone(),
            digest: prepared.release_digest.clone(),
            members: prepared
                .members
                .values()
                .map(|member| ProjectLockedBundleMember {
                    id: member.id.clone(),
                    digest: member.digest.clone(),
                    overridable: member.overridable,
                })
                .collect(),
        };
        lock.bundles.retain(|bundle| bundle.id != next.id);
        lock.bundles.push(next);
        manifest.bundles.insert(
            prepared.descriptor.id.clone(),
            BundleDependency {
                version: prepared.descriptor.version.clone(),
                artifact: artifact_relative,
            },
        );
        history
            .releases
            .insert(release_key, prepared.release_digest.clone());

        let changed_members: Vec<_> = changes
            .replacements
            .iter()
            .map(|change| change.id.clone())
            .chain(changes.deletions.iter().cloned())
            .collect();
        self.persist_transaction(
            &changes.deletions,
            &changes.replacements,
            &manifest,
            &lock,
            Some((
                &history,
                artifact,
                &self.project_root.join(BUNDLE_STATE_DIRECTORY).join(format!(
                    "{}-{}.zip",
                    prepared.descriptor.id, prepared.descriptor.version
                )),
            )),
            Some(&prepared.descriptor),
        )?;

        Ok(BundleApplyResult {
            id: prepared.descriptor.id,
            version: prepared.descriptor.version,
            changed_members,
            unchanged: false,
        })
    }

    fn load_project_state(
        &self,
    ) -> Result<(ProjectBundleManifest, ProjectSkillsLock), ServiceError> {
        let manifest_path = self.project_root.join("skill-project.toml");
        let project = SkillProjectToml::load_from_file(&manifest_path).map_err(|error| {
            ServiceError::Config(format!("Failed to load skill-project.toml: {error}"))
        })?;
        let raw_manifest = fs::read_to_string(&manifest_path).map_err(ServiceError::Io)?;
        let declarations: BundleManifestTables =
            toml::from_str(&raw_manifest).map_err(|error| {
                ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
            })?;
        let lock_path = self.project_root.join("skills.lock");
        let lock = if lock_path.exists() {
            ProjectSkillsLock::load_from_file(&lock_path).map_err(|error| {
                ServiceError::Config(format!("Failed to load skills.lock: {error}"))
            })?
        } else {
            ProjectSkillsLock::new_empty()
        };
        Ok((
            ProjectBundleManifest {
                project,
                bundles: declarations.bundles,
                overrides: declarations.overrides,
            },
            lock,
        ))
    }

    fn load_history(&self) -> Result<BundleHistory, ServiceError> {
        let path = self.project_root.join(BUNDLE_HISTORY_FILE);
        if !path.exists() {
            return Ok(BundleHistory::default());
        }
        let content = fs::read_to_string(path).map_err(ServiceError::Io)?;
        toml::from_str(&content).map_err(|error| {
            ServiceError::Config(format!("Failed to load bundle history: {error}"))
        })
    }

    fn preflight_apply(
        &self,
        prepared: &PreparedBundle,
        existing: Option<&ProjectLockedBundleEntry>,
        manifest: &ProjectBundleManifest,
        lock: &ProjectSkillsLock,
    ) -> Result<(), ServiceError> {
        if let Some(current) = existing {
            for member in &current.members {
                if self.has_personal_override(lock, &member.id) {
                    continue;
                }
                let still_present = prepared.members.contains_key(&member.id);
                let needs_mutation = still_present
                    .then(|| prepared.members.get(&member.id))
                    .flatten()
                    .is_none_or(|replacement| replacement.digest != member.digest)
                    || !still_present;
                if needs_mutation
                    && !self.has_other_owner(lock, &current.id, &member.id)
                    && !self.has_direct_dependency(&manifest.project, &member.id)
                {
                    self.ensure_unmodified(&member.id, &member.digest)?;
                }
            }
        }

        for member in prepared.members.values() {
            let owners =
                self.owners_for(lock, &member.id, existing.map(|bundle| bundle.id.as_str()));
            if self.has_personal_override(lock, &member.id) {
                if !member.overridable || owners.iter().any(|owner| !owner.overridable) {
                    return Err(ServiceError::InvalidOperation(format!(
                        "Bundle '{}' does not permit the personal override for '{}'",
                        prepared.descriptor.id, member.id
                    )));
                }
                continue;
            }
            if owners.iter().any(|owner| owner.digest != member.digest) {
                return Err(ServiceError::InvalidOperation(format!(
                    "Skill '{}' has conflicting contents required by another installed bundle",
                    member.id
                )));
            }
            let destination = self.skills_directory.join(&member.id);
            if destination.exists() {
                let actual = digest_directory(&destination)?;
                let expected_existing =
                    owners
                        .first()
                        .map(|owner| owner.digest.as_str())
                        .or_else(|| {
                            existing.and_then(|bundle| {
                                bundle
                                    .members
                                    .iter()
                                    .find(|current| current.id == member.id)
                                    .map(|current| current.digest.as_str())
                            })
                        });
                if let Some(expected) = expected_existing {
                    if actual != expected {
                        return Err(ServiceError::InvalidOperation(format!(
                            "Skill '{}' is locally modified; declare a permitted personal override or discard the edit before changing bundles",
                            member.id
                        )));
                    }
                } else if actual != member.digest {
                    return Err(ServiceError::InvalidOperation(format!(
                        "Skill '{}' already exists with different contents",
                        member.id
                    )));
                }
            }
        }
        Ok(())
    }

    fn plan_changes(
        &self,
        prepared: &PreparedBundle,
        existing: Option<&ProjectLockedBundleEntry>,
        manifest: &ProjectBundleManifest,
        lock: &ProjectSkillsLock,
    ) -> Result<BundleChanges, ServiceError> {
        let mut changes = BundleChanges::default();
        for member in prepared.members.values() {
            if self.has_personal_override(lock, &member.id) {
                continue;
            }
            let destination = self.skills_directory.join(&member.id);
            let identical = destination
                .exists()
                .then(|| digest_directory(&destination))
                .transpose()?
                .is_some_and(|digest| digest == member.digest);
            if !identical {
                changes.replacements.push(BundleReplacement {
                    id: member.id.clone(),
                    source: member.source.clone(),
                });
            }
        }

        if let Some(current) = existing {
            for member in &current.members {
                if prepared.members.contains_key(&member.id)
                    || self.has_personal_override(lock, &member.id)
                    || self.has_other_owner(lock, &current.id, &member.id)
                    || self.has_direct_dependency(&manifest.project, &member.id)
                {
                    continue;
                }
                changes.deletions.push(member.id.clone());
            }
        }
        changes
            .replacements
            .sort_by(|left, right| left.id.cmp(&right.id));
        changes.deletions.sort();
        Ok(changes)
    }

    fn artifact_relative(&self, descriptor: &BundleDescriptor) -> Result<String, ServiceError> {
        semver::Version::parse(&descriptor.version).map_err(|error| {
            ServiceError::Validation(format!(
                "Bundle version '{}' is not valid SemVer: {error}",
                descriptor.version
            ))
        })?;
        Ok(format!(
            "{BUNDLE_STATE_DIRECTORY}/{}-{}.zip",
            descriptor.id, descriptor.version
        ))
    }

    fn ensure_unmodified(&self, id: &str, expected: &str) -> Result<(), ServiceError> {
        let destination = self.skills_directory.join(id);
        if !destination.exists() {
            return Ok(());
        }
        let actual = digest_directory(&destination)?;
        if actual == expected {
            Ok(())
        } else {
            Err(ServiceError::InvalidOperation(format!(
                "Skill '{id}' is locally modified; FastSkill will not replace or delete it"
            )))
        }
    }

    fn owners_for<'a>(
        &self,
        lock: &'a ProjectSkillsLock,
        skill_id: &str,
        excluding_bundle: Option<&str>,
    ) -> Vec<&'a ProjectLockedBundleMember> {
        lock.bundles
            .iter()
            .filter(|bundle| Some(bundle.id.as_str()) != excluding_bundle)
            .flat_map(|bundle| bundle.members.iter())
            .filter(|member| member.id == skill_id)
            .collect()
    }

    fn has_other_owner(&self, lock: &ProjectSkillsLock, bundle_id: &str, skill_id: &str) -> bool {
        !self.owners_for(lock, skill_id, Some(bundle_id)).is_empty()
    }

    fn has_direct_dependency(&self, manifest: &SkillProjectToml, id: &str) -> bool {
        manifest
            .dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.dependencies.contains_key(id))
    }

    fn has_personal_override(&self, lock: &ProjectSkillsLock, id: &str) -> bool {
        lock.overrides
            .iter()
            .any(|override_entry| override_entry.id == id)
    }

    fn bundle_members_match(&self, bundle: &ProjectLockedBundleEntry) -> bool {
        bundle.members.iter().all(|member| {
            digest_directory(&self.skills_directory.join(&member.id))
                .map(|digest| digest == member.digest)
                .unwrap_or(false)
        })
    }

    fn persist_transaction(
        &self,
        deletions: &[String],
        replacements: &[BundleReplacement],
        manifest: &ProjectBundleManifest,
        lock: &ProjectSkillsLock,
        history_and_artifact: Option<(&BundleHistory, &Path, &Path)>,
        _descriptor: Option<&BundleDescriptor>,
    ) -> Result<(), ServiceError> {
        let affected: Vec<_> = deletions
            .iter()
            .cloned()
            .chain(
                replacements
                    .iter()
                    .map(|replacement| replacement.id.clone()),
            )
            .collect();
        let mut transaction_files = vec![
            self.project_root.join("skill-project.toml"),
            self.project_root.join("skills.lock"),
            self.project_root.join(BUNDLE_HISTORY_FILE),
        ];
        if let Some((_, _, destination)) = history_and_artifact {
            transaction_files.push(destination.to_path_buf());
        }
        let mut transaction =
            BundleTransaction::capture(&self.skills_directory, &affected, &transaction_files)?;
        let result = (|| {
            for id in deletions {
                remove_skill_directory(&self.skills_directory.join(id))?;
            }
            for replacement in replacements {
                replace_skill_directory(
                    &self.skills_directory.join(&replacement.id),
                    &replacement.source,
                )?;
            }
            manifest
                .project
                .save_to_file(&self.project_root.join("skill-project.toml"))
                .map_err(|error| {
                    ServiceError::Config(format!("Failed to save skill-project.toml: {error}"))
                })?;
            save_bundle_declarations(
                &self.project_root.join("skill-project.toml"),
                &manifest.bundles,
                &manifest.overrides,
            )?;
            lock.save_to_file(&self.project_root.join("skills.lock"))
                .map_err(|error| {
                    ServiceError::Config(format!("Failed to save skills.lock: {error}"))
                })?;
            if let Some((history, source, destination)) = history_and_artifact {
                let source_bytes = fs::read(source).map_err(ServiceError::Io)?;
                atomic_write(destination, &source_bytes).map_err(ServiceError::Io)?;
                let history_bytes = toml::to_string_pretty(history).map_err(|error| {
                    ServiceError::Config(format!("Failed to serialize bundle history: {error}"))
                })?;
                atomic_write(
                    &self.project_root.join(BUNDLE_HISTORY_FILE),
                    history_bytes.as_bytes(),
                )
                .map_err(ServiceError::Io)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            transaction.rollback()?;
            return Err(error);
        }
        transaction.commit();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum ApplyMode<'a> {
    Add,
    Update {
        id: &'a str,
    },
    Restore {
        id: &'a str,
    },
    RestoreLocked {
        expected: &'a ProjectLockedBundleEntry,
    },
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct BundleManifestTables {
    #[serde(default)]
    pub(crate) bundles: BTreeMap<String, BundleDependency>,
    #[serde(default)]
    pub(crate) overrides: BTreeMap<String, BundleOverrideDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BundleDependency {
    pub(crate) version: String,
    pub(crate) artifact: String,
}

#[derive(Debug, Clone)]
struct ProjectBundleManifest {
    project: SkillProjectToml,
    bundles: BTreeMap<String, BundleDependency>,
    overrides: BTreeMap<String, BundleOverrideDeclaration>,
}

#[derive(Debug, Default)]
struct BundleChanges {
    replacements: Vec<BundleReplacement>,
    deletions: Vec<String>,
}

#[derive(Debug, Clone)]
struct BundleReplacement {
    id: String,
    source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BundleDescriptor {
    #[serde(rename = "format")]
    pub(crate) format_marker: String,
    pub(crate) id: String,
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) members: BTreeMap<String, BundleMemberPolicy>,
}

impl BundleDescriptor {
    pub(super) fn validate(
        &self,
        dependencies: &BTreeMap<String, crate::core::manifest::DependencySpec>,
    ) -> Result<(), ServiceError> {
        if self.format_marker != BUNDLE_FORMAT {
            return Err(ServiceError::Validation(format!(
                "Archive is not a supported FastSkill bundle (expected format '{BUNDLE_FORMAT}')"
            )));
        }
        SkillId::new(self.id.clone())?;
        semver::Version::parse(&self.version).map_err(|error| {
            ServiceError::Validation(format!(
                "Bundle version '{}' is not valid SemVer: {error}",
                self.version
            ))
        })?;
        if self.members.is_empty() {
            return Err(ServiceError::Validation(
                "Bundle must declare at least one member".to_string(),
            ));
        }
        for member in self.members.keys() {
            SkillId::new(member.clone())?;
            if !dependencies.contains_key(member) {
                return Err(ServiceError::Validation(format!(
                    "Bundle member '{member}' is not declared in [dependencies]"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct BundleMemberPolicy {
    #[serde(default)]
    pub(crate) overridable: bool,
}

#[derive(Debug)]
struct PreparedBundle {
    _temporary: TempDir,
    descriptor: BundleDescriptor,
    members: BTreeMap<String, PreparedMember>,
    release_digest: String,
}

impl PreparedBundle {
    fn load(artifact: &Path) -> Result<Self, ServiceError> {
        let temporary = TempDir::new().map_err(ServiceError::Io)?;
        ZipHandler::new()?.extract_to_dir(artifact, temporary.path())?;
        let manifest_bytes =
            fs::read(temporary.path().join("skill-project.toml")).map_err(|_| {
                ServiceError::Validation(
                    "Bundle archive is missing root skill-project.toml".to_string(),
                )
            })?;
        let (descriptor, dependencies) = parse_bundle_project(&manifest_bytes)?;
        let members =
            prepare_members(&temporary.path().join("skills"), &descriptor, &dependencies)?;
        let lock_content =
            fs::read_to_string(temporary.path().join("skills.lock")).map_err(|_| {
                ServiceError::Validation("Bundle archive is missing root skills.lock".to_string())
            })?;
        let archive_lock: BundleArchiveLock = toml::from_str(&lock_content).map_err(|error| {
            ServiceError::Validation(format!("Invalid bundle skills.lock: {error}"))
        })?;
        archive_lock.verify(&descriptor, &members)?;
        Ok(Self {
            _temporary: temporary,
            descriptor,
            members,
            release_digest: archive_lock.release_digest().to_string(),
        })
    }

    fn verify_locked_release(
        &self,
        expected: &ProjectLockedBundleEntry,
    ) -> Result<(), ServiceError> {
        let members_match = expected.members.len() == self.members.len()
            && expected.members.iter().all(|locked| {
                self.members.get(&locked.id).is_some_and(|member| {
                    member.digest == locked.digest && member.overridable == locked.overridable
                })
            });
        if self.descriptor.id != expected.id
            || self.descriptor.version != expected.version
            || self.release_digest != expected.digest
            || !members_match
        {
            return Err(ServiceError::Validation(format!(
                "Locked bundle '{}@{}' does not match its cached artifact",
                expected.id, expected.version
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedMember {
    pub(crate) id: String,
    pub(crate) source: PathBuf,
    pub(crate) digest: String,
    pub(crate) overridable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BundleArchiveLockMember {
    pub(crate) digest: String,
    pub(crate) overridable: bool,
}

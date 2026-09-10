//! Self-contained, versioned bundle lifecycle.
//!
//! A bundle is deliberately handled as one core operation. The command layer
//! supplies paths and reports the outcome; this module validates the complete
//! archive, plans all member changes, and persists the Manifest and Lock with
//! the installed membership.

use crate::core::bundle_archive::{write_bundle_archive, BundleArchiveLock};
use crate::core::bundle_persistence::{
    apply_personal_override, digest_directory, parse_bundle_descriptor, parse_bundle_project,
    prepare_members, preview_personal_override, remove_skill_directory, replace_skill_directory,
    save_bundle_declarations, BundleHistory, BundleTransaction,
};
use crate::core::lock::{ProjectLockedBundleEntry, ProjectLockedBundleMember, ProjectSkillsLock};
use crate::core::manifest::SkillProjectToml;
use crate::core::ownership::ProjectOwnership;
use crate::core::service::ServiceError;
use crate::core::state_guard::{StateMutationGuard, StateMutationLease};
use crate::storage::zip::ZipHandler;
use crate::utils::atomic_write;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub(crate) use crate::core::bundle_types::{
    BundleArchiveLockMember, BundleDependency, BundleDescriptor, BundleManifestTables,
    BundleMemberPolicy, PreparedBundle, PreparedMember, ProjectBundleManifest,
};
pub use crate::core::bundle_types::{
    BundleInstallPreview, BundleOverridePreview, BundleRemovalPreview, BundleUpdatePreview,
};

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

/// A member selected by a fully validated declared-bundle plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredBundleMember {
    pub bundle_id: String,
    pub id: String,
    pub digest: String,
    pub overridable: bool,
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
        self.apply(artifact, ApplyMode::Add, None)
    }

    /// Validate an add artifact and report its complete effects without mutation.
    pub fn plan_install(&self, artifact: &Path) -> Result<BundleInstallPreview, ServiceError> {
        crate::core::bundle_plan::preview_install(self, artifact)
    }

    /// Preview a selected replacement artifact without changing project state.
    pub fn preview_update(&self, id: &str, artifact: &Path) -> Result<Vec<String>, ServiceError> {
        self.plan_update(id, artifact)
            .map(|preview| preview.changes)
    }

    /// Validate a selected replacement and report revisions and ownership effects.
    pub fn plan_update(
        &self,
        id: &str,
        artifact: &Path,
    ) -> Result<BundleUpdatePreview, ServiceError> {
        crate::core::bundle_plan::preview_update(self, id, artifact)
    }

    /// Update one installed bundle from an explicitly selected artifact.
    pub fn update(&self, id: &str, artifact: &Path) -> Result<BundleApplyResult, ServiceError> {
        self.apply(artifact, ApplyMode::Update { id }, None)
    }

    pub fn override_member(&self, id: &str, source: &Path) -> Result<(), ServiceError> {
        apply_personal_override(self, id, source)
    }

    /// Validate a personal override without modifying files or project state.
    pub fn preview_override(
        &self,
        id: &str,
        source: &Path,
    ) -> Result<BundleOverridePreview, ServiceError> {
        preview_personal_override(self, id, source)
    }

    /// Restore a personally overridden member to the packaged contents agreed
    /// by all retained bundle owners. Returns `false` when no override exists.
    pub fn reset_override(&self, id: &str) -> Result<bool, ServiceError> {
        crate::core::bundle_override::reset_personal_override(self, id)
    }

    /// Validate an override reset without modifying files or project state.
    pub fn preview_reset_override(&self, id: &str) -> Result<BundleOverridePreview, ServiceError> {
        crate::core::bundle_override::preview_reset_personal_override(self, id)
    }

    /// Remove a bundle's ownership. Members remain when another bundle or a
    /// direct project dependency still owns them.
    pub fn remove(&self, id: &str) -> Result<(), ServiceError> {
        let prepared = self.prepare_remove(id)?;
        self.persist_transaction(
            &prepared.preview.delete_members,
            &[],
            &prepared.manifest,
            &prepared.lock,
            &prepared.baseline_manifest,
            &prepared.baseline_lock,
            None,
            None,
            None,
        )
    }

    /// Validate bundle removal and report ownership effects without mutation.
    pub fn preview_remove(&self, id: &str) -> Result<BundleRemovalPreview, ServiceError> {
        self.prepare_remove(id).map(|prepared| prepared.preview)
    }

    fn prepare_remove(&self, id: &str) -> Result<PreparedBundleRemoval, ServiceError> {
        crate::core::service::SkillId::new(id.to_string())?;
        let (mut manifest, mut lock) = self.load_project_state()?;
        let baseline_manifest = manifest.clone();
        let baseline_lock = lock.clone();
        let current = lock
            .bundles
            .iter()
            .find(|bundle| bundle.id == id)
            .cloned()
            .ok_or_else(|| {
                ServiceError::SkillNotFound(format!("Bundle '{id}' is not installed"))
            })?;

        let override_ids: BTreeSet<_> = lock
            .overrides
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
        crate::core::bundle_override::retain_final_personal_overrides(
            &self.project_root,
            id,
            &mut manifest.project,
            &mut manifest.overrides,
            &mut lock,
        )?;

        let mut deletes = Vec::new();
        let individual_ownership = ProjectOwnership::new(&manifest.project, &lock);
        for member in &current.members {
            if self.has_other_owner(&lock, id, &member.id)
                || self.has_direct_dependency(&manifest.project, &member.id)
                || !individual_ownership
                    .roots_requiring_skill(&member.id)
                    .is_empty()
                || self.has_personal_override(&lock, &member.id)
            {
                continue;
            }
            self.ensure_unmodified(&member.id, &member.digest)?;
            deletes.push(member.id.clone());
        }

        lock.bundles.retain(|bundle| bundle.id != id);
        manifest.bundles.remove(id);
        let retained_members = current
            .members
            .iter()
            .map(|member| member.id.clone())
            .filter(|member| !deletes.contains(member))
            .collect();
        let promoted_overrides = override_ids
            .into_iter()
            .filter(|override_id| !lock.overrides.iter().any(|entry| entry.id == *override_id))
            .collect();
        Ok(PreparedBundleRemoval {
            manifest,
            lock,
            baseline_manifest,
            baseline_lock,
            preview: BundleRemovalPreview {
                id: id.to_string(),
                current_revision: current.version,
                delete_members: deletes,
                retained_members,
                promoted_overrides,
            },
        })
    }

    pub fn list(&self) -> Result<Vec<InstalledBundle>, ServiceError> {
        crate::core::bundle_declared::list(self)
    }

    /// Restore all declared bundle artifacts. The declared artifact is always
    /// relative to the project root, making an installed local release portable
    /// with the project when its artifact directory is retained.
    pub fn install_declared(&self) -> Result<Vec<BundleApplyResult>, ServiceError> {
        crate::core::bundle_declared::install_declared(self, false, None)
    }

    /// Restore declared bundles while a caller retains the combined writer lease.
    pub fn install_declared_with_guard(
        &self,
        guard: &StateMutationGuard,
    ) -> Result<Vec<BundleApplyResult>, ServiceError> {
        crate::core::bundle_declared::install_declared(self, false, Some(guard))
    }

    /// Restore the exact bundle releases recorded in `skills.lock`.
    pub fn install_declared_locked(&self) -> Result<Vec<BundleApplyResult>, ServiceError> {
        crate::core::bundle_declared::install_declared(self, true, None)
    }

    /// Restore locked bundles while a caller retains the combined writer lease.
    pub fn install_declared_locked_with_guard(
        &self,
        guard: &StateMutationGuard,
    ) -> Result<Vec<BundleApplyResult>, ServiceError> {
        crate::core::bundle_declared::install_declared(self, true, Some(guard))
    }

    /// Validate every declared bundle and their combined ownership without
    /// mutating files or project state.
    pub fn validate_declared(&self, locked: bool) -> Result<(), ServiceError> {
        self.validated_declared_members(locked).map(|_| ())
    }

    /// Return the complete member selections after running the same validation
    /// used by declared-bundle installation.
    pub fn validated_declared_members(
        &self,
        locked: bool,
    ) -> Result<Vec<DeclaredBundleMember>, ServiceError> {
        crate::core::bundle_plan::validate_declared(self, locked)
    }

    /// Reject ordinary skill removal while an installed bundle owns the skill.
    pub fn ensure_individual_removal_allowed(&self, id: &str) -> Result<(), ServiceError> {
        crate::core::bundle_declared::ensure_individual_removal_allowed(self, id)
    }

    pub(crate) fn apply(
        &self,
        artifact: &Path,
        mode: ApplyMode<'_>,
        guard: Option<&StateMutationGuard>,
    ) -> Result<BundleApplyResult, ServiceError> {
        let prepared = PreparedBundle::load(artifact)?;
        let (mut manifest, mut lock) = self.load_project_state()?;
        let baseline_manifest = manifest.clone();
        let baseline_lock = lock.clone();
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
                            "Bundle '{}' is already installed at {}; use 'fastskill bundle update {} --from <artifact>'",
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
                if let Some(installed) = &existing {
                    if installed.digest == prepared.release_digest
                        && self.bundle_members_match(installed, &lock)
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
                        && self.bundle_members_match(installed, &lock)
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
                        && self.bundle_members_match(installed, &lock)
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
            &baseline_manifest,
            &baseline_lock,
            Some((
                &history,
                artifact,
                &self.project_root.join(BUNDLE_STATE_DIRECTORY).join(format!(
                    "{}-{}.zip",
                    prepared.descriptor.id, prepared.descriptor.version
                )),
            )),
            Some(&prepared.descriptor),
            guard,
        )?;

        Ok(BundleApplyResult {
            id: prepared.descriptor.id,
            version: prepared.descriptor.version,
            changed_members,
            unchanged: false,
        })
    }

    pub(crate) fn load_project_state(
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

    pub(crate) fn load_history(&self) -> Result<BundleHistory, ServiceError> {
        let path = self.project_root.join(BUNDLE_HISTORY_FILE);
        if !path.exists() {
            return Ok(BundleHistory::default());
        }
        let content = fs::read_to_string(path).map_err(ServiceError::Io)?;
        toml::from_str(&content).map_err(|error| {
            ServiceError::Config(format!("Failed to load bundle history: {error}"))
        })
    }

    pub(crate) fn preflight_apply(
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
            let individual_roots =
                ProjectOwnership::new(&manifest.project, lock).roots_requiring_skill(&member.id);
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
            if !individual_roots.is_empty() {
                let selected_digest = lock
                    .skills
                    .iter()
                    .find(|entry| entry.id == member.id)
                    .and_then(|entry| entry.resolved.checksum.as_deref());
                if selected_digest.is_some_and(|digest| digest != member.digest) {
                    return Err(ServiceError::InvalidOperation(format!(
                        "Skill '{}' has conflicting contents required by individual root(s): {}",
                        member.id,
                        individual_roots.join(", ")
                    )));
                }
            }
            let destination = self.skills_directory.join(&member.id);
            if destination.exists() {
                let actual = digest_directory(&destination)?;
                if !individual_roots.is_empty() && actual != member.digest {
                    return Err(ServiceError::InvalidOperation(format!(
                        "Skill '{}' has conflicting contents required by individual root(s): {}",
                        member.id,
                        individual_roots.join(", ")
                    )));
                }
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

    pub(crate) fn plan_changes(
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

    fn bundle_members_match(
        &self,
        bundle: &ProjectLockedBundleEntry,
        lock: &ProjectSkillsLock,
    ) -> bool {
        bundle.members.iter().all(|member| {
            let effective_digest = lock
                .overrides
                .iter()
                .find(|candidate| candidate.id == member.id)
                .filter(|_| {
                    lock.bundles
                        .iter()
                        .flat_map(|owner| owner.members.iter())
                        .filter(|owned| owned.id == member.id)
                        .all(|owned| owned.overridable)
                })
                .map_or(member.digest.as_str(), |override_entry| {
                    override_entry.digest.as_str()
                });
            digest_directory(&self.skills_directory.join(&member.id))
                .map(|digest| digest == effective_digest)
                .unwrap_or(false)
        })
    }

    fn persist_transaction(
        &self,
        deletions: &[String],
        replacements: &[BundleReplacement],
        manifest: &ProjectBundleManifest,
        lock: &ProjectSkillsLock,
        baseline_manifest: &ProjectBundleManifest,
        baseline_lock: &ProjectSkillsLock,
        history_and_artifact: Option<(&BundleHistory, &Path, &Path)>,
        _descriptor: Option<&BundleDescriptor>,
        existing_guard: Option<&StateMutationGuard>,
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
        let state_guard = StateMutationLease::acquire_or_borrow(
            &self.project_root,
            Some(&self.skills_directory),
            "bundle lifecycle",
            existing_guard,
        )?;
        let current = match self.load_project_state() {
            Ok(current) => current,
            Err(error) => {
                state_guard.recovered()?;
                return Err(error);
            }
        };
        if !same_project_state(&current.0, &current.1, baseline_manifest, baseline_lock) {
            state_guard.recovered()?;
            return Err(ServiceError::InvalidOperation(
                "Bundle state changed while the operation was prepared; retry".to_string(),
            ));
        }
        let transaction =
            match BundleTransaction::capture(&self.skills_directory, &affected, &transaction_files)
            {
                Ok(transaction) => transaction,
                Err(error) => {
                    state_guard.recovered()?;
                    return Err(error);
                }
            };
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
            crate::core::project_state::save_project_preserving(
                &self.project_root.join("skill-project.toml"),
                &manifest.project,
            )
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
            if let Err(recovery_error) = transaction.rollback() {
                return Err(ServiceError::Custom(format!(
                    "Bundle operation failed: {error}; recovery also failed: {recovery_error}"
                )));
            }
            state_guard.recovered()?;
            return Err(error);
        }
        transaction.commit();
        state_guard.commit()?;
        Ok(())
    }
}

fn same_project_state(
    left_manifest: &ProjectBundleManifest,
    left_lock: &ProjectSkillsLock,
    right_manifest: &ProjectBundleManifest,
    right_lock: &ProjectSkillsLock,
) -> bool {
    toml::to_string(&left_manifest.project).ok() == toml::to_string(&right_manifest.project).ok()
        && left_manifest.bundles == right_manifest.bundles
        && left_manifest.overrides == right_manifest.overrides
        && left_lock.metadata == right_lock.metadata
        && left_lock.skills == right_lock.skills
        && left_lock.bundles == right_lock.bundles
        && left_lock.overrides == right_lock.overrides
        && left_lock.covered_roots == right_lock.covered_roots
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ApplyMode<'a> {
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

#[derive(Debug, Default)]
pub(crate) struct BundleChanges {
    pub(crate) replacements: Vec<BundleReplacement>,
    pub(crate) deletions: Vec<String>,
}

struct PreparedBundleRemoval {
    manifest: ProjectBundleManifest,
    lock: ProjectSkillsLock,
    baseline_manifest: ProjectBundleManifest,
    baseline_lock: ProjectSkillsLock,
    preview: BundleRemovalPreview,
}

#[derive(Debug, Clone)]
pub(crate) struct BundleReplacement {
    pub(crate) id: String,
    source: PathBuf,
}

#[cfg(test)]
#[path = "bundle_tests.rs"]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;

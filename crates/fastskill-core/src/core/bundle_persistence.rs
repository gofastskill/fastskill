use crate::core::bundle::{
    BundleArchiveLockMember, BundleDependency, BundleDescriptor, BundleManifestTables,
    BundleMemberPolicy, BundleOverridePreview, BundleService, PreparedMember, BUNDLE_FORMAT,
};
use crate::core::lock::{ProjectLockedPersonalOverride, ProjectSkillsLock};
use crate::core::manifest::DependencySpec;
use crate::core::origin::Origin;
use crate::core::service::{ServiceError, SkillId};
use crate::core::state_guard::{StateMutationGuard, StateMutationLease};
use crate::core::version::VersionConstraint;
use crate::utils::atomic_write;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;

#[cfg(test)]
thread_local! {
    static OVERRIDE_TEST_CHANGE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn take_override_test_change(expected: u8) -> bool {
    OVERRIDE_TEST_CHANGE.with(|change| {
        if change.get() == expected {
            change.set(0);
            true
        } else {
            false
        }
    })
}

pub(crate) fn save_bundle_declarations(
    manifest_path: &Path,
    bundles: &BTreeMap<String, BundleDependency>,
    overrides: &BTreeMap<String, BundleOverrideDeclaration>,
) -> Result<(), ServiceError> {
    let content = fs::read_to_string(manifest_path).map_err(ServiceError::Io)?;
    let mut document = content.parse::<toml_edit::DocumentMut>().map_err(|error| {
        ServiceError::Config(format!(
            "Failed to edit skill-project.toml bundle declarations: {error}"
        ))
    })?;
    if bundles.is_empty() {
        document.remove("bundles");
    } else {
        let replacement = toml::to_string_pretty(&BundleManifestTables {
            bundles: bundles.clone(),
            overrides: BTreeMap::new(),
        })
        .map_err(|error| {
            ServiceError::Config(format!("Failed to serialize bundle declarations: {error}"))
        })?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            ServiceError::Config(format!("Failed to edit bundle declarations: {error}"))
        })?;
        document["bundles"] = replacement["bundles"].clone();
    }
    if overrides.is_empty() {
        document.remove("overrides");
    } else {
        let replacement = toml::to_string_pretty(&BundleManifestTables {
            bundles: BTreeMap::new(),
            overrides: overrides.clone(),
        })
        .map_err(|error| ServiceError::Config(format!("Failed to serialize overrides: {error}")))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| ServiceError::Config(format!("Failed to edit overrides: {error}")))?;
        document["overrides"] = replacement["overrides"].clone();
    }
    atomic_write(manifest_path, document.to_string().as_bytes()).map_err(ServiceError::Io)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BundleOverrideDeclaration {
    pub(crate) origin: String,
}

#[derive(Debug, Default, Deserialize)]
struct SkillDependencyTable {
    #[serde(default)]
    dependencies: BTreeMap<String, DependencySpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct BundleProject {
    bundle: BundleDescriptor,
    #[serde(default)]
    dependencies: BTreeMap<String, DependencySpec>,
}

pub(super) fn parse_bundle_descriptor(bytes: &[u8]) -> Result<BundleDescriptor, ServiceError> {
    parse_bundle_project(bytes).map(|(descriptor, _)| descriptor)
}

pub(super) fn parse_bundle_project(
    bytes: &[u8],
) -> Result<(BundleDescriptor, BTreeMap<String, DependencySpec>), ServiceError> {
    let content = std::str::from_utf8(bytes).map_err(|error| {
        ServiceError::Validation(format!("Bundle skill-project.toml is not UTF-8: {error}"))
    })?;
    let value: toml::Value = toml::from_str(content).map_err(|error| {
        ServiceError::Validation(format!("Invalid bundle skill-project.toml: {error}"))
    })?;
    if value.get("bundle").is_none() {
        return Err(ServiceError::Validation(
            "skill-project.toml has no [bundle] declaration. Add [bundle] with format, id, version, and at least one [bundle.members.<skill-id>] entry; run `fastskill bundle build --help` for an example."
                .to_string(),
        ));
    }
    let project: BundleProject = value.try_into().map_err(|error| {
        ServiceError::Validation(format!("Invalid bundle skill-project.toml: {error}"))
    })?;
    project.bundle.validate(&project.dependencies)?;
    Ok((project.bundle, project.dependencies))
}

pub(crate) fn prepare_members(
    skills_directory: &Path,
    descriptor: &BundleDescriptor,
    dependencies: &BTreeMap<String, DependencySpec>,
) -> Result<BTreeMap<String, PreparedMember>, ServiceError> {
    let mut policies = descriptor.members.clone();
    let mut pending = policies
        .keys()
        .filter_map(|id| dependencies.get(id).cloned().map(|spec| (id.clone(), spec)))
        .collect::<VecDeque<_>>();
    let mut members = BTreeMap::new();
    while let Some((id, requirement)) = pending.pop_front() {
        let policy = policies.get(&id).cloned().ok_or_else(|| {
            ServiceError::Validation(format!("Bundle member '{id}' has no policy"))
        })?;
        let source = skills_directory.join(&id);
        if !source.join("SKILL.md").is_file() {
            return Err(ServiceError::Validation(format!(
                "Bundle member '{id}' is missing skills/{id}/SKILL.md"
            )));
        }
        validate_member_version(&id, &source, &requirement)?;
        let member_manifest = source.join("skill-project.toml");
        if member_manifest.is_file() {
            let content = fs::read_to_string(&member_manifest).map_err(ServiceError::Io)?;
            let child: SkillDependencyTable = toml::from_str(&content).map_err(|error| {
                ServiceError::Validation(format!("Invalid dependency manifest for '{id}': {error}"))
            })?;
            for (dependency, requirement) in child.dependencies {
                if !policies.contains_key(&dependency) {
                    policies.insert(dependency.clone(), BundleMemberPolicy::default());
                    pending.push_back((dependency, requirement));
                }
            }
        }
        members.insert(
            id.clone(),
            PreparedMember {
                id,
                digest: digest_directory(&source)?,
                source,
                overridable: policy.overridable,
            },
        );
    }
    Ok(members)
}

fn validate_member_version(
    id: &str,
    source: &Path,
    requirement: &DependencySpec,
) -> Result<(), ServiceError> {
    let constraint = match requirement {
        DependencySpec::Version(raw) => Some(VersionConstraint::parse(raw).map_err(|error| {
            ServiceError::Validation(format!(
                "Bundle dependency '{id}' has invalid version constraint '{raw}': {error}"
            ))
        })?),
        DependencySpec::Inline {
            origin:
                Origin::Repository {
                    version: Some(constraint),
                    ..
                },
            ..
        } => Some(constraint.clone()),
        DependencySpec::Inline { .. } => None,
    };
    let Some(constraint) = constraint else {
        return Ok(());
    };
    let skill = fs::read_to_string(source.join("SKILL.md")).map_err(ServiceError::Io)?;
    let metadata = crate::core::frontmatter::parse_skill_frontmatter(&skill).map_err(|error| {
        ServiceError::Validation(format!(
            "Bundle member '{id}' has invalid SKILL.md frontmatter: {error}"
        ))
    })?;
    let version = metadata.version.as_deref().unwrap_or("1.0.0");
    if constraint.satisfies(version).map_err(|error| {
        ServiceError::Validation(format!("Bundle member '{id}' has invalid version: {error}"))
    })? {
        return Ok(());
    }
    Err(ServiceError::Validation(format!(
        "Bundle member '{id}' is version {version}, which does not satisfy declared dependency {constraint}"
    )))
}

pub(crate) fn digest_release(
    descriptor: &BundleDescriptor,
    members: &BTreeMap<String, BundleArchiveLockMember>,
) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, BUNDLE_FORMAT);
    hash_field(&mut hasher, &descriptor.id);
    hash_field(&mut hasher, &descriptor.version);
    for (id, member) in members {
        hash_field(&mut hasher, id);
        hash_field(&mut hasher, &member.digest);
        hash_field(
            &mut hasher,
            if member.overridable {
                "overridable"
            } else {
                "required"
            },
        );
    }
    crate::utils::to_hex_lower(&hasher.finalize())
}

pub(crate) fn digest_directory(path: &Path) -> Result<String, ServiceError> {
    if !path.is_dir() {
        return Err(ServiceError::Validation(format!(
            "Expected skill directory at {}",
            path.display()
        )));
    }
    let mut hasher = Sha256::new();
    for entry in WalkDir::new(path).sort_by_file_name() {
        let entry = entry.map_err(|error| ServiceError::Io(io_error(error)))?;
        if entry.file_type().is_symlink() {
            return Err(ServiceError::Validation(format!(
                "Symbolic links are not permitted in bundle members: {}",
                entry.path().display()
            )));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(path).map_err(|error| {
            ServiceError::Custom(format!("Failed to form skill digest path: {error}"))
        })?;
        hash_field(&mut hasher, &relative.to_string_lossy().replace('\\', "/"));
        let mut file = fs::File::open(entry.path()).map_err(ServiceError::Io)?;
        let mut buffer = [0u8; 8192];
        loop {
            let read = file.read(&mut buffer).map_err(ServiceError::Io)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(crate::utils::to_hex_lower(&hasher.finalize()))
}

fn hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct BundleHistory {
    #[serde(default)]
    pub(crate) releases: BTreeMap<String, String>,
}

pub(crate) fn apply_personal_override(
    service: &BundleService,
    id: &str,
    source: &Path,
) -> Result<(), ServiceError> {
    let preview = preview_personal_override(service, id, source)?;
    if !preview.changed {
        return Ok(());
    }
    #[cfg(test)]
    if take_override_test_change(1) {
        fs::remove_file(source.join("SKILL.md")).map_err(ServiceError::Io)?;
    }
    SkillId::new(id.to_string())?;
    if !source.join("SKILL.md").is_file() {
        return Err(ServiceError::Validation(format!(
            "Personal override '{id}' must contain SKILL.md"
        )));
    }
    let source = source.canonicalize().map_err(ServiceError::Io)?;
    let digest = digest_directory(&source)?;
    if preview.target_revision.as_deref() != Some(digest.as_str()) {
        return Err(ServiceError::InvalidOperation(format!(
            "Personal override source for '{id}' changed while the operation was prepared; retry"
        )));
    }
    let manifest_path = service.project_root.join("skill-project.toml");
    #[cfg(test)]
    if take_override_test_change(2) {
        fs::write(&manifest_path, "invalid = [").map_err(ServiceError::Io)?;
    }
    let raw_manifest = fs::read_to_string(&manifest_path).map_err(ServiceError::Io)?;
    let mut tables: BundleManifestTables = toml::from_str(&raw_manifest).map_err(|error| {
        ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
    })?;
    let lock_path = service.project_root.join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path)
        .map_err(|error| ServiceError::Config(format!("Failed to load skills.lock: {error}")))?;
    #[cfg(test)]
    if take_override_test_change(3) {
        lock.bundles[0].members[0].overridable = false;
    }
    let owners = lock
        .bundles
        .iter()
        .flat_map(|bundle| bundle.members.iter())
        .filter(|member| member.id == id)
        .collect::<Vec<_>>();
    if owners.is_empty() {
        return Err(ServiceError::InvalidOperation(format!(
            "Skill '{id}' is not owned by an installed bundle"
        )));
    }
    if owners.iter().any(|member| !member.overridable) {
        return Err(ServiceError::InvalidOperation(format!(
            "Every bundle owning '{id}' must permit a personal override"
        )));
    }
    let state_guard = StateMutationGuard::acquire_for(
        &service.project_root,
        Some(&service.skills_directory),
        "bundle override",
    )?;
    #[cfg(test)]
    if take_override_test_change(4) {
        fs::write(&lock_path, "invalid = [").map_err(ServiceError::Io)?;
    }
    #[cfg(test)]
    if take_override_test_change(5) {
        fs::write(&manifest_path, "invalid = [").map_err(ServiceError::Io)?;
    }
    #[cfg(test)]
    if take_override_test_change(6) {
        let mut changed = lock.clone();
        changed.covered_roots.push("concurrent".to_string());
        changed
            .save_to_file(&lock_path)
            .map_err(|error| ServiceError::Config(error.to_string()))?;
    }
    let current_lock = match ProjectSkillsLock::load_from_file(&lock_path) {
        Ok(lock) => lock,
        Err(error) => {
            state_guard.recovered()?;
            return Err(ServiceError::Config(format!(
                "Failed to reload skills.lock before override: {error}"
            )));
        }
    };
    let current_tables = match fs::read_to_string(&manifest_path)
        .map_err(ServiceError::Io)
        .and_then(|content| {
            toml::from_str::<BundleManifestTables>(&content).map_err(|error| {
                ServiceError::Config(format!("Failed to reload bundle declarations: {error}"))
            })
        }) {
        Ok(tables) => tables,
        Err(error) => {
            state_guard.recovered()?;
            return Err(error);
        }
    };
    if current_lock.bundles != lock.bundles
        || current_lock.overrides != lock.overrides
        || current_lock.skills != lock.skills
        || current_lock.covered_roots != lock.covered_roots
        || current_tables != tables
    {
        state_guard.recovered()?;
        return Err(ServiceError::InvalidOperation(format!(
            "Ownership for '{id}' changed while the override was being prepared; retry"
        )));
    }
    lock = current_lock;
    tables = current_tables;
    #[cfg(test)]
    if take_override_test_change(7) {
        fs::remove_file(source.join("SKILL.md")).map_err(ServiceError::Io)?;
    }
    let current_digest = match digest_directory(&source) {
        Ok(current_digest) => current_digest,
        Err(error) => {
            state_guard.recovered()?;
            return Err(error);
        }
    };
    if current_digest != digest {
        state_guard.recovered()?;
        return Err(ServiceError::InvalidOperation(format!(
            "Personal override source for '{id}' changed while the operation was prepared; retry"
        )));
    }
    let ids = vec![id.to_string()];
    let transaction = match BundleTransaction::capture(
        &service.skills_directory,
        &ids,
        &[manifest_path.clone(), lock_path.clone()],
    ) {
        Ok(transaction) => transaction,
        Err(error) => {
            state_guard.recovered()?;
            return Err(error);
        }
    };
    let result = (|| {
        replace_skill_directory(&service.skills_directory.join(id), &source)?;
        tables.overrides.insert(
            id.to_string(),
            BundleOverrideDeclaration {
                origin: source.display().to_string(),
            },
        );
        lock.overrides.retain(|entry| entry.id != id);
        lock.overrides.push(ProjectLockedPersonalOverride {
            id: id.to_string(),
            origin: source.display().to_string(),
            digest,
        });
        save_bundle_declarations(&manifest_path, &tables.bundles, &tables.overrides)?;
        lock.save_to_file(&lock_path)
            .map_err(|error| ServiceError::Config(format!("Failed to save skills.lock: {error}")))
    })();
    if let Err(error) = result {
        if let Err(recovery_error) = transaction.rollback() {
            return Err(ServiceError::Custom(format!(
                "Override failed: {error}; recovery also failed: {recovery_error}"
            )));
        }
        state_guard.recovered()?;
        return Err(error);
    }
    transaction.commit();
    state_guard.commit()?;
    Ok(())
}

pub(crate) fn preview_personal_override(
    service: &BundleService,
    id: &str,
    source: &Path,
) -> Result<BundleOverridePreview, ServiceError> {
    SkillId::new(id.to_string())?;
    if !source.join("SKILL.md").is_file() {
        return Err(ServiceError::Validation(format!(
            "Personal override '{id}' must contain SKILL.md"
        )));
    }
    let source = source.canonicalize().map_err(ServiceError::Io)?;
    let target = digest_directory(&source)?;
    let manifest_path = service.project_root.join("skill-project.toml");
    let content = fs::read_to_string(&manifest_path).map_err(ServiceError::Io)?;
    let tables: BundleManifestTables = toml::from_str(&content).map_err(|error| {
        ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
    })?;
    let lock_path = service.project_root.join("skills.lock");
    let lock = ProjectSkillsLock::load_from_file(&lock_path)
        .map_err(|error| ServiceError::Config(format!("Failed to load skills.lock: {error}")))?;
    let owners = lock
        .bundles
        .iter()
        .flat_map(|bundle| bundle.members.iter())
        .filter(|member| member.id == id)
        .collect::<Vec<_>>();
    if owners.is_empty() {
        return Err(ServiceError::InvalidOperation(format!(
            "Skill '{id}' is not owned by an installed bundle"
        )));
    }
    if owners.iter().any(|member| !member.overridable) {
        return Err(ServiceError::InvalidOperation(format!(
            "Every bundle owning '{id}' must permit a personal override"
        )));
    }
    let owner_digest = &owners[0].digest;
    if owners.iter().any(|member| member.digest != *owner_digest) {
        return Err(ServiceError::InvalidOperation(format!(
            "Bundle owners of '{id}' disagree on packaged contents"
        )));
    }
    let existing_override = lock.overrides.iter().find(|entry| entry.id == id);
    match (tables.overrides.get(id), existing_override) {
        (None, None) => {}
        (Some(declaration), Some(locked)) if declaration.origin == locked.origin => {}
        _ => {
            return Err(ServiceError::Config(format!(
                "Personal override '{id}' is inconsistent between the Manifest and Lock"
            )));
        }
    }
    let current = existing_override
        .map(|entry| entry.digest.clone())
        .unwrap_or_else(|| owner_digest.clone());
    let installed = service.skills_directory.join(id);
    if installed.exists() && digest_directory(&installed)? != current {
        return Err(ServiceError::InvalidOperation(format!(
            "Skill '{id}' is locally modified; FastSkill will not discard those edits while setting an override"
        )));
    }
    let changed = current != target || existing_override.is_none();
    Ok(BundleOverridePreview {
        id: id.to_string(),
        current_revision: Some(current),
        target_revision: Some(target),
        changed,
        changes: if changed {
            vec![
                "content".to_string(),
                "manifest".to_string(),
                "lock".to_string(),
            ]
        } else {
            Vec::new()
        },
        retained: Vec::new(),
    })
}

pub(crate) fn restore_personal_overrides(service: &BundleService) -> Result<(), ServiceError> {
    restore_personal_overrides_impl(service, None)
}

pub(crate) fn restore_personal_overrides_with_guard(
    service: &BundleService,
    guard: &StateMutationGuard,
) -> Result<(), ServiceError> {
    restore_personal_overrides_impl(service, Some(guard))
}

fn restore_personal_overrides_impl(
    service: &BundleService,
    existing_guard: Option<&StateMutationGuard>,
) -> Result<(), ServiceError> {
    let manifest_path = service.project_root.join("skill-project.toml");
    let raw_manifest = fs::read_to_string(&manifest_path).map_err(ServiceError::Io)?;
    let mut tables: BundleManifestTables = toml::from_str(&raw_manifest).map_err(|error| {
        ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
    })?;
    if tables.overrides.is_empty() {
        return Ok(());
    }
    let lock_path = service.project_root.join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path)
        .map_err(|error| ServiceError::Config(format!("Failed to load skills.lock: {error}")))?;
    let ids = tables.overrides.keys().cloned().collect::<Vec<_>>();
    let state_guard = StateMutationLease::acquire_or_borrow(
        &service.project_root,
        Some(&service.skills_directory),
        "restore overrides",
        existing_guard,
    )?;
    #[cfg(test)]
    if take_override_test_change(8) {
        fs::write(&manifest_path, "invalid = [").map_err(ServiceError::Io)?;
    }
    #[cfg(test)]
    if take_override_test_change(9) {
        fs::write(&lock_path, "invalid = [").map_err(ServiceError::Io)?;
    }
    #[cfg(test)]
    if take_override_test_change(10) {
        let mut changed = lock.clone();
        changed.bundles.clear();
        changed
            .save_to_file(&lock_path)
            .map_err(|error| ServiceError::Config(error.to_string()))?;
    }
    let current_tables = match fs::read_to_string(&manifest_path)
        .map_err(ServiceError::Io)
        .and_then(|content| {
            toml::from_str::<BundleManifestTables>(&content).map_err(|error| {
                ServiceError::Config(format!("Failed to reload bundle declarations: {error}"))
            })
        }) {
        Ok(tables) => tables,
        Err(error) => {
            state_guard.recovered()?;
            return Err(error);
        }
    };
    let current_lock = match ProjectSkillsLock::load_from_file(&lock_path) {
        Ok(lock) => lock,
        Err(error) => {
            state_guard.recovered()?;
            return Err(ServiceError::Config(format!(
                "Failed to reload skills.lock before restoring overrides: {error}"
            )));
        }
    };
    if current_tables != tables
        || current_lock.bundles != lock.bundles
        || current_lock.overrides != lock.overrides
    {
        state_guard.recovered()?;
        return Err(ServiceError::InvalidOperation(
            "Bundle ownership changed while overrides were being prepared; retry".to_string(),
        ));
    }
    tables = current_tables;
    lock = current_lock;
    let transaction = match BundleTransaction::capture(&service.skills_directory, &ids, &[]) {
        Ok(transaction) => transaction,
        Err(error) => {
            state_guard.recovered()?;
            return Err(error);
        }
    };
    let result = (|| {
        for (id, declaration) in &tables.overrides {
            let locked = lock
                .overrides
                .iter()
                .find(|entry| entry.id == *id)
                .ok_or_else(|| {
                    ServiceError::Config(format!(
                        "Personal override '{id}' is missing from skills.lock"
                    ))
                })?;
            if locked.origin != declaration.origin {
                return Err(ServiceError::Config(format!(
                    "Personal override '{id}' has different Manifest and Lock origins"
                )));
            }
            let source = PathBuf::from(&declaration.origin);
            if !source.join("SKILL.md").is_file() {
                return Err(ServiceError::InvalidOperation(format!(
                    "Personal override '{id}' source is unavailable: {}",
                    source.display()
                )));
            }
            if digest_directory(&source)? != locked.digest {
                return Err(ServiceError::InvalidOperation(format!(
                    "Personal override '{id}' no longer matches its locked digest"
                )));
            }
            replace_skill_directory(&service.skills_directory.join(id), &source)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(recovery_error) = transaction.rollback() {
            return Err(ServiceError::Custom(format!(
                "Override restoration failed: {error}; recovery also failed: {recovery_error}"
            )));
        }
        state_guard.recovered()?;
        return Err(error);
    }
    transaction.commit();
    state_guard.commit()?;
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), ServiceError> {
    for entry in WalkDir::new(source).sort_by_file_name() {
        let entry = entry.map_err(|error| ServiceError::Io(io_error(error)))?;
        if entry.file_type().is_symlink() {
            return Err(ServiceError::Validation(format!(
                "Symbolic links are not permitted in bundle members: {}",
                entry.path().display()
            )));
        }
        let relative = entry.path().strip_prefix(source).map_err(|error| {
            ServiceError::Custom(format!("Failed to form copied skill path: {error}"))
        })?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(ServiceError::Io)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(ServiceError::Io)?;
            }
            fs::copy(entry.path(), &target).map_err(ServiceError::Io)?;
        }
    }
    Ok(())
}

pub(crate) fn replace_skill_directory(
    destination: &Path,
    source: &Path,
) -> Result<(), ServiceError> {
    let parent = destination.parent().ok_or_else(|| {
        ServiceError::InvalidOperation(format!(
            "Skill destination has no parent: {}",
            destination.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(ServiceError::Io)?;
    let stage = tempfile::Builder::new()
        .prefix(".fastskill-bundle-stage-")
        .tempdir_in(parent)
        .map_err(ServiceError::Io)?;
    let staged = stage.path().join("skill");
    copy_directory(source, &staged)?;
    remove_skill_directory(destination)?;
    fs::rename(&staged, destination).map_err(ServiceError::Io)
}

pub(crate) fn remove_skill_directory(destination: &Path) -> Result<(), ServiceError> {
    if !destination.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(destination).map_err(ServiceError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ServiceError::Validation(format!(
            "Refusing to remove non-directory skill destination: {}",
            destination.display()
        )));
    }
    fs::remove_dir_all(destination).map_err(ServiceError::Io)
}

pub(crate) struct BundleTransaction {
    temporary: TempDir,
    skill_backups: Vec<(PathBuf, Option<PathBuf>)>,
    file_backups: Vec<(PathBuf, Option<PathBuf>)>,
}

#[derive(Debug)]
pub(crate) struct BundleRollbackError {
    source: ServiceError,
    backup_path: PathBuf,
}

impl BundleRollbackError {
    #[cfg(test)]
    pub(crate) fn backup_path(&self) -> &Path {
        &self.backup_path
    }
}

impl std::fmt::Display for BundleRollbackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}; recovery inputs retained at {}",
            self.source,
            self.backup_path.display()
        )
    }
}

impl std::error::Error for BundleRollbackError {}

#[derive(Serialize)]
struct BundleRecoveryPath {
    kind: &'static str,
    destination: String,
    backup: Option<String>,
}

impl BundleTransaction {
    pub(crate) fn capture(
        skills_directory: &Path,
        ids: &[String],
        files: &[PathBuf],
    ) -> Result<Self, ServiceError> {
        let temporary = TempDir::new().map_err(ServiceError::Io)?;
        let mut skill_backups = Vec::new();
        for id in ids {
            let destination = skills_directory.join(id);
            let backup = if destination.exists() {
                let backup = temporary.path().join("skills").join(id);
                copy_directory(&destination, &backup)?;
                Some(backup)
            } else {
                None
            };
            skill_backups.push((destination, backup));
        }
        let files_root = temporary.path().join("files");
        fs::create_dir_all(&files_root).map_err(ServiceError::Io)?;
        let mut file_backups = Vec::with_capacity(files.len());
        for (index, file) in files.iter().enumerate() {
            let backup = if file.exists() {
                let backup = files_root.join(index.to_string());
                fs::copy(file, &backup).map_err(ServiceError::Io)?;
                Some(backup)
            } else {
                None
            };
            file_backups.push((file.clone(), backup));
        }
        write_bundle_recovery_mapping(&temporary, &skill_backups, &file_backups)?;
        Ok(Self {
            temporary,
            skill_backups,
            file_backups,
        })
    }

    pub(crate) fn rollback(self) -> Result<(), BundleRollbackError> {
        for (destination, backup) in &self.skill_backups {
            let result = remove_skill_directory(destination).and_then(|()| {
                if let Some(backup) = backup {
                    copy_directory(backup, destination)?;
                }
                Ok(())
            });
            if let Err(source) = result {
                return Err(BundleRollbackError {
                    source,
                    backup_path: self.temporary.keep(),
                });
            }
        }
        for (path, content) in &self.file_backups {
            let result = if let Some(content) = content {
                fs::read(content)
                    .map_err(ServiceError::Io)
                    .and_then(|content| atomic_write(path, &content).map_err(ServiceError::Io))
            } else if path.exists() {
                fs::remove_file(path).map_err(ServiceError::Io)
            } else {
                Ok(())
            };
            if let Err(source) = result {
                return Err(BundleRollbackError {
                    source,
                    backup_path: self.temporary.keep(),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn commit(self) {}
}

fn write_bundle_recovery_mapping(
    temporary: &TempDir,
    skill_backups: &[(PathBuf, Option<PathBuf>)],
    file_backups: &[(PathBuf, Option<PathBuf>)],
) -> Result<(), ServiceError> {
    let paths = skill_backups
        .iter()
        .map(|(destination, backup)| ("skill", destination, backup))
        .chain(
            file_backups
                .iter()
                .map(|(destination, backup)| ("file", destination, backup)),
        )
        .map(|(kind, destination, backup)| {
            let backup = backup
                .as_ref()
                .map(|path| {
                    path.strip_prefix(temporary.path())
                        .map(|path| path.display().to_string())
                        .map_err(|error| {
                            ServiceError::Custom(format!("Failed to record bundle backup: {error}"))
                        })
                })
                .transpose()?;
            Ok(BundleRecoveryPath {
                kind,
                destination: destination.display().to_string(),
                backup,
            })
        })
        .collect::<Result<Vec<_>, ServiceError>>()?;
    let content = serde_json::to_vec_pretty(&paths).map_err(|error| {
        ServiceError::Custom(format!("Failed to serialize bundle recovery map: {error}"))
    })?;
    fs::write(temporary.path().join("recovery-map.json"), content).map_err(ServiceError::Io)
}

fn io_error(error: walkdir::Error) -> std::io::Error {
    std::io::Error::other(error)
}

#[cfg(test)]
#[path = "bundle_persistence_tests.rs"]
#[allow(clippy::unwrap_used)]
mod tests;

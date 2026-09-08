use crate::core::bundle::{
    BundleArchiveLockMember, BundleDependency, BundleDescriptor, BundleManifestTables,
    BundleMemberPolicy, BundleService, PreparedMember, BUNDLE_FORMAT,
};
use crate::core::lock::{ProjectLockedPersonalOverride, ProjectSkillsLock};
use crate::core::manifest::DependencySpec;
use crate::core::origin::Origin;
use crate::core::service::{ServiceError, SkillId};
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    SkillId::new(id.to_string())?;
    if !source.join("SKILL.md").is_file() {
        return Err(ServiceError::Validation(format!(
            "Personal override '{id}' must contain SKILL.md"
        )));
    }
    let source = source.canonicalize().map_err(ServiceError::Io)?;
    let digest = digest_directory(&source)?;
    let manifest_path = service.project_root.join("skill-project.toml");
    let raw_manifest = fs::read_to_string(&manifest_path).map_err(ServiceError::Io)?;
    let mut tables: BundleManifestTables = toml::from_str(&raw_manifest).map_err(|error| {
        ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
    })?;
    let lock_path = service.project_root.join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path)
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
    let ids = vec![id.to_string()];
    let mut transaction = BundleTransaction::capture(
        &service.skills_directory,
        &ids,
        &[manifest_path.clone(), lock_path.clone()],
    )?;
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
        transaction.rollback()?;
        return Err(error);
    }
    transaction.commit();
    Ok(())
}

pub(crate) fn restore_personal_overrides(service: &BundleService) -> Result<(), ServiceError> {
    let manifest_path = service.project_root.join("skill-project.toml");
    let raw_manifest = fs::read_to_string(&manifest_path).map_err(ServiceError::Io)?;
    let tables: BundleManifestTables = toml::from_str(&raw_manifest).map_err(|error| {
        ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
    })?;
    if tables.overrides.is_empty() {
        return Ok(());
    }
    let lock_path = service.project_root.join("skills.lock");
    let lock = ProjectSkillsLock::load_from_file(&lock_path)
        .map_err(|error| ServiceError::Config(format!("Failed to load skills.lock: {error}")))?;
    let ids = tables.overrides.keys().cloned().collect::<Vec<_>>();
    let mut transaction = BundleTransaction::capture(&service.skills_directory, &ids, &[])?;
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
        transaction.rollback()?;
        return Err(error);
    }
    transaction.commit();
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
    _temporary: TempDir,
    skill_backups: Vec<(PathBuf, Option<PathBuf>)>,
    file_backups: Vec<(PathBuf, Option<Vec<u8>>)>,
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
        let file_backups = files
            .iter()
            .map(|file| {
                Ok((
                    file.clone(),
                    file.exists().then(|| fs::read(file)).transpose()?,
                ))
            })
            .collect::<Result<Vec<_>, std::io::Error>>()
            .map_err(ServiceError::Io)?;
        Ok(Self {
            _temporary: temporary,
            skill_backups,
            file_backups,
        })
    }

    pub(crate) fn rollback(&mut self) -> Result<(), ServiceError> {
        for (destination, backup) in &self.skill_backups {
            remove_skill_directory(destination)?;
            if let Some(backup) = backup {
                copy_directory(backup, destination)?;
            }
        }
        for (path, content) in &self.file_backups {
            if let Some(content) = content {
                atomic_write(path, content).map_err(ServiceError::Io)?;
            } else if path.exists() {
                fs::remove_file(path).map_err(ServiceError::Io)?;
            }
        }
        Ok(())
    }

    pub(crate) fn commit(self) {}
}

fn io_error(error: walkdir::Error) -> std::io::Error {
    std::io::Error::other(error)
}

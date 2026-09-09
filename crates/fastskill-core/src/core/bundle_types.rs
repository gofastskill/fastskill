use crate::core::bundle::BUNDLE_FORMAT;
use crate::core::bundle_archive::BundleArchiveLock;
use crate::core::bundle_persistence::{
    parse_bundle_project, prepare_members, BundleOverrideDeclaration,
};
use crate::core::lock::ProjectLockedBundleEntry;
use crate::core::manifest::{DependencySpec, SkillProjectToml};
use crate::core::service::{ServiceError, SkillId};
use crate::storage::zip::ZipHandler;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Validated effects of removing an installed bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleRemovalPreview {
    pub id: String,
    pub current_revision: String,
    pub delete_members: Vec<String>,
    pub retained_members: Vec<String>,
    pub promoted_overrides: Vec<String>,
}

/// Validated effects of setting or resetting a personal bundle override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleOverridePreview {
    pub id: String,
    pub current_revision: Option<String>,
    pub target_revision: Option<String>,
    pub changed: bool,
    pub changes: Vec<String>,
    pub retained: Vec<String>,
}

/// Validated effects of replacing one installed bundle release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleUpdatePreview {
    pub id: String,
    pub current_revision: String,
    pub target_revision: String,
    pub changes: Vec<String>,
    pub retained: Vec<String>,
}

/// Validated effects of adding a bundle artifact to a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleInstallPreview {
    pub id: String,
    pub current_revision: Option<String>,
    pub target_revision: String,
    pub changes: Vec<String>,
    pub retained: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
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
pub(crate) struct ProjectBundleManifest {
    pub(crate) project: SkillProjectToml,
    pub(crate) bundles: BTreeMap<String, BundleDependency>,
    pub(crate) overrides: BTreeMap<String, BundleOverrideDeclaration>,
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
    pub(crate) fn validate(
        &self,
        dependencies: &BTreeMap<String, DependencySpec>,
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
pub(crate) struct PreparedBundle {
    _temporary: TempDir,
    pub(crate) descriptor: BundleDescriptor,
    pub(crate) members: BTreeMap<String, PreparedMember>,
    pub(crate) release_digest: String,
}

impl PreparedBundle {
    pub(crate) fn load(artifact: &Path) -> Result<Self, ServiceError> {
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

    pub(crate) fn verify_locked_release(
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

use crate::core::bundle::{BundleArchiveLockMember, BundleDescriptor, PreparedMember};
use crate::core::bundle_persistence::digest_release;
use crate::core::service::ServiceError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;

const BUNDLE_LOCK_FORMAT: &str = "fastskill-bundle-lock-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct BundleArchiveLock {
    format: String,
    id: String,
    version: String,
    release_digest: String,
    members: BTreeMap<String, BundleArchiveLockMember>,
}

impl BundleArchiveLock {
    pub(super) fn from_members(
        descriptor: &BundleDescriptor,
        members: &BTreeMap<String, PreparedMember>,
    ) -> Self {
        let members: BTreeMap<_, _> = members
            .iter()
            .map(|(id, member)| {
                (
                    id.clone(),
                    BundleArchiveLockMember {
                        digest: member.digest.clone(),
                        overridable: member.overridable,
                    },
                )
            })
            .collect();
        let release_digest = digest_release(descriptor, &members);
        Self {
            format: BUNDLE_LOCK_FORMAT.to_string(),
            id: descriptor.id.clone(),
            version: descriptor.version.clone(),
            release_digest,
            members,
        }
    }

    pub(super) fn release_digest(&self) -> &str {
        &self.release_digest
    }

    pub(super) fn verify(
        &self,
        descriptor: &BundleDescriptor,
        members: &BTreeMap<String, PreparedMember>,
    ) -> Result<(), ServiceError> {
        if self.format != BUNDLE_LOCK_FORMAT
            || self.id != descriptor.id
            || self.version != descriptor.version
        {
            return Err(ServiceError::Validation(
                "Bundle skills.lock does not match bundle identity and version".to_string(),
            ));
        }
        let expected = Self::from_members(descriptor, members);
        if self.members != expected.members || self.release_digest != expected.release_digest {
            return Err(ServiceError::Validation(
                "Bundle contents do not match the digests in skills.lock".to_string(),
            ));
        }
        Ok(())
    }
}

pub(super) fn write_bundle_archive(
    artifact: &Path,
    manifest: &[u8],
    archive_lock: &BundleArchiveLock,
    members: &BTreeMap<String, PreparedMember>,
) -> Result<(), ServiceError> {
    let file = fs::File::create(artifact).map_err(ServiceError::Io)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    write_zip_file(&mut writer, "skill-project.toml", manifest, options)?;
    let lock_content = toml::to_string_pretty(archive_lock).map_err(|error| {
        ServiceError::Config(format!("Failed to serialize bundle skills.lock: {error}"))
    })?;
    write_zip_file(&mut writer, "skills.lock", lock_content.as_bytes(), options)?;
    for member in members.values() {
        for entry in WalkDir::new(&member.source).sort_by_file_name() {
            let entry = entry.map_err(|error| ServiceError::Io(std::io::Error::other(error)))?;
            if entry.file_type().is_symlink() {
                return Err(ServiceError::Validation(format!(
                    "Bundle member '{}' contains a symbolic link: {}",
                    member.id,
                    entry.path().display()
                )));
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry.path().strip_prefix(&member.source).map_err(|error| {
                ServiceError::Custom(format!("Failed to form bundle member path: {error}"))
            })?;
            let archive_path = format!(
                "skills/{}/{}",
                member.id,
                relative.to_string_lossy().replace('\\', "/")
            );
            write_zip_file(
                &mut writer,
                &archive_path,
                &fs::read(entry.path()).map_err(ServiceError::Io)?,
                options,
            )?;
        }
    }
    writer.finish().map_err(|error| {
        ServiceError::Validation(format!("Failed to finish bundle ZIP: {error}"))
    })?;
    Ok(())
}

fn write_zip_file(
    writer: &mut zip::ZipWriter<fs::File>,
    name: &str,
    content: &[u8],
    options: zip::write::SimpleFileOptions,
) -> Result<(), ServiceError> {
    writer.start_file(name, options).map_err(|error| {
        ServiceError::Validation(format!(
            "Failed to write bundle ZIP entry '{name}': {error}"
        ))
    })?;
    writer.write_all(content).map_err(ServiceError::Io)
}

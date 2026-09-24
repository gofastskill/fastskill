use crate::core::bundle::{BundleArchiveLockMember, BundleDescriptor, PreparedMember};
use crate::core::bundle_persistence::digest_release;
use crate::core::content_digest::DigestForms;
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
    /// The archive lock a new artifact carries: every digest in the current form.
    pub(super) fn from_members(
        descriptor: &BundleDescriptor,
        members: &BTreeMap<String, PreparedMember>,
    ) -> Self {
        Self::with_member_digests(descriptor, members, |forms| &forms.current)
    }

    fn with_member_digests(
        descriptor: &BundleDescriptor,
        members: &BTreeMap<String, PreparedMember>,
        form: fn(&DigestForms) -> &String,
    ) -> Self {
        let members: BTreeMap<_, _> = members
            .iter()
            .map(|(id, member)| {
                (
                    id.clone(),
                    BundleArchiveLockMember {
                        digest: form(&member.digest).clone(),
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

    /// The release digest of `members` in both forms: the current form fastskill records,
    /// and the legacy form older releases recorded in Locks and bundle history.
    pub(super) fn release_digests(
        descriptor: &BundleDescriptor,
        members: &BTreeMap<String, PreparedMember>,
    ) -> DigestForms {
        DigestForms {
            current: Self::from_members(descriptor, members).release_digest,
            legacy: Self::with_member_digests(descriptor, members, |forms| &forms.legacy)
                .release_digest,
        }
    }

    /// Check the archive lock against the extracted members. An artifact built before
    /// ADR-0017 declares every digest in the legacy form; it is accepted with a warning. An
    /// archive lock that mixes forms matches neither and is refused.
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
        if self.declares(&Self::from_members(descriptor, members)) {
            return Ok(());
        }
        let legacy = Self::with_member_digests(descriptor, members, |forms| &forms.legacy);
        if self.declares(&legacy) {
            crate::utils::warn_once(
                "legacy-bundle-artifact",
                "bundle artifact was built by an older fastskill and declares legacy content \
                 digests; rebuild it with `fastskill bundle build` (ADR-0017)",
            );
            return Ok(());
        }
        Err(ServiceError::Validation(
            "Bundle contents do not match the digests in skills.lock".to_string(),
        ))
    }

    fn declares(&self, expected: &Self) -> bool {
        self.members == expected.members && self.release_digest == expected.release_digest
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
    let metadata_options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    write_zip_file(
        &mut writer,
        "skill-project.toml",
        manifest,
        metadata_options,
    )?;
    let lock_content = toml::to_string_pretty(archive_lock).map_err(|error| {
        ServiceError::Config(format!("Failed to serialize bundle skills.lock: {error}"))
    })?;
    write_zip_file(
        &mut writer,
        "skills.lock",
        lock_content.as_bytes(),
        metadata_options,
    )?;
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
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(
                        crate::utils::package_file_mode(entry.path()).map_err(ServiceError::Io)?,
                    ),
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

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::bundle::{BundleMemberPolicy, BUNDLE_FORMAT};
    use std::os::unix::fs::PermissionsExt;

    fn forms(current: &str) -> DigestForms {
        DigestForms {
            current: current.to_string(),
            legacy: format!("legacy-{current}"),
        }
    }

    #[test]
    fn bundle_archive_preserves_the_executable_bit_for_member_files() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("demo");
        std::fs::create_dir_all(source.join("scripts")).unwrap();
        std::fs::write(source.join("SKILL.md"), "skill").unwrap();
        let script = source.join("scripts/run.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let descriptor = BundleDescriptor {
            format_marker: BUNDLE_FORMAT.to_string(),
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            members: BTreeMap::from([("demo".to_string(), BundleMemberPolicy::default())]),
        };
        let members = BTreeMap::from([(
            "demo".to_string(),
            PreparedMember {
                id: "demo".to_string(),
                source,
                digest: forms("digest"),
                overridable: false,
            },
        )]);
        let lock = BundleArchiveLock::from_members(&descriptor, &members);
        let artifact = root.path().join("bundle.zip");

        write_bundle_archive(&artifact, b"manifest", &lock, &members).unwrap();

        let mut archive = zip::ZipArchive::new(std::fs::File::open(artifact).unwrap()).unwrap();
        assert_eq!(
            archive
                .by_name("skills/demo/scripts/run.sh")
                .unwrap()
                .unix_mode()
                .unwrap()
                & 0o777,
            0o755
        );
    }

    #[test]
    fn bundle_lock_verification_rejects_identity_and_content_changes() {
        let descriptor = BundleDescriptor {
            format_marker: BUNDLE_FORMAT.to_string(),
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            members: BTreeMap::from([("demo".to_string(), BundleMemberPolicy::default())]),
        };
        let members = BTreeMap::from([(
            "demo".to_string(),
            PreparedMember {
                id: "demo".to_string(),
                source: std::path::PathBuf::from("demo"),
                digest: forms("digest"),
                overridable: false,
            },
        )]);
        let lock = BundleArchiveLock::from_members(&descriptor, &members);
        lock.verify(&descriptor, &members).unwrap();

        let mut wrong_identity = descriptor.clone();
        wrong_identity.version = "2.0.0".to_string();
        assert!(matches!(
            lock.verify(&wrong_identity, &members),
            Err(ServiceError::Validation(message))
                if message.contains("does not match bundle identity")
        ));

        let mut changed_members = members;
        changed_members.get_mut("demo").unwrap().digest = forms("changed");
        assert!(matches!(
            lock.verify(&descriptor, &changed_members),
            Err(ServiceError::Validation(message))
                if message.contains("do not match the digests")
        ));
    }

    #[test]
    fn bundle_archive_rejects_symlinks_and_invalid_artifact_targets() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("demo");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("SKILL.md"), "skill").unwrap();
        std::os::unix::fs::symlink(source.join("SKILL.md"), source.join("linked")).unwrap();
        let descriptor = BundleDescriptor {
            format_marker: BUNDLE_FORMAT.to_string(),
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            members: BTreeMap::from([("demo".to_string(), BundleMemberPolicy::default())]),
        };
        let members = BTreeMap::from([(
            "demo".to_string(),
            PreparedMember {
                id: "demo".to_string(),
                source,
                digest: forms("digest"),
                overridable: false,
            },
        )]);
        let lock = BundleArchiveLock::from_members(&descriptor, &members);

        assert!(matches!(
            write_bundle_archive(&root.path().join("bundle.zip"), b"manifest", &lock, &members),
            Err(ServiceError::Validation(message)) if message.contains("symbolic link")
        ));
        assert!(matches!(
            write_bundle_archive(root.path(), b"manifest", &lock, &members),
            Err(ServiceError::Io(_))
        ));
    }

    #[test]
    fn bundle_lock_verification_accepts_an_all_legacy_lock_but_not_a_mixed_one() {
        let descriptor = BundleDescriptor {
            format_marker: BUNDLE_FORMAT.to_string(),
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            members: BTreeMap::from([
                ("one".to_string(), BundleMemberPolicy::default()),
                ("two".to_string(), BundleMemberPolicy::default()),
            ]),
        };
        let member = |id: &str| PreparedMember {
            id: id.to_string(),
            source: std::path::PathBuf::from(id),
            digest: forms(id),
            overridable: false,
        };
        let members = BTreeMap::from([
            ("one".to_string(), member("one")),
            ("two".to_string(), member("two")),
        ]);
        let current = BundleArchiveLock::from_members(&descriptor, &members);
        let legacy =
            BundleArchiveLock::with_member_digests(&descriptor, &members, |forms| &forms.legacy);
        assert_ne!(current.release_digest, legacy.release_digest);
        legacy.verify(&descriptor, &members).unwrap();
        assert_eq!(
            BundleArchiveLock::release_digests(&descriptor, &members),
            DigestForms {
                current: current.release_digest.clone(),
                legacy: legacy.release_digest.clone(),
            }
        );

        let mut mixed = legacy.clone();
        mixed.members.get_mut("one").unwrap().digest = "one".to_string();
        mixed.release_digest = digest_release(&descriptor, &mixed.members);
        assert!(matches!(
            mixed.verify(&descriptor, &members),
            Err(ServiceError::Validation(message))
                if message.contains("do not match the digests")
        ));
    }
}

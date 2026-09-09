//! Resetting a personal bundle-member override.

use crate::core::bundle::{
    BundleManifestTables, BundleOverridePreview, BundleService, PreparedBundle,
};
use crate::core::bundle_persistence::{
    digest_directory, replace_skill_directory, save_bundle_declarations, BundleOverrideDeclaration,
    BundleTransaction,
};
use crate::core::lock::{ProjectLockedSkillEntry, ProjectSkillsLock};
use crate::core::manifest::{DependenciesSection, DependencySpec, SkillProjectToml};
use crate::core::origin::{Origin, Resolved};
use crate::core::service::{ServiceError, SkillId};
use crate::core::state_guard::StateMutationGuard;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn retain_final_personal_overrides(
    project_root: &Path,
    removed_bundle: &str,
    project: &mut SkillProjectToml,
    declarations: &mut BTreeMap<String, BundleOverrideDeclaration>,
    lock: &mut ProjectSkillsLock,
) -> Result<(), ServiceError> {
    let final_overrides = lock
        .overrides
        .iter()
        .filter(|override_entry| {
            let owners = lock
                .bundles
                .iter()
                .filter(|bundle| {
                    bundle
                        .members
                        .iter()
                        .any(|member| member.id == override_entry.id)
                })
                .map(|bundle| bundle.id.as_str())
                .collect::<Vec<_>>();
            owners == vec![removed_bundle]
        })
        .cloned()
        .collect::<Vec<_>>();
    if final_overrides.is_empty() {
        return Ok(());
    }
    let dependencies = project
        .dependencies
        .get_or_insert_with(|| DependenciesSection {
            dependencies: HashMap::new(),
        });
    for override_entry in &final_overrides {
        let absolute_origin = Origin::Local {
            path: PathBuf::from(&override_entry.origin),
            editable: false,
        };
        let (origin, _) = absolute_origin.to_manifest_relative(project_root);
        dependencies.dependencies.insert(
            override_entry.id.clone(),
            DependencySpec::Inline {
                origin: origin.clone(),
                groups: None,
            },
        );
        let skill_content =
            fs::read_to_string(PathBuf::from(&override_entry.origin).join("SKILL.md"))
                .map_err(ServiceError::Io)?;
        let metadata = crate::core::metadata::parse_yaml_frontmatter(&skill_content).ok();
        let name = metadata
            .as_ref()
            .map(|value| value.name.clone())
            .unwrap_or_else(|| override_entry.id.clone());
        let version = metadata
            .and_then(|value| value.version)
            .unwrap_or_else(|| "0.0.0".to_string());
        let override_manifest = PathBuf::from(&override_entry.origin).join("skill-project.toml");
        let mut required_dependencies = if override_manifest.is_file() {
            SkillProjectToml::load_from_file(&override_manifest)
                .map_err(|error| {
                    ServiceError::Config(format!(
                        "Failed to load personal override dependencies for '{}': {error}",
                        override_entry.id
                    ))
                })?
                .dependencies
                .map(|section| section.dependencies.into_keys().collect::<Vec<_>>())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        required_dependencies.sort();
        if let Some(entry) = lock
            .skills
            .iter_mut()
            .find(|entry| entry.id == override_entry.id)
        {
            entry.name = name;
            entry.origin = origin.clone();
            entry.resolved.version = version;
            entry.resolved.commit_hash = None;
            entry.resolved.checksum = Some(override_entry.digest.clone());
            entry.dependencies = required_dependencies;
            entry.depth = 0;
            entry.parent_skill = None;
            entry.required_by.clear();
        } else {
            lock.skills.push(ProjectLockedSkillEntry {
                id: override_entry.id.clone(),
                name,
                origin,
                resolved: Resolved {
                    version,
                    commit_hash: None,
                    checksum: Some(override_entry.digest.clone()),
                },
                dependencies: required_dependencies,
                groups: Vec::new(),
                depth: 0,
                parent_skill: None,
                required_by: Vec::new(),
            });
        }
        if !lock.covered_roots.contains(&override_entry.id) {
            lock.covered_roots.push(override_entry.id.clone());
        }
        declarations.remove(&override_entry.id);
    }
    lock.overrides
        .retain(|entry| !final_overrides.iter().any(|item| item.id == entry.id));
    Ok(())
}

pub(crate) fn reset_personal_override(
    service: &BundleService,
    id: &str,
) -> Result<bool, ServiceError> {
    let Some(preparation) = prepare_reset(service, id)? else {
        return Ok(false);
    };
    let manifest_path = service.project_root.join("skill-project.toml");
    let lock_path = service.project_root.join("skills.lock");
    let mut manifest = preparation.manifest;
    let mut lock = preparation.lock;
    let expected_override = preparation.expected_override;
    let artifact = preparation.artifact;
    let packaged_digest = preparation.packaged_digest;
    let installed = service.skills_directory.join(id);

    let state_guard = StateMutationGuard::acquire_for(
        &service.project_root,
        Some(&service.skills_directory),
        "bundle override reset",
    )?;
    let current = match load_state(&manifest_path, &lock_path) {
        Ok(state) => state,
        Err(error) => {
            state_guard.recovered()?;
            return Err(error);
        }
    };
    if current.0.overrides != manifest.overrides
        || current.1.overrides != lock.overrides
        || current.1.bundles != lock.bundles
    {
        state_guard.recovered()?;
        return Err(ServiceError::InvalidOperation(
            "Bundle ownership changed while the reset was prepared; retry".to_string(),
        ));
    }
    manifest = current.0;
    lock = current.1;
    let prepared = match PreparedBundle::load(&artifact) {
        Ok(prepared) => prepared,
        Err(error) => {
            state_guard.recovered()?;
            return Err(error);
        }
    };
    let packaged = match prepared.members.get(id) {
        Some(packaged) if packaged.digest == packaged_digest => packaged,
        Some(_) => {
            state_guard.recovered()?;
            return Err(ServiceError::Config(format!(
                "Cached bundle artifact '{}' changed while the reset was prepared",
                artifact.display()
            )));
        }
        None => {
            state_guard.recovered()?;
            return Err(ServiceError::Config(format!(
                "Cached bundle artifact '{}' does not contain member '{id}'",
                artifact.display()
            )));
        }
    };
    if installed.exists() {
        let installed_digest = match digest_directory(&installed) {
            Ok(digest) => digest,
            Err(error) => {
                state_guard.recovered()?;
                return Err(error);
            }
        };
        if installed_digest != expected_override.digest {
            state_guard.recovered()?;
            return Err(ServiceError::InvalidOperation(format!(
                "Skill '{id}' changed while the reset was prepared; retry"
            )));
        }
    }
    let ids = vec![id.to_string()];
    let mut transaction = match BundleTransaction::capture(
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
        replace_skill_directory(&installed, &packaged.source)?;
        manifest.overrides.remove(id);
        lock.overrides.retain(|entry| entry.id != id);
        save_bundle_declarations(&manifest_path, &manifest.bundles, &manifest.overrides)?;
        lock.save_to_file(&lock_path)
            .map_err(|error| ServiceError::Config(format!("Failed to save skills.lock: {error}")))
    })();
    if let Err(error) = result {
        if let Err(recovery_error) = transaction.rollback() {
            return Err(ServiceError::Custom(format!(
                "Failed to reset override: {error}; recovery also failed: {recovery_error}"
            )));
        }
        state_guard.recovered()?;
        return Err(error);
    }
    transaction.commit();
    state_guard.commit()?;
    Ok(true)
}

pub(crate) fn preview_reset_personal_override(
    service: &BundleService,
    id: &str,
) -> Result<BundleOverridePreview, ServiceError> {
    let preparation = prepare_reset(service, id)?;
    Ok(match preparation {
        Some(preparation) => BundleOverridePreview {
            id: id.to_string(),
            current_revision: Some(preparation.expected_override.digest),
            target_revision: Some(preparation.packaged_digest),
            changed: true,
            changes: vec![
                "content".to_string(),
                "manifest".to_string(),
                "lock".to_string(),
            ],
            retained: Vec::new(),
        },
        None => BundleOverridePreview {
            id: id.to_string(),
            current_revision: None,
            target_revision: None,
            changed: false,
            changes: Vec::new(),
            retained: Vec::new(),
        },
    })
}

struct ResetPreparation {
    manifest: BundleManifestTables,
    lock: ProjectSkillsLock,
    expected_override: crate::core::lock::ProjectLockedPersonalOverride,
    artifact: PathBuf,
    packaged_digest: String,
}

fn prepare_reset(
    service: &BundleService,
    id: &str,
) -> Result<Option<ResetPreparation>, ServiceError> {
    SkillId::new(id.to_string())?;
    let manifest_path = service.project_root.join("skill-project.toml");
    let lock_path = service.project_root.join("skills.lock");
    let (manifest, lock) = load_state(&manifest_path, &lock_path)?;
    let declared = manifest.overrides.contains_key(id);
    let locked_override = lock.overrides.iter().find(|entry| entry.id == id).cloned();
    let expected_override = match (declared, locked_override) {
        (false, None) => return Ok(None),
        (true, Some(entry)) => entry,
        _ => {
            return Err(ServiceError::Config(format!(
                "Personal override '{id}' is inconsistent between the Manifest and Lock"
            )));
        }
    };
    let (artifact, packaged_digest) = agreed_owner_selection(service, &lock, id)?;
    let installed = service.skills_directory.join(id);
    if installed.exists() && digest_directory(&installed)? != expected_override.digest {
        return Err(ServiceError::InvalidOperation(format!(
            "Skill '{id}' is locally modified; FastSkill will not discard those edits while resetting the override"
        )));
    }
    let prepared = PreparedBundle::load(&artifact)?;
    let packaged = prepared.members.get(id).ok_or_else(|| {
        ServiceError::Config(format!(
            "Cached bundle artifact '{}' does not contain member '{id}'",
            artifact.display()
        ))
    })?;
    if packaged.digest != packaged_digest {
        return Err(ServiceError::Config(format!(
            "Cached bundle artifact '{}' does not match the locked contents for '{id}'",
            artifact.display()
        )));
    }
    Ok(Some(ResetPreparation {
        manifest,
        lock,
        expected_override,
        artifact,
        packaged_digest,
    }))
}

fn load_state(
    manifest_path: &std::path::Path,
    lock_path: &std::path::Path,
) -> Result<(BundleManifestTables, ProjectSkillsLock), ServiceError> {
    let raw = fs::read_to_string(manifest_path).map_err(ServiceError::Io)?;
    let manifest = toml::from_str(&raw).map_err(|error| {
        ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
    })?;
    let lock = if lock_path.exists() {
        ProjectSkillsLock::load_from_file(lock_path)
            .map_err(|error| ServiceError::Config(format!("Failed to load skills.lock: {error}")))?
    } else {
        ProjectSkillsLock::new_empty()
    };
    Ok((manifest, lock))
}

fn agreed_owner_selection(
    service: &BundleService,
    lock: &ProjectSkillsLock,
    id: &str,
) -> Result<(std::path::PathBuf, String), ServiceError> {
    let owners = lock
        .bundles
        .iter()
        .filter_map(|bundle| {
            bundle
                .members
                .iter()
                .find(|member| member.id == id)
                .map(|member| (bundle, member))
        })
        .collect::<Vec<_>>();
    let Some((first_bundle, first_member)) = owners.first() else {
        return Err(ServiceError::InvalidOperation(format!(
            "Personal override '{id}' has no retained bundle owner to restore"
        )));
    };
    if owners
        .iter()
        .any(|(_, member)| member.digest != first_member.digest)
    {
        let bundles = owners
            .iter()
            .map(|(bundle, _)| bundle.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ServiceError::InvalidOperation(format!(
            "Cannot reset '{id}': retained bundles select conflicting contents ({bundles})"
        )));
    }
    Ok((
        service.project_root.join(&first_bundle.artifact),
        first_member.digest.clone(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::lock::{
        ProjectLockedBundleEntry, ProjectLockedBundleMember, ProjectLockedPersonalOverride,
    };
    use tempfile::TempDir;

    fn locked_bundle(id: &str, member: &str) -> ProjectLockedBundleEntry {
        ProjectLockedBundleEntry {
            id: id.to_string(),
            version: "1.0.0".to_string(),
            artifact: format!("{id}.zip"),
            digest: format!("{id}-digest"),
            members: vec![ProjectLockedBundleMember {
                id: member.to_string(),
                digest: "personal-digest".to_string(),
                overridable: true,
            }],
        }
    }

    #[test]
    fn final_override_promotion_updates_an_existing_lock_root_and_dependency_edges() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("personal");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: Personal Name\nversion: 2.3.4\ndescription: personal\n---\nbody\n",
        )
        .unwrap();
        fs::write(
            source.join("skill-project.toml"),
            "[dependencies]\nzeta = \"1.0.0\"\nalpha = \"1.0.0\"\n",
        )
        .unwrap();
        let mut project = SkillProjectToml {
            schema_version: None,
            metadata: None,
            dependencies: None,
            tool: None,
        };
        let mut declarations = BTreeMap::from([(
            "personal".to_string(),
            BundleOverrideDeclaration {
                origin: source.display().to_string(),
            },
        )]);
        let mut lock = ProjectSkillsLock::new_empty();
        lock.bundles = vec![locked_bundle("team", "personal")];
        lock.overrides = vec![ProjectLockedPersonalOverride {
            id: "personal".to_string(),
            origin: source.display().to_string(),
            digest: "personal-digest".to_string(),
        }];
        lock.skills = vec![ProjectLockedSkillEntry {
            id: "personal".to_string(),
            name: "old".to_string(),
            origin: Origin::Local {
                path: "old".into(),
                editable: false,
            },
            resolved: Resolved {
                version: "0.1.0".to_string(),
                commit_hash: Some("old".to_string()),
                checksum: None,
            },
            dependencies: Vec::new(),
            groups: vec!["old".to_string()],
            depth: 1,
            parent_skill: Some("team".to_string()),
            required_by: vec!["team".to_string()],
        }];

        retain_final_personal_overrides(
            root.path(),
            "team",
            &mut project,
            &mut declarations,
            &mut lock,
        )
        .unwrap();

        let entry = &lock.skills[0];
        assert_eq!(entry.name, "Personal Name");
        assert_eq!(entry.resolved.version, "2.3.4");
        assert_eq!(entry.dependencies, vec!["alpha", "zeta"]);
        assert_eq!(entry.depth, 0);
        assert!(entry.parent_skill.is_none());
        assert!(entry.required_by.is_empty());
        assert_eq!(lock.covered_roots, vec!["personal"]);
        assert!(lock.overrides.is_empty());
        assert!(declarations.is_empty());
        assert!(project
            .dependencies
            .unwrap()
            .dependencies
            .contains_key("personal"));
    }

    #[test]
    fn final_override_promotion_reports_an_invalid_dependency_manifest() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("personal");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "body without frontmatter").unwrap();
        fs::write(source.join("skill-project.toml"), "invalid = [").unwrap();
        let mut project = SkillProjectToml {
            schema_version: None,
            metadata: None,
            dependencies: None,
            tool: None,
        };
        let mut declarations = BTreeMap::new();
        let mut lock = ProjectSkillsLock::new_empty();
        lock.bundles = vec![locked_bundle("team", "personal")];
        lock.overrides = vec![ProjectLockedPersonalOverride {
            id: "personal".to_string(),
            origin: source.display().to_string(),
            digest: "personal-digest".to_string(),
        }];

        let error = retain_final_personal_overrides(
            root.path(),
            "team",
            &mut project,
            &mut declarations,
            &mut lock,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("Failed to load personal override dependencies"));
    }

    #[test]
    fn final_override_promotion_creates_a_root_with_safe_metadata_defaults() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("personal");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "body without frontmatter").unwrap();
        let mut project = SkillProjectToml {
            schema_version: None,
            metadata: None,
            dependencies: None,
            tool: None,
        };
        let mut declarations = BTreeMap::from([(
            "personal".to_string(),
            BundleOverrideDeclaration {
                origin: source.display().to_string(),
            },
        )]);
        let mut lock = ProjectSkillsLock::new_empty();
        lock.covered_roots.push("personal".to_string());
        lock.bundles = vec![locked_bundle("team", "personal")];
        lock.overrides = vec![ProjectLockedPersonalOverride {
            id: "personal".to_string(),
            origin: source.display().to_string(),
            digest: "personal-digest".to_string(),
        }];

        retain_final_personal_overrides(
            root.path(),
            "team",
            &mut project,
            &mut declarations,
            &mut lock,
        )
        .unwrap();

        let entry = lock
            .skills
            .iter()
            .find(|entry| entry.id == "personal")
            .unwrap();
        assert_eq!(entry.name, "personal");
        assert_eq!(entry.resolved.version, "0.0.0");
        assert!(entry.dependencies.is_empty());
        assert_eq!(lock.covered_roots, vec!["personal"]);
    }

    #[test]
    fn state_loader_handles_absent_and_invalid_lock_and_manifest() {
        let root = TempDir::new().unwrap();
        let manifest = root.path().join("skill-project.toml");
        let lock = root.path().join("skills.lock");
        fs::write(&manifest, "").unwrap();
        assert!(load_state(&manifest, &lock).unwrap().1.skills.is_empty());

        fs::write(&lock, "invalid = [").unwrap();
        assert!(load_state(&manifest, &lock).is_err());
        fs::write(&manifest, "invalid = [").unwrap();
        assert!(load_state(&manifest, &lock).is_err());
    }

    #[test]
    fn promotion_without_a_final_override_is_a_noop() {
        let mut project = SkillProjectToml {
            schema_version: None,
            metadata: None,
            dependencies: None,
            tool: None,
        };
        let mut declarations = BTreeMap::new();
        let mut lock = ProjectSkillsLock::new_empty();
        retain_final_personal_overrides(
            Path::new("."),
            "team",
            &mut project,
            &mut declarations,
            &mut lock,
        )
        .unwrap();
        assert!(project.dependencies.is_none());
        assert!(lock.skills.is_empty());
    }
}

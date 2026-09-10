//! Read-only validation of all declared bundle selections.

use crate::core::bundle::{
    BundleInstallPreview, BundleManifestTables, BundleService, BundleUpdatePreview,
    DeclaredBundleMember, PreparedBundle, ProjectBundleManifest,
};
use crate::core::lock::{ProjectLockedBundleEntry, ProjectLockedBundleMember, ProjectSkillsLock};
use crate::core::manifest::SkillProjectToml;
use crate::core::service::ServiceError;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub(crate) fn preview_install(
    service: &BundleService,
    artifact: &Path,
) -> Result<BundleInstallPreview, ServiceError> {
    let prepared = PreparedBundle::load(artifact)?;
    let (manifest, lock) = service.load_project_state()?;
    let existing = lock
        .bundles
        .iter()
        .find(|bundle| bundle.id == prepared.descriptor.id);
    if let Some(installed) = existing {
        if installed.version == prepared.descriptor.version
            && installed.digest != prepared.release_digest
        {
            return Err(ServiceError::Validation(format!(
                "Bundle release '{}@{}' is immutable: its contents differ from the installed digest",
                installed.id, installed.version
            )));
        }
        if installed.version != prepared.descriptor.version {
            return Err(ServiceError::InvalidOperation(format!(
                "Bundle '{}' is already installed at {}; use 'fastskill bundle update {} --from <artifact>'",
                installed.id, installed.version, installed.id
            )));
        }
        if installed.digest == prepared.release_digest {
            return Ok(BundleInstallPreview {
                id: installed.id.clone(),
                current_revision: Some(installed.version.clone()),
                target_revision: prepared.descriptor.version,
                changes: Vec::new(),
                retained: installed
                    .members
                    .iter()
                    .map(|member| member.id.clone())
                    .collect(),
            });
        }
    }

    let release_key = format!("{}@{}", prepared.descriptor.id, prepared.descriptor.version);
    if service
        .load_history()?
        .releases
        .get(&release_key)
        .is_some_and(|known| known != &prepared.release_digest)
    {
        return Err(ServiceError::Validation(format!(
            "Bundle release '{release_key}' is immutable: its contents differ from the previously known digest"
        )));
    }
    service.preflight_apply(&prepared, existing, &manifest, &lock)?;
    let changes = service.plan_changes(&prepared, existing, &manifest, &lock)?;
    let changed: BTreeSet<_> = changes
        .replacements
        .iter()
        .map(|replacement| replacement.id.as_str())
        .collect();
    let mut rendered = changes
        .replacements
        .iter()
        .map(|replacement| format!("add {}", replacement.id))
        .collect::<Vec<_>>();
    rendered.extend(["manifest".to_string(), "lock".to_string()]);
    rendered.sort();
    let retained = prepared
        .members
        .keys()
        .filter(|id| !changed.contains(id.as_str()))
        .cloned()
        .collect();
    Ok(BundleInstallPreview {
        id: prepared.descriptor.id,
        current_revision: existing.map(|bundle| bundle.version.clone()),
        target_revision: prepared.descriptor.version,
        changes: rendered,
        retained,
    })
}

pub(crate) fn preview_update(
    service: &BundleService,
    id: &str,
    artifact: &Path,
) -> Result<BundleUpdatePreview, ServiceError> {
    let prepared = PreparedBundle::load(artifact)?;
    if prepared.descriptor.id != id {
        return Err(ServiceError::InvalidOperation(format!(
            "Bundle artifact identifies '{}', not requested bundle '{}'",
            prepared.descriptor.id, id
        )));
    }
    let (manifest, lock) = service.load_project_state()?;
    let current = lock
        .bundles
        .iter()
        .find(|bundle| bundle.id == id)
        .ok_or_else(|| ServiceError::SkillNotFound(format!("Bundle '{id}' is not installed")))?;
    service.preflight_apply(&prepared, Some(current), &manifest, &lock)?;
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
        let locked = current
            .members
            .iter()
            .find(|candidate| candidate.id == *member);
        let proposed = prepared.members.get(*member);
        if locked.map(|candidate| candidate.digest.as_str())
            != proposed.map(|candidate| candidate.digest.as_str())
        {
            changes.push(format!("replace {member}"));
        } else if locked.map(|candidate| candidate.overridable)
            != proposed.map(|candidate| candidate.overridable)
        {
            changes.push(format!("change policy {member}"));
        }
    }
    changes.sort();
    let changed_ids: BTreeSet<_> = changes
        .iter()
        .filter_map(|change| change.split_whitespace().last())
        .collect();
    let retained = current
        .members
        .iter()
        .map(|member| member.id.clone())
        .filter(|member| !changed_ids.contains(member.as_str()))
        .collect();
    Ok(BundleUpdatePreview {
        id: id.to_string(),
        current_revision: current.version.clone(),
        target_revision: prepared.descriptor.version,
        changes,
        retained,
    })
}

pub(crate) fn validate_declared(
    service: &BundleService,
    locked: bool,
) -> Result<Vec<DeclaredBundleMember>, ServiceError> {
    let manifest_path = service.project_root.join("skill-project.toml");
    let raw = fs::read_to_string(&manifest_path).map_err(ServiceError::Io)?;
    let declarations: BundleManifestTables = toml::from_str(&raw).map_err(|error| {
        ServiceError::Config(format!("Failed to load bundle declarations: {error}"))
    })?;
    let project = SkillProjectToml::load_from_file(&manifest_path).map_err(|error| {
        ServiceError::Config(format!("Failed to load skill-project.toml: {error}"))
    })?;
    let lock_path = service.project_root.join("skills.lock");
    let mut simulated = if lock_path.exists() {
        ProjectSkillsLock::load_from_file(&lock_path)
            .map_err(|error| ServiceError::Config(format!("Failed to load skills.lock: {error}")))?
    } else {
        ProjectSkillsLock::new_empty()
    };
    let manifest = ProjectBundleManifest {
        project,
        bundles: declarations.bundles.clone(),
        overrides: declarations.overrides,
    };

    if locked {
        let selections = simulated.bundles.clone();
        let mut members = Vec::new();
        for expected in selections {
            let artifact = service.project_root.join(&expected.artifact);
            let prepared = PreparedBundle::load(&artifact)?;
            prepared.verify_locked_release(&expected)?;
            service.preflight_apply(&prepared, Some(&expected), &manifest, &simulated)?;
            members.extend(expected.members.iter().map(|member| DeclaredBundleMember {
                bundle_id: expected.id.clone(),
                id: member.id.clone(),
                digest: member.digest.clone(),
                overridable: member.overridable,
            }));
        }
        return Ok(members);
    }

    let mut selected_members = Vec::new();
    for (id, dependency) in &manifest.bundles {
        let artifact = service.project_root.join(&dependency.artifact);
        let prepared = PreparedBundle::load(&artifact)?;
        if prepared.descriptor.id != *id {
            return Err(ServiceError::InvalidOperation(format!(
                "Declared bundle '{id}' does not match artifact identity '{}'",
                prepared.descriptor.id
            )));
        }
        let existing = simulated
            .bundles
            .iter()
            .find(|bundle| bundle.id == *id)
            .cloned();
        service.preflight_apply(&prepared, existing.as_ref(), &manifest, &simulated)?;
        selected_members.extend(
            prepared
                .members
                .values()
                .map(|member| DeclaredBundleMember {
                    bundle_id: id.clone(),
                    id: member.id.clone(),
                    digest: member.digest.clone(),
                    overridable: member.overridable,
                }),
        );
        simulated.bundles.retain(|bundle| bundle.id != *id);
        simulated.bundles.push(ProjectLockedBundleEntry {
            id: id.clone(),
            version: prepared.descriptor.version.clone(),
            artifact: dependency.artifact.clone(),
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
        });
    }
    Ok(selected_members)
}

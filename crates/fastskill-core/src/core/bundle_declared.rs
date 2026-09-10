//! Declared bundle restore and ownership query operations.

use crate::core::bundle::{ApplyMode, BundleApplyResult, BundleService, InstalledBundle};
use crate::core::bundle_persistence::{
    restore_personal_overrides, restore_personal_overrides_with_guard,
};
use crate::core::service::ServiceError;
use crate::core::state_guard::StateMutationGuard;

pub(crate) fn list(service: &BundleService) -> Result<Vec<InstalledBundle>, ServiceError> {
    let (_, lock) = service.load_project_state()?;
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

pub(crate) fn install_declared(
    service: &BundleService,
    locked: bool,
    guard: Option<&StateMutationGuard>,
) -> Result<Vec<BundleApplyResult>, ServiceError> {
    let mut results = Vec::new();
    if locked {
        let (_, lock) = service.load_project_state()?;
        for expected in lock.bundles {
            let artifact = service.project_root.join(&expected.artifact);
            results.push(service.apply(
                &artifact,
                ApplyMode::RestoreLocked {
                    expected: &expected,
                },
                guard,
            )?);
        }
    } else {
        let (manifest, _) = service.load_project_state()?;
        for (id, dependency) in manifest.bundles {
            let artifact = service.project_root.join(&dependency.artifact);
            results.push(service.apply(&artifact, ApplyMode::Restore { id: &id }, guard)?);
        }
    }
    if let Some(guard) = guard {
        restore_personal_overrides_with_guard(service, guard)?;
    } else {
        restore_personal_overrides(service)?;
    }
    Ok(results)
}

pub(crate) fn ensure_individual_removal_allowed(
    service: &BundleService,
    id: &str,
) -> Result<(), ServiceError> {
    let (_, lock) = service.load_project_state()?;
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
        "Skill '{id}' is managed by installed bundle(s): {}. Remove the owning bundle with 'fastskill bundle remove <bundle-id>'",
        owners.join(", ")
    )))
}

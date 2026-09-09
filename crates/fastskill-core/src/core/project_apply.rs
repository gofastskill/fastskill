//! Transactional application of a verified project dependency plan.

use crate::core::install::PreparedSkill;
use crate::core::lifecycle_transaction::LifecycleTransaction;
use crate::core::lock::{ProjectLockedSkillEntry, ProjectSkillsLock};
use crate::core::manifest::{DependenciesSection, DependencySpec, SkillEntry, SkillProjectToml};
use crate::core::origin::Origin;
use crate::core::project_state::save_project_preserving;
use crate::core::service::{FastSkillService, ServiceError, SkillId};
use crate::core::skill_manager::SkillDefinition;
use crate::core::state_guard::{StateMutationGuard, StateMutationLease};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::Path;

pub struct ProjectApplyCandidate {
    pub prepared: PreparedSkill,
    pub origin: Origin,
    pub groups: Vec<String>,
    pub depth: u32,
    pub required_by: BTreeSet<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExpectedManifestSkill {
    pub id: String,
    /// `None` records that the direct requirement did not exist at planning
    /// time. This lets unrelated concurrent additions merge safely while a
    /// same-id change forces the caller to replan.
    pub entry: Option<SkillEntry>,
}

pub struct ProjectApplyPlan {
    pub candidates: Vec<ProjectApplyCandidate>,
    pub base_lock: Option<ProjectSkillsLock>,
    pub covered_roots: Vec<String>,
    pub manifest_updates: Vec<SkillEntry>,
    pub removals: Vec<String>,
    pub write_lock: bool,
    /// Individual Lock facts observed during planning. Extra unrelated entries
    /// may appear before the lease is acquired, but these facts and candidate
    /// identities must not change underneath the prepared operation.
    pub expected_skills: Vec<ProjectLockedSkillEntry>,
    pub expected_manifest: Vec<ExpectedManifestSkill>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectApplyResult {
    pub installed: Vec<String>,
    pub removed: Vec<String>,
}

/// Remove the previous closures of changed roots while retaining entries that
/// remain reachable from another selected root. Incoming ownership edges are
/// recomputed from the retained graph so replaced parents do not linger.
pub fn retain_unaffected_roots(
    mut lock: ProjectSkillsLock,
    changed_roots: &[String],
) -> (ProjectSkillsLock, Vec<String>) {
    let replaced = lock_closure(&lock, changed_roots.iter().cloned());
    let protected = lock_closure(
        &lock,
        lock.covered_roots
            .iter()
            .filter(|root| !changed_roots.contains(root))
            .cloned(),
    );
    let removed: Vec<_> = lock
        .skills
        .iter()
        .filter(|entry| replaced.contains(&entry.id) && !protected.contains(&entry.id))
        .map(|entry| entry.id.clone())
        .collect();
    lock.skills
        .retain(|entry| !replaced.contains(&entry.id) || protected.contains(&entry.id));
    lock.covered_roots
        .retain(|root| !changed_roots.contains(root));
    recompute_owners(&mut lock);
    (lock, removed)
}

fn lock_closure(
    lock: &ProjectSkillsLock,
    roots: impl IntoIterator<Item = String>,
) -> BTreeSet<String> {
    let mut included: BTreeSet<_> = roots.into_iter().collect();
    loop {
        let before = included.len();
        for entry in &lock.skills {
            if included.contains(&entry.id) {
                included.extend(entry.dependencies.iter().cloned());
            }
        }
        if included.len() == before {
            return included;
        }
    }
}

fn recompute_owners(lock: &mut ProjectSkillsLock) {
    let mut owners: HashMap<String, Vec<String>> = HashMap::new();
    for entry in &lock.skills {
        for dependency in &entry.dependencies {
            owners
                .entry(dependency.clone())
                .or_default()
                .push(entry.id.clone());
        }
    }
    for entry in &mut lock.skills {
        entry.required_by = owners.remove(&entry.id).unwrap_or_default();
        entry.required_by.sort();
        entry.required_by.dedup();
        entry.parent_skill = entry.required_by.first().cloned();
    }
    let dependencies: HashMap<_, _> = lock
        .skills
        .iter()
        .map(|entry| (entry.id.clone(), entry.dependencies.clone()))
        .collect();
    let mut depths = HashMap::new();
    let mut queue: VecDeque<_> = lock
        .covered_roots
        .iter()
        .cloned()
        .map(|id| (id, 0_u32))
        .collect();
    while let Some((id, depth)) = queue.pop_front() {
        if depths.get(&id).is_some_and(|known| *known <= depth) {
            continue;
        }
        depths.insert(id.clone(), depth);
        if let Some(children) = dependencies.get(&id) {
            queue.extend(children.iter().cloned().map(|child| (child, depth + 1)));
        }
    }
    for entry in &mut lock.skills {
        if let Some(depth) = depths.get(&entry.id) {
            entry.depth = *depth;
        }
    }
}

impl FastSkillService {
    /// Validate ownership and installed-content preconditions without mutation.
    /// Preview callers use this same check before reporting an operation as
    /// applicable; apply repeats it after acquiring the writer lease.
    pub fn validate_project_plan(
        &self,
        project_root: &Path,
        plan: &ProjectApplyPlan,
    ) -> Result<(), ServiceError> {
        let lock_path = project_root.join("skills.lock");
        let current = if lock_path.exists() {
            Some(
                ProjectSkillsLock::load_from_file(&lock_path).map_err(|error| {
                    ServiceError::Config(format!("Failed to load skills.lock: {error}"))
                })?,
            )
        } else {
            None
        };
        validate_expected_manifest(project_root, &plan.expected_manifest)?;
        validate_expected_lock_state(current.as_ref(), plan)?;
        if let Some(lock) = &current {
            validate_bundle_ownership(&plan.candidates, lock)?;
            validate_installed_content(
                &plan
                    .candidates
                    .iter()
                    .map(|candidate| candidate.prepared.id().to_string())
                    .chain(plan.removals.iter().cloned())
                    .collect::<Vec<_>>(),
                lock,
                &self.config().skill_storage_path,
            )?;
        }
        validate_retained_candidates(&plan.candidates, plan.base_lock.as_ref(), project_root)?;
        validate_unmanaged_destinations(
            &plan.candidates,
            current.as_ref(),
            &self.config().skill_storage_path,
        )
    }

    /// Apply a fully prepared dependency closure under one project/destination
    /// lease. Callers resolve and validate every candidate before entering this
    /// method; this operation owns filesystem, registry, Manifest, and Lock
    /// recovery if any later step fails.
    pub async fn apply_project_plan(
        &self,
        project_root: &Path,
        plan: ProjectApplyPlan,
    ) -> Result<ProjectApplyResult, ServiceError> {
        self.apply_project_plan_inner(project_root, plan, None)
            .await
    }

    /// Apply inside a wider transaction, such as combined bundle and
    /// individual reconciliation. The caller owns the outer lease lifecycle.
    pub async fn apply_project_plan_with_guard(
        &self,
        project_root: &Path,
        plan: ProjectApplyPlan,
        guard: &StateMutationGuard,
    ) -> Result<ProjectApplyResult, ServiceError> {
        self.apply_project_plan_inner(project_root, plan, Some(guard))
            .await
    }

    async fn apply_project_plan_inner(
        &self,
        project_root: &Path,
        mut plan: ProjectApplyPlan,
        guard: Option<&StateMutationGuard>,
    ) -> Result<ProjectApplyResult, ServiceError> {
        let storage = &self.config().skill_storage_path;
        let lease = StateMutationLease::acquire_or_borrow(
            project_root,
            Some(storage),
            "apply project skill plan",
            guard,
        )?;
        let lock_path = project_root.join("skills.lock");
        let current_lock = if lock_path.exists() {
            match ProjectSkillsLock::load_from_file(&lock_path) {
                Ok(lock) => Some(lock),
                Err(error) => {
                    lease.recovered()?;
                    return Err(ServiceError::Config(format!(
                        "Failed to load skills.lock: {error}"
                    )));
                }
            }
        } else {
            None
        };
        if let Err(error) = validate_expected_manifest(project_root, &plan.expected_manifest) {
            lease.recovered()?;
            return Err(error);
        }
        let mut affected: Vec<_> = plan
            .candidates
            .iter()
            .map(|candidate| candidate.prepared.id().to_string())
            .chain(plan.removals.iter().cloned())
            .collect();
        affected.sort();
        affected.dedup();
        if let Err(error) = validate_expected_lock_state(current_lock.as_ref(), &plan) {
            lease.recovered()?;
            return Err(error);
        }
        if let Some(lock) = &current_lock {
            if let Err(error) = validate_bundle_ownership(&plan.candidates, lock)
                .and_then(|()| validate_installed_content(&affected, lock, storage))
                .and_then(|()| {
                    validate_retained_candidates(
                        &plan.candidates,
                        plan.base_lock.as_ref(),
                        project_root,
                    )
                })
            {
                lease.recovered()?;
                return Err(error);
            }
        }
        if let Err(error) =
            validate_unmanaged_destinations(&plan.candidates, current_lock.as_ref(), storage)
        {
            lease.recovered()?;
            return Err(error);
        }
        let transaction = match LifecycleTransaction::capture(project_root, storage, &affected) {
            Ok(transaction) => transaction,
            Err(error) => {
                lease.recovered()?;
                return Err(error);
            }
        };
        let previous_registry = match capture_registry(self, &affected).await {
            Ok(previous) => previous,
            Err(error) => {
                transaction.commit();
                lease.recovered()?;
                return Err(error);
            }
        };
        plan.candidates.sort_by(|left, right| {
            right
                .depth
                .cmp(&left.depth)
                .then_with(|| left.prepared.id().cmp(right.prepared.id()))
        });
        let result = self
            .apply_project_mutations(project_root, plan, current_lock)
            .await;
        match result {
            Ok(result) => {
                transaction.commit();
                lease.commit()?;
                Ok(result)
            }
            Err(original) => {
                if let Err(error) = transaction.rollback() {
                    drop(lease);
                    return Err(ServiceError::InvalidOperation(format!(
                        "{original}; project recovery failed: {error}"
                    )));
                }
                if let Err(error) = restore_registry(self, &previous_registry).await {
                    drop(lease);
                    return Err(ServiceError::InvalidOperation(format!(
                        "{original}; registry recovery failed: {error}"
                    )));
                }
                lease.recovered()?;
                Err(original)
            }
        }
    }

    async fn apply_project_mutations(
        &self,
        project_root: &Path,
        plan: ProjectApplyPlan,
        current_lock: Option<ProjectSkillsLock>,
    ) -> Result<ProjectApplyResult, ServiceError> {
        let mut entries = Vec::new();
        let mut installed = Vec::new();
        for candidate in plan.candidates {
            let id = candidate.prepared.id().to_string();
            let resolved = candidate.prepared.resolved().clone();
            self.apply_prepared_install(candidate.prepared, candidate.groups.clone())
                .await?;
            let (origin, _) = candidate.origin.to_manifest_relative(project_root);
            let required_by: Vec<_> = candidate.required_by.into_iter().collect();
            entries.push(ProjectLockedSkillEntry {
                id: id.clone(),
                name: id.clone(),
                origin,
                resolved,
                dependencies: candidate.dependencies,
                groups: candidate.groups,
                depth: candidate.depth,
                parent_skill: required_by.first().cloned(),
                required_by,
            });
            installed.push(id);
        }
        for id in &plan.removals {
            if !should_remove_managed_path(current_lock.as_ref(), id) {
                continue;
            }
            let id = SkillId::new(id.clone())?;
            if self.skill_manager().get_skill(&id).await?.is_some() {
                self.skill_manager().unregister_skill(&id).await?;
            }
            remove_managed_path(&self.config().skill_storage_path.join(id.as_str()))?;
        }
        if !plan.manifest_updates.is_empty() {
            apply_manifest_updates(project_root, &plan.manifest_updates)?;
        }
        if plan.write_lock {
            let retained = plan.base_lock.unwrap_or_else(ProjectSkillsLock::new_empty);
            let mut lock = current_lock.unwrap_or_else(ProjectSkillsLock::new_empty);
            for entry in &retained.skills {
                if let Some(current) = lock.skills.iter_mut().find(|item| item.id == entry.id) {
                    *current = entry.clone();
                }
            }
            let replaced: BTreeSet<_> = entries.iter().map(|entry| entry.id.as_str()).collect();
            lock.skills.retain(|entry| {
                !replaced.contains(entry.id.as_str()) && !plan.removals.contains(&entry.id)
            });
            for entry in &mut entries {
                if let Some(retained) = retained.skills.iter().find(|item| item.id == entry.id) {
                    entry.required_by.extend(retained.required_by.clone());
                    entry.required_by.sort();
                    entry.required_by.dedup();
                    entry.parent_skill = entry.required_by.first().cloned();
                    entry.depth = entry.depth.min(retained.depth);
                }
            }
            lock.skills.extend(entries);
            lock.covered_roots.extend(plan.covered_roots);
            lock.save_to_file(&project_root.join("skills.lock"))
                .map_err(|error| {
                    ServiceError::Config(format!("Failed to save skills.lock: {error}"))
                })?;
        }
        installed.sort();
        let mut removed = plan.removals;
        removed.sort();
        Ok(ProjectApplyResult { installed, removed })
    }
}

fn validate_retained_candidates(
    candidates: &[ProjectApplyCandidate],
    retained: Option<&ProjectSkillsLock>,
    project_root: &Path,
) -> Result<(), ServiceError> {
    let Some(retained) = retained else {
        return Ok(());
    };
    for candidate in candidates {
        let Some(existing) = retained
            .skills
            .iter()
            .find(|entry| entry.id == candidate.prepared.id())
        else {
            continue;
        };
        if !crate::core::resolution::origins_accept_same_resolution(
            &existing.origin.resolved_against(project_root),
            &candidate.origin,
            &candidate.prepared.resolved().version,
        ) || existing.resolved != *candidate.prepared.resolved()
        {
            return Err(ServiceError::InvalidOperation(format!(
                "skill '{}' is already required by retained roots with incompatible content",
                candidate.prepared.id()
            )));
        }
    }
    Ok(())
}

fn validate_expected_skills(
    current: &ProjectSkillsLock,
    plan: &ProjectApplyPlan,
) -> Result<(), ServiceError> {
    for expected in &plan.expected_skills {
        if current.skills.iter().find(|entry| entry.id == expected.id) != Some(expected) {
            return Err(ServiceError::InvalidOperation(format!(
                "locked state for '{}' changed after planning; retry the operation",
                expected.id
            )));
        }
    }
    for candidate in &plan.candidates {
        let id = candidate.prepared.id();
        if current.skills.iter().any(|entry| entry.id == id)
            && !plan.expected_skills.iter().any(|entry| entry.id == id)
        {
            return Err(ServiceError::InvalidOperation(format!(
                "locked state for '{id}' changed after planning; retry the operation"
            )));
        }
    }
    Ok(())
}

fn validate_expected_lock_state(
    current: Option<&ProjectSkillsLock>,
    plan: &ProjectApplyPlan,
) -> Result<(), ServiceError> {
    match current {
        Some(current) => validate_expected_skills(current, plan),
        None if !plan.expected_skills.is_empty() => Err(ServiceError::InvalidOperation(
            "skills.lock changed after planning; retry the operation".to_string(),
        )),
        None => Ok(()),
    }
}

fn has_non_ordinary_owner(lock: &ProjectSkillsLock, id: &str) -> bool {
    lock.bundles
        .iter()
        .any(|bundle| bundle.members.iter().any(|member| member.id == id))
        || lock.overrides.iter().any(|entry| entry.id == id)
}

fn should_remove_managed_path(lock: Option<&ProjectSkillsLock>, id: &str) -> bool {
    lock.is_none_or(|lock| !has_non_ordinary_owner(lock, id))
}

fn validate_expected_manifest(
    project_root: &Path,
    expected_manifest: &[ExpectedManifestSkill],
) -> Result<(), ServiceError> {
    if expected_manifest.is_empty() {
        return Ok(());
    }
    let path = project_root.join("skill-project.toml");
    let project = SkillProjectToml::load_from_file(&path)
        .map_err(|error| ServiceError::Validation(error.to_string()))?;
    let current = project
        .to_skill_entries(project_root)
        .map_err(ServiceError::Validation)?;
    for expected in expected_manifest {
        let actual = current.iter().find(|entry| entry.id == expected.id);
        let agrees = match (&expected.entry, actual) {
            (None, None) => true,
            (Some(expected), Some(actual)) => {
                expected.origin == actual.origin
                    && normalized_groups(&expected.groups) == normalized_groups(&actual.groups)
            }
            _ => false,
        };
        if !agrees {
            return Err(ServiceError::InvalidOperation(format!(
                "manifest requirement for '{}' changed after planning; retry the operation",
                expected.id
            )));
        }
    }
    Ok(())
}

fn normalized_groups(groups: &[String]) -> Vec<String> {
    let mut groups = groups.to_vec();
    groups.sort();
    groups.dedup();
    groups
}

fn validate_unmanaged_destinations(
    candidates: &[ProjectApplyCandidate],
    current: Option<&ProjectSkillsLock>,
    storage: &Path,
) -> Result<(), ServiceError> {
    for candidate in candidates {
        let id = candidate.prepared.id();
        let managed = current.is_some_and(|lock| {
            lock.skills.iter().any(|entry| entry.id == id) || has_non_ordinary_owner(lock, id)
        });
        let destination = storage.join(id);
        if !managed && (destination.exists() || destination.is_symlink()) {
            return Err(ServiceError::InvalidOperation(format!(
                "installed destination for '{id}' is unmanaged; move or remove it before adding the skill"
            )));
        }
    }
    Ok(())
}

fn remove_managed_path(path: &Path) -> Result<(), ServiceError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ServiceError::Io(error)),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(path).map_err(ServiceError::Io)
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(path).map_err(ServiceError::Io)
    } else {
        Err(ServiceError::InvalidOperation(format!(
            "unsupported managed destination: {}",
            path.display()
        )))
    }
}

fn apply_manifest_updates(project_root: &Path, updates: &[SkillEntry]) -> Result<(), ServiceError> {
    let path = project_root.join("skill-project.toml");
    let mut project = SkillProjectToml::load_from_file(&path)
        .map_err(|error| ServiceError::Validation(error.to_string()))?;
    let dependencies = project
        .dependencies
        .get_or_insert_with(|| DependenciesSection {
            dependencies: HashMap::new(),
        });
    for update in updates {
        let (origin, _) = update.origin.to_manifest_relative(project_root);
        dependencies.dependencies.insert(
            update.id.clone(),
            DependencySpec::Inline {
                origin,
                groups: (!update.groups.is_empty()).then_some(update.groups.clone()),
            },
        );
    }
    save_project_preserving(&path, &project)
        .map_err(|error| ServiceError::Validation(error.to_string()))
}

fn validate_bundle_ownership(
    candidates: &[ProjectApplyCandidate],
    lock: &ProjectSkillsLock,
) -> Result<(), ServiceError> {
    for candidate in candidates {
        let active_override = lock
            .overrides
            .iter()
            .find(|entry| entry.id == candidate.prepared.id());
        for bundle in &lock.bundles {
            for member in bundle
                .members
                .iter()
                .filter(|member| member.id == candidate.prepared.id())
            {
                let expected = if let Some(active_override) = active_override {
                    if !member.overridable {
                        return Err(ServiceError::InvalidOperation(format!(
                            "personal override '{}' is not permitted by bundle '{}'",
                            member.id, bundle.id
                        )));
                    }
                    active_override.digest.as_str()
                } else {
                    member.digest.as_str()
                };
                if candidate.prepared.resolved().checksum.as_deref() != Some(expected) {
                    return Err(ServiceError::InvalidOperation(format!(
                        "individual skill '{}' conflicts with installed bundle '{}'",
                        member.id, bundle.id
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_installed_content(
    affected: &[String],
    lock: &ProjectSkillsLock,
    storage: &Path,
) -> Result<(), ServiceError> {
    for id in affected {
        let installed = storage.join(id);
        if !installed.exists() && !installed.is_symlink() {
            continue;
        }
        let ordinary = lock.skills.iter().find(|entry| entry.id == *id);
        if ordinary
            .is_some_and(|entry| matches!(entry.origin, Origin::Local { editable: true, .. }))
            && !has_non_ordinary_owner(lock, id)
        {
            continue;
        }
        let active_override = lock.overrides.iter().find(|entry| entry.id == *id);
        if let Some(active_override) = active_override {
            if lock
                .bundles
                .iter()
                .flat_map(|bundle| &bundle.members)
                .any(|member| member.id == *id && !member.overridable)
            {
                return Err(ServiceError::InvalidOperation(format!(
                    "personal override '{id}' is no longer permitted by every bundle owner"
                )));
            }
            let actual = crate::core::install::content_digest(&installed)?;
            if actual != active_override.digest {
                return Err(ServiceError::InvalidOperation(format!(
                    "installed skill '{id}' was modified; restore or remove local edits before changing managed state"
                )));
            }
            continue;
        }
        let mut expected = Vec::new();
        if let Some(entry) = ordinary {
            if !matches!(entry.origin, Origin::Local { editable: true, .. }) {
                expected.push(entry.resolved.checksum.as_deref().ok_or_else(|| {
                    ServiceError::InvalidOperation(format!(
                        "installed skill '{id}' has no recorded integrity digest; regenerate the lock before changing managed state"
                    ))
                })?);
            }
        }
        expected.extend(
            lock.bundles
                .iter()
                .flat_map(|bundle| &bundle.members)
                .filter(|member| member.id == *id)
                .map(|member| member.digest.as_str()),
        );
        if expected.windows(2).any(|pair| pair[0] != pair[1]) {
            return Err(ServiceError::InvalidOperation(format!(
                "installed skill '{id}' has conflicting recorded ownership digests"
            )));
        }
        if let Some(expected) = expected.first() {
            let actual = crate::core::install::content_digest(&installed)?;
            if actual != **expected {
                return Err(ServiceError::InvalidOperation(format!(
                    "installed skill '{}' was modified; restore or remove local edits before changing managed state",
                    id
                )));
            }
        }
    }
    Ok(())
}

async fn capture_registry(
    service: &FastSkillService,
    ids: &[String],
) -> Result<Vec<(SkillId, Option<SkillDefinition>)>, ServiceError> {
    let mut previous = Vec::with_capacity(ids.len());
    for id in ids {
        let id = SkillId::new(id.clone())?;
        let definition = service.skill_manager().get_skill(&id).await?;
        previous.push((id, definition));
    }
    Ok(previous)
}

async fn restore_registry(
    service: &FastSkillService,
    previous: &[(SkillId, Option<SkillDefinition>)],
) -> Result<(), ServiceError> {
    for (id, definition) in previous {
        if service.skill_manager().get_skill(id).await?.is_some() {
            service.skill_manager().unregister_skill(id).await?;
        }
        if let Some(definition) = definition {
            service
                .skill_manager()
                .force_register_skill(definition.clone())
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

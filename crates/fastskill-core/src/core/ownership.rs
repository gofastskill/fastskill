//! Ownership and reachability planning for project skill removal.

use crate::core::lock::ProjectSkillsLock;
use crate::core::manifest::SkillProjectToml;
use crate::core::service::ServiceError;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A fully validated removal plan. State records and installed files are
/// separate because another owner can retain the bytes after direct intent is
/// removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRemovalPlan {
    pub remove_manifest_dependencies: Vec<String>,
    pub remove_lock_entries: Vec<String>,
    pub delete_files: Vec<String>,
    pub retained_files: Vec<String>,
    pub unchanged: Vec<String>,
    pub remaining_individual_roots: Vec<String>,
}

/// Read-only ownership graph assembled from the Manifest and Lock.
pub struct ProjectOwnership<'a> {
    manifest: &'a SkillProjectToml,
    lock: &'a ProjectSkillsLock,
    dependencies: BTreeMap<&'a str, Vec<&'a str>>,
}

impl<'a> ProjectOwnership<'a> {
    pub fn new(manifest: &'a SkillProjectToml, lock: &'a ProjectSkillsLock) -> Self {
        let dependencies = lock
            .skills
            .iter()
            .map(|entry| {
                (
                    entry.id.as_str(),
                    entry.dependencies.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        Self {
            manifest,
            lock,
            dependencies,
        }
    }

    pub fn plan_removal(&self, requested: &[String]) -> Result<ProjectRemovalPlan, ServiceError> {
        let direct_manifest = self.direct_manifest_roots();
        let roots = self.individual_roots(&direct_manifest);
        let locked_ids: BTreeSet<&str> = self
            .lock
            .skills
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        let bundle_members: BTreeSet<&str> = self
            .lock
            .bundles
            .iter()
            .flat_map(|bundle| bundle.members.iter().map(|member| member.id.as_str()))
            .collect();
        let overrides: BTreeSet<&str> = self
            .lock
            .overrides
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();

        let mut removable_roots = BTreeSet::new();
        let mut unchanged = BTreeSet::new();
        for id in requested {
            if overrides.contains(id.as_str()) {
                return Err(ServiceError::InvalidOperation(format!(
                    "Skill '{id}' has a personal bundle override; reset the override before removing it"
                )));
            }
            if roots.contains(id.as_str()) {
                removable_roots.insert(id.as_str());
                continue;
            }
            let required_by = self.roots_requiring(&roots, id);
            if !required_by.is_empty() {
                return Err(ServiceError::InvalidOperation(format!(
                    "Skill '{id}' is required by retained root(s): {}",
                    required_by.join(", ")
                )));
            }
            if bundle_members.contains(id.as_str()) {
                let mut owners = self
                    .lock
                    .bundles
                    .iter()
                    .filter(|bundle| bundle.members.iter().any(|member| member.id == *id))
                    .map(|bundle| bundle.id.as_str())
                    .collect::<Vec<_>>();
                owners.sort_unstable();
                return Err(ServiceError::InvalidOperation(format!(
                    "Skill '{id}' is managed by installed bundle(s): {}",
                    owners.join(", ")
                )));
            }
            if locked_ids.contains(id.as_str()) {
                removable_roots.insert(id.as_str());
            } else {
                unchanged.insert(id.as_str());
            }
        }

        let remaining_roots: BTreeSet<&str> = roots.difference(&removable_roots).copied().collect();
        let retained_individual = self.closure(&remaining_roots);
        let removed_closure = self.closure(&removable_roots);
        let remove_lock: BTreeSet<&str> = removed_closure
            .difference(&retained_individual)
            .copied()
            .filter(|id| locked_ids.contains(id))
            .collect();
        let retained_by_other_owner: BTreeSet<&str> = retained_individual
            .union(&bundle_members)
            .copied()
            .chain(overrides.iter().copied())
            .collect();
        let delete_files: BTreeSet<&str> = removed_closure
            .difference(&retained_by_other_owner)
            .copied()
            .filter(|id| locked_ids.contains(id))
            .collect();
        let retained_files: BTreeSet<&str> = removed_closure
            .intersection(&retained_by_other_owner)
            .copied()
            .collect();

        Ok(ProjectRemovalPlan {
            remove_manifest_dependencies: removable_roots
                .iter()
                .filter(|id| direct_manifest.contains(**id))
                .map(|id| (*id).to_string())
                .collect(),
            remove_lock_entries: remove_lock.into_iter().map(str::to_string).collect(),
            delete_files: delete_files.into_iter().map(str::to_string).collect(),
            retained_files: retained_files.into_iter().map(str::to_string).collect(),
            unchanged: unchanged.into_iter().map(str::to_string).collect(),
            remaining_individual_roots: remaining_roots.into_iter().map(str::to_string).collect(),
        })
    }

    /// Direct individual roots that require `id`, including transitive paths.
    pub fn roots_requiring_skill(&self, id: &str) -> Vec<String> {
        let direct = self.direct_manifest_roots();
        let roots = self.individual_roots(&direct);
        self.roots_requiring(&roots, id)
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    fn direct_manifest_roots(&self) -> BTreeSet<&'a str> {
        self.manifest
            .dependencies
            .as_ref()
            .map(|section| section.dependencies.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }

    fn individual_roots(&self, direct_manifest: &BTreeSet<&'a str>) -> BTreeSet<&'a str> {
        let locked_roots: BTreeSet<&str> = if self.lock.covered_roots.is_empty() {
            self.lock
                .skills
                .iter()
                .filter(|entry| entry.depth == 0)
                .map(|entry| entry.id.as_str())
                .collect()
        } else {
            self.lock.covered_roots.iter().map(String::as_str).collect()
        };
        direct_manifest.union(&locked_roots).copied().collect()
    }

    fn roots_requiring(&self, roots: &BTreeSet<&'a str>, id: &str) -> Vec<&'a str> {
        roots
            .iter()
            .copied()
            .filter(|root| self.closure(&BTreeSet::from([*root])).contains(id))
            .collect()
    }

    fn closure(&self, roots: &BTreeSet<&'a str>) -> BTreeSet<&'a str> {
        let mut reached = BTreeSet::new();
        let mut pending: VecDeque<&str> = roots.iter().copied().collect();
        while let Some(id) = pending.pop_front() {
            if !reached.insert(id) {
                continue;
            }
            if let Some(children) = self.dependencies.get(id) {
                pending.extend(children.iter().copied());
            }
        }
        reached
    }
}

/// Recompute the denormalized root/depth/parent facts after removing roots.
pub fn normalize_lock_ownership(lock: &mut ProjectSkillsLock, roots: &[String]) {
    let mut depths: BTreeMap<String, u32> = BTreeMap::new();
    let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let dependencies: BTreeMap<String, Vec<String>> = lock
        .skills
        .iter()
        .map(|entry| (entry.id.clone(), entry.dependencies.clone()))
        .collect();
    let mut pending: VecDeque<(String, u32)> =
        roots.iter().cloned().map(|root| (root, 0)).collect();
    while let Some((id, depth)) = pending.pop_front() {
        if depths.get(&id).is_some_and(|known| *known <= depth) {
            continue;
        }
        depths
            .entry(id.clone())
            .and_modify(|known| *known = (*known).min(depth))
            .or_insert(depth);
        if let Some(children) = dependencies.get(&id) {
            for child in children {
                parents.entry(child.clone()).or_default().insert(id.clone());
                pending.push_back((child.clone(), depth.saturating_add(1)));
            }
        }
    }
    lock.covered_roots = roots.to_vec();
    for entry in &mut lock.skills {
        if let Some(depth) = depths.get(&entry.id) {
            entry.depth = *depth;
        }
        entry.required_by = parents
            .remove(&entry.id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        entry.parent_skill = entry.required_by.first().cloned();
    }
}

use super::plan::{PlannedSkill, PreparedInstallPlan};
use crate::error::{CliError, CliResult};
use fastskill_core::core::lock::ProjectSkillsLock;
use fastskill_core::core::manifest::SkillEntry;
use fastskill_core::core::origin::{Origin, Resolved};
use fastskill_core::core::resolution::{
    prepare_resolution, prepare_resolution_preview, ResolutionRoot,
};
use fastskill_core::FastSkillService;
use std::collections::BTreeSet;
use std::path::Path;

/// A direct requirement whose complete dependency closure must be recomputed.
/// `locked` supplies immutable facts for an offline update; it is absent for a
/// new root and for online resolution.
pub(crate) struct ChangeRoot {
    pub origin: Origin,
    pub expected_id: Option<String>,
    pub groups: Vec<String>,
    pub locked: Option<Resolved>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangePreview {
    pub id: String,
    pub previous_version: Option<String>,
    pub resolved_version: String,
    pub origin_changed: bool,
    pub resolved_changed: bool,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
}

/// Resolve changed direct requirements and every dependency before any state is
/// mutated. Existing Lock entries outside the replaced closures are preserved.
pub(crate) async fn prepare_changes(
    service: &FastSkillService,
    lock_path: &Path,
    manifest_dir: &Path,
    roots: Vec<ChangeRoot>,
    max_levels: u32,
    offline: bool,
    preview: bool,
) -> CliResult<(PreparedInstallPlan, Vec<ChangePreview>)> {
    let existing = if lock_path.exists() {
        Some(
            ProjectSkillsLock::load_from_file(lock_path)
                .map_err(|error| CliError::Config(format!("Failed to load lock file: {error}")))?,
        )
    } else {
        None
    };
    let current_manifest = fastskill_core::core::manifest::SkillProjectToml::load_from_file(
        &manifest_dir.join("skill-project.toml"),
    )
    .map_err(|error| CliError::Config(format!("Failed to load manifest: {error}")))?
    .to_skill_entries(manifest_dir)
    .map_err(CliError::Config)?;
    let mut previews = Vec::new();
    let mut manifest_updates = Vec::new();
    let locked = existing
        .as_ref()
        .map(|lock| {
            lock.skills
                .iter()
                .map(|entry| (entry.id.clone(), entry.resolved.clone()))
                .collect()
        })
        .unwrap_or_default();
    let resolution_roots: Vec<_> = roots
        .iter()
        .map(|root| ResolutionRoot {
            origin: root.origin.clone(),
            expected_id: root.expected_id.clone(),
            groups: root.groups.clone(),
            locked: root.locked.clone(),
        })
        .collect();
    let resolution = if preview && !offline {
        prepare_resolution_preview(service, resolution_roots, &locked, max_levels).await
    } else {
        prepare_resolution(service, resolution_roots, &locked, max_levels, offline).await
    }
    .map_err(CliError::Service)?;

    let candidate_unchanged: Vec<bool> = resolution
        .candidates
        .iter()
        .map(|candidate| {
            existing.as_ref().is_some_and(|lock| {
                candidate_matches_lock_and_install(service, manifest_dir, candidate, lock)
            })
        })
        .collect();
    let closure_changed = candidate_unchanged.iter().any(|unchanged| !unchanged);

    for (root, id) in roots.iter().zip(&resolution.root_ids) {
        let locked_entry = existing
            .as_ref()
            .and_then(|lock| lock.skills.iter().find(|entry| entry.id == *id));
        let previous_version = locked_entry.map(|entry| entry.resolved.version.clone());
        let origin_changed = locked_entry
            .is_some_and(|entry| entry.origin.resolved_against(manifest_dir) != root.origin);
        let candidate = resolution
            .candidates
            .iter()
            .find(|candidate| candidate.prepared.id() == id)
            .expect("resolution roots always have a candidate");
        let resolved_changed =
            locked_entry.is_none_or(|entry| &entry.resolved != candidate.prepared.resolved());
        let groups_changed = locked_entry
            .is_none_or(|entry| sorted(entry.groups.clone()) != sorted(candidate.groups.clone()));
        let dependencies_changed = locked_entry.is_none_or(|entry| {
            sorted(entry.dependencies.clone()) != sorted(candidate.dependencies.clone())
        });
        let mut changes = Vec::new();
        if origin_changed {
            changes.push("origin changed".to_string());
        }
        if resolved_changed {
            changes.push("resolved content changed".to_string());
        }
        if groups_changed {
            changes.push("groups changed".to_string());
        }
        if dependencies_changed || (closure_changed && changes.is_empty()) {
            changes.push("dependency ownership changed".to_string());
        }
        previews.push(ChangePreview {
            id: id.clone(),
            previous_version,
            resolved_version: candidate.prepared.resolved().version.clone(),
            origin_changed,
            resolved_changed,
            changes,
            warnings: root
                .origin
                .to_manifest_relative(manifest_dir)
                .1
                .map(|warning| vec![warning.warning(id)])
                .unwrap_or_default(),
        });
        manifest_updates.push(SkillEntry {
            id: id.clone(),
            origin: root.origin.clone(),
            groups: root.groups.clone(),
        });
    }

    let positions: BTreeSet<_> = resolution
        .candidates
        .iter()
        .map(|candidate| candidate.prepared.id().to_string())
        .collect();
    let planned = resolution
        .candidates
        .into_iter()
        .map(|candidate| PlannedSkill {
            prepared: candidate.prepared,
            origin: candidate.origin,
            groups: candidate.groups,
            depth: candidate.depth,
            required_by: candidate.required_by,
            dependencies: candidate.dependencies,
        })
        .collect();
    let root_ids = resolution.root_ids;
    let expected_manifest: Vec<_> = root_ids
        .iter()
        .map(
            |id| fastskill_core::core::project_apply::ExpectedManifestSkill {
                id: id.clone(),
                entry: current_manifest
                    .iter()
                    .find(|entry| entry.id == *id)
                    .cloned(),
            },
        )
        .collect();
    let roots_already_covered = existing
        .as_ref()
        .is_some_and(|lock| root_ids.iter().all(|id| lock.covered_roots.contains(id)));
    let expected_skills = existing
        .as_ref()
        .map(|lock| lock.skills.clone())
        .unwrap_or_default();

    let (retained_lock, mut removed) = match existing {
        Some(lock) => {
            let (lock, removed) =
                fastskill_core::core::project_apply::retain_unaffected_roots(lock, &root_ids);
            (Some(lock), removed)
        }
        None => (None, Vec::new()),
    };
    removed.retain(|id| !positions.contains(id));
    let no_op = candidate_unchanged.iter().all(|unchanged| *unchanged)
        && removed.is_empty()
        && roots_already_covered
        && previews.iter().all(|preview| preview.changes.is_empty());
    if no_op {
        return Ok((
            PreparedInstallPlan {
                planned: Vec::new(),
                existing_lock: None,
                newly_covered_roots: Vec::new(),
                used_lock: offline,
                refreshed_repositories: resolution.refreshed_repositories,
                manifest_updates: Vec::new(),
                removed: Vec::new(),
                expected_skills,
                expected_manifest,
            },
            previews,
        ));
    }
    Ok((
        PreparedInstallPlan {
            planned,
            existing_lock: retained_lock,
            newly_covered_roots: root_ids,
            used_lock: offline,
            refreshed_repositories: resolution.refreshed_repositories,
            manifest_updates,
            removed,
            expected_skills,
            expected_manifest,
        },
        previews,
    ))
}

fn candidate_matches_lock_and_install(
    service: &FastSkillService,
    manifest_dir: &Path,
    candidate: &fastskill_core::core::resolution::ResolutionCandidate,
    lock: &ProjectSkillsLock,
) -> bool {
    let Some(entry) = lock
        .skills
        .iter()
        .find(|entry| entry.id == candidate.prepared.id())
    else {
        return false;
    };
    if entry.origin.resolved_against(manifest_dir) != candidate.origin
        || entry.resolved != *candidate.prepared.resolved()
        || sorted(entry.groups.clone()) != sorted(candidate.groups.clone())
        || sorted(entry.dependencies.clone()) != sorted(candidate.dependencies.clone())
        || sorted(entry.required_by.clone())
            != candidate.required_by.iter().cloned().collect::<Vec<_>>()
        || entry.depth != candidate.depth
    {
        return false;
    }
    let installed = service
        .config()
        .skill_storage_path
        .join(candidate.prepared.id());
    match &candidate.origin {
        Origin::Local {
            path: source,
            editable: true,
        } => installed
            .canonicalize()
            .ok()
            .zip(source.canonicalize().ok())
            .is_some_and(|(installed, source)| installed == source),
        _ => entry.resolved.checksum.as_ref().is_some_and(|expected| {
            fastskill_core::core::install::content_digest(&installed)
                .is_ok_and(|actual| &actual == expected)
        }),
    }
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastskill_core::core::lock::ProjectLockedSkillEntry;
    use fastskill_core::core::origin::Resolved;

    fn locked(id: &str, dependencies: &[&str]) -> ProjectLockedSkillEntry {
        ProjectLockedSkillEntry {
            id: id.to_string(),
            name: id.to_string(),
            origin: Origin::Local {
                path: id.into(),
                editable: false,
            },
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum: Some("digest".to_string()),
            },
            dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
            groups: Vec::new(),
            depth: 0,
            parent_skill: None,
            required_by: Vec::new(),
        }
    }

    #[test]
    fn replacing_one_root_keeps_shared_and_unrelated_closures() {
        let mut lock = ProjectSkillsLock::new_empty();
        lock.covered_roots = vec!["a".into(), "b".into()];
        lock.skills = vec![
            locked("a", &["shared", "old-only"]),
            locked("b", &["shared"]),
            locked("shared", &[]),
            locked("old-only", &[]),
        ];

        let (retained, removed) =
            fastskill_core::core::project_apply::retain_unaffected_roots(lock, &["a".into()]);
        let ids: BTreeSet<_> = retained
            .skills
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(ids, BTreeSet::from(["b", "shared"]));
        assert_eq!(retained.covered_roots, vec!["b"]);
        assert_eq!(removed, vec!["a", "old-only"]);
    }
}

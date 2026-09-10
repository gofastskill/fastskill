use crate::error::{CliError, CliResult};
use fastskill_core::core::install::PreparedSkill;
use fastskill_core::core::lock::ProjectSkillsLock;
use fastskill_core::core::manifest::SkillEntry;
use fastskill_core::core::origin::Origin;
use fastskill_core::FastSkillService;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

pub(super) struct InstallSelection<'a> {
    pub roots: Vec<SkillEntry>,
    pub only: Option<&'a [String]>,
    pub without: Option<&'a [String]>,
    pub max_levels: u32,
    pub skip_transitive: bool,
    pub strict: bool,
    pub offline: bool,
}

pub(crate) struct InstallReport {
    pub installed: Vec<String>,
    pub used_lock: bool,
    pub refreshed_repositories: Vec<String>,
    pub mutable_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstallPlanTarget {
    pub id: String,
    pub resolved_version: String,
    pub checksum: Option<String>,
}

pub(super) struct PlannedSkill {
    pub(super) prepared: PreparedSkill,
    pub(super) origin: Origin,
    pub(super) groups: Vec<String>,
    pub(super) depth: u32,
    pub(super) required_by: BTreeSet<String>,
    pub(super) dependencies: Vec<String>,
}

pub(crate) struct PreparedInstallPlan {
    pub(super) planned: Vec<PlannedSkill>,
    pub(super) existing_lock: Option<ProjectSkillsLock>,
    pub(super) newly_covered_roots: Vec<String>,
    pub(super) used_lock: bool,
    pub(super) refreshed_repositories: Vec<String>,
    pub(super) manifest_updates: Vec<SkillEntry>,
    pub(super) removed: Vec<String>,
    pub(super) expected_skills: Vec<fastskill_core::core::lock::ProjectLockedSkillEntry>,
    pub(super) expected_manifest: Vec<fastskill_core::core::project_apply::ExpectedManifestSkill>,
}

impl PreparedInstallPlan {
    pub(crate) fn targets(&self) -> Vec<InstallPlanTarget> {
        let mut targets: Vec<_> = self
            .planned
            .iter()
            .map(|item| InstallPlanTarget {
                id: item.prepared.id().to_string(),
                resolved_version: item.prepared.resolved().version.clone(),
                checksum: item.prepared.resolved().checksum.clone(),
            })
            .collect();
        targets.sort_by(|left, right| left.id.cmp(&right.id));
        targets
    }

    pub(crate) fn affected_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self
            .planned
            .iter()
            .map(|item| item.prepared.id().to_string())
            .chain(self.removed.iter().cloned())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub(crate) fn refreshed_repositories(&self) -> &[String] {
        &self.refreshed_repositories
    }
}

pub(super) async fn prepare(
    service: &FastSkillService,
    lock_path: &Path,
    manifest_dir: &Path,
    selection: InstallSelection<'_>,
) -> CliResult<PreparedInstallPlan> {
    let selected_roots = select_roots(selection.roots, selection.only, selection.without)?;
    if selected_roots.is_empty() {
        return Ok(PreparedInstallPlan {
            planned: Vec::new(),
            existing_lock: None,
            newly_covered_roots: Vec::new(),
            used_lock: false,
            refreshed_repositories: Vec::new(),
            manifest_updates: Vec::new(),
            removed: Vec::new(),
            expected_skills: Vec::new(),
            expected_manifest: Vec::new(),
        });
    }

    let expected_manifest = selected_roots
        .iter()
        .cloned()
        .map(
            |entry| fastskill_core::core::project_apply::ExpectedManifestSkill {
                id: entry.id.clone(),
                entry: Some(entry),
            },
        )
        .collect();
    let existing_lock = if lock_path.exists() {
        Some(
            ProjectSkillsLock::load_from_file(lock_path)
                .map_err(|error| CliError::Config(format!("Failed to load lock file: {error}")))?,
        )
    } else {
        None
    };

    let selected_ids: BTreeSet<String> = selected_roots
        .iter()
        .map(|entry| entry.id.clone())
        .collect();

    if selection.strict {
        let lock = existing_lock.as_ref().ok_or_else(|| {
            CliError::Config(
                "skills.lock not found. Run 'fastskill project install' first to create it."
                    .to_string(),
            )
        })?;
        if !roots_have_compatible_coverage(lock, &selected_roots, manifest_dir)? {
            return Err(CliError::Config(format!(
                "skills.lock does not contain compatible complete coverage for: {}",
                selected_ids.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
        let planned = prepare_locked(
            service,
            lock,
            &selected_roots,
            manifest_dir,
            selection.max_levels,
            selection.offline,
        )
        .await?;
        return Ok(PreparedInstallPlan {
            planned,
            existing_lock: None,
            newly_covered_roots: Vec::new(),
            used_lock: true,
            refreshed_repositories: Vec::new(),
            manifest_updates: Vec::new(),
            removed: Vec::new(),
            expected_skills: lock.skills.clone(),
            expected_manifest,
        });
    }

    let mut planned = Vec::new();
    let expected_skills = existing_lock
        .as_ref()
        .map(|lock| lock.skills.clone())
        .unwrap_or_default();
    let mut fresh_roots = selected_roots.clone();
    let mut newly_covered = selected_ids.clone();
    if let Some(lock) = &existing_lock {
        let mut locked_roots = Vec::new();
        fresh_roots.retain(|root| {
            if lock.skills.iter().any(|entry| entry.id == root.id) {
                locked_roots.push(root.clone());
                false
            } else {
                true
            }
        });
        validate_recorded_roots(lock, &locked_roots, manifest_dir)?;
        if !locked_roots.is_empty() {
            planned = prepare_locked(
                service,
                lock,
                &locked_roots,
                manifest_dir,
                selection.max_levels,
                selection.offline,
            )
            .await?;
        }
        newly_covered.retain(|id| !lock.covered_roots.contains(id));
    }

    if selection.offline
        && fresh_roots
            .iter()
            .any(|root| !matches!(root.origin, Origin::Local { .. }))
    {
        return Err(CliError::Config(format!(
            "offline installation is missing locked artifacts for: {}",
            fresh_roots
                .iter()
                .filter(|root| !matches!(root.origin, Origin::Local { .. }))
                .map(|root| root.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let used_lock = fresh_roots.is_empty();
    let mut refreshed_repositories = Vec::new();
    if !fresh_roots.is_empty() {
        let (fresh, refreshed) = prepare_fresh(
            service,
            fresh_roots,
            selection.max_levels,
            selection.skip_transitive,
            selection.offline,
        )
        .await?;
        refreshed_repositories = refreshed;
        merge_plans(&mut planned, fresh)?;
    }
    let roots: Vec<String> = newly_covered.into_iter().collect();
    Ok(PreparedInstallPlan {
        planned,
        existing_lock,
        newly_covered_roots: roots,
        used_lock,
        refreshed_repositories,
        manifest_updates: Vec::new(),
        removed: Vec::new(),
        expected_skills,
        expected_manifest,
    })
}

pub(crate) async fn apply_prepared(
    service: &FastSkillService,
    _lock_path: &Path,
    manifest_dir: &Path,
    prepared: PreparedInstallPlan,
) -> CliResult<InstallReport> {
    apply_core_plan(service, manifest_dir, prepared, None).await
}

/// Run the same ownership, unmanaged-destination, stale-state, and installed
/// content checks as apply, while keeping persistent state unchanged.
pub(crate) fn validate_prepared(
    service: &FastSkillService,
    manifest_dir: &Path,
    prepared: PreparedInstallPlan,
) -> CliResult<()> {
    let (plan, _, _, _, _) = into_core_plan(prepared);
    service
        .validate_project_plan(manifest_dir, &plan)
        .map_err(CliError::Service)
}

pub(crate) async fn apply_prepared_with_guard(
    service: &FastSkillService,
    _lock_path: &Path,
    manifest_dir: &Path,
    prepared: PreparedInstallPlan,
    guard: &fastskill_core::core::state_guard::StateMutationGuard,
) -> CliResult<InstallReport> {
    apply_core_plan(service, manifest_dir, prepared, Some(guard)).await
}

async fn apply_core_plan(
    service: &FastSkillService,
    manifest_dir: &Path,
    prepared: PreparedInstallPlan,
    guard: Option<&fastskill_core::core::state_guard::StateMutationGuard>,
) -> CliResult<InstallReport> {
    let (plan, used_lock, refreshed_repositories, mutable_sources, _) = into_core_plan(prepared);
    let result = match guard {
        Some(guard) => {
            service
                .apply_project_plan_with_guard(manifest_dir, plan, guard)
                .await
        }
        None => service.apply_project_plan(manifest_dir, plan).await,
    }
    .map_err(CliError::Service)?;
    Ok(InstallReport {
        installed: result.installed,
        used_lock,
        refreshed_repositories,
        mutable_sources,
    })
}

fn into_core_plan(
    prepared: PreparedInstallPlan,
) -> (
    fastskill_core::core::project_apply::ProjectApplyPlan,
    bool,
    Vec<String>,
    Vec<String>,
    bool,
) {
    let PreparedInstallPlan {
        planned,
        existing_lock,
        newly_covered_roots,
        used_lock,
        refreshed_repositories,
        manifest_updates,
        removed,
        expected_skills,
        expected_manifest,
    } = prepared;
    let mut mutable_sources: Vec<_> = planned
        .iter()
        .filter(|candidate| matches!(candidate.origin, Origin::Local { editable: true, .. }))
        .map(|candidate| candidate.prepared.id().to_string())
        .collect();
    mutable_sources.sort();
    let write_lock = !newly_covered_roots.is_empty();
    let plan = fastskill_core::core::project_apply::ProjectApplyPlan {
        candidates: planned
            .into_iter()
            .map(
                |candidate| fastskill_core::core::project_apply::ProjectApplyCandidate {
                    prepared: candidate.prepared,
                    origin: candidate.origin,
                    groups: candidate.groups,
                    depth: candidate.depth,
                    required_by: candidate.required_by,
                    dependencies: candidate.dependencies,
                },
            )
            .collect(),
        base_lock: existing_lock,
        covered_roots: newly_covered_roots,
        manifest_updates,
        removals: removed,
        write_lock,
        expected_skills,
        expected_manifest,
    };
    (
        plan,
        used_lock,
        refreshed_repositories,
        mutable_sources,
        write_lock,
    )
}

pub(crate) fn validate_declared_bundle_members(
    prepared: &PreparedInstallPlan,
    members: &[fastskill_core::core::bundle::DeclaredBundleMember],
) -> CliResult<()> {
    for planned in &prepared.planned {
        for member in members
            .iter()
            .filter(|member| member.id == planned.prepared.id())
        {
            if !member.overridable
                && planned.prepared.resolved().checksum.as_deref() != Some(member.digest.as_str())
            {
                return Err(CliError::Config(format!(
                    "individual skill '{}' conflicts with member content from bundle '{}'",
                    member.id, member.bundle_id
                )));
            }
        }
    }
    Ok(())
}

fn roots_have_compatible_coverage(
    lock: &ProjectSkillsLock,
    roots: &[SkillEntry],
    manifest_dir: &Path,
) -> CliResult<bool> {
    if !roots
        .iter()
        .all(|root| lock.covered_roots.contains(&root.id))
    {
        return Ok(false);
    }
    validate_recorded_roots(lock, roots, manifest_dir)?;
    Ok(true)
}

fn validate_recorded_roots(
    lock: &ProjectSkillsLock,
    roots: &[SkillEntry],
    manifest_dir: &Path,
) -> CliResult<()> {
    for root in roots {
        let locked = lock
            .skills
            .iter()
            .find(|entry| entry.id == root.id)
            .ok_or_else(|| CliError::Config(format!("skills.lock is missing '{}'", root.id)))?;
        if locked.origin.resolved_against(manifest_dir) != root.origin {
            return Err(CliError::Config(format!(
                "locked intent for '{}' differs from the Manifest; run `fastskill skill update {}`",
                root.id, root.id
            )));
        }
        let immutable = !matches!(locked.origin, Origin::Local { editable: true, .. });
        if immutable && locked.resolved.checksum.is_none() {
            return Err(CliError::Config(format!(
                "skills.lock has no integrity evidence for '{}'; run `fastskill skill update {}`",
                root.id, root.id
            )));
        }
        if matches!(locked.origin, Origin::Git { .. }) && locked.resolved.commit_hash.is_none() {
            return Err(CliError::Config(format!(
                "skills.lock has no commit for '{}'; run `fastskill skill update {}`",
                root.id, root.id
            )));
        }
    }
    Ok(())
}

fn merge_plans(target: &mut Vec<PlannedSkill>, incoming: Vec<PlannedSkill>) -> CliResult<()> {
    for mut candidate in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|item| item.prepared.id() == candidate.prepared.id())
        {
            if !fastskill_core::core::resolution::origins_accept_same_resolution(
                &existing.origin,
                &candidate.origin,
                &existing.prepared.resolved().version,
            ) || existing.prepared.resolved() != candidate.prepared.resolved()
            {
                return Err(CliError::Config(format!(
                    "dependency '{}' resolves incompatibly across selected roots",
                    candidate.prepared.id()
                )));
            }
            existing.required_by.append(&mut candidate.required_by);
            existing.groups.extend(candidate.groups);
            existing.groups.sort();
            existing.groups.dedup();
            existing.depth = existing.depth.min(candidate.depth);
            existing.dependencies.extend(candidate.dependencies);
            existing.dependencies.sort();
            existing.dependencies.dedup();
        } else {
            target.push(candidate);
        }
    }
    Ok(())
}

fn select_roots(
    roots: Vec<SkillEntry>,
    only: Option<&[String]>,
    without: Option<&[String]>,
) -> CliResult<Vec<SkillEntry>> {
    if only.is_some() && without.is_some() {
        return Err(CliError::Validation(
            "--only and --without cannot be used together".to_string(),
        ));
    }

    let available: BTreeSet<String> = roots
        .iter()
        .flat_map(|root| {
            if root.groups.is_empty() {
                vec!["default".to_string()]
            } else {
                root.groups.clone()
            }
        })
        .collect();
    for requested in only.into_iter().chain(without).flatten() {
        if !available.contains(requested) {
            return Err(CliError::Validation(format!(
                "unknown dependency group '{requested}'"
            )));
        }
    }

    Ok(roots
        .into_iter()
        .filter(|root| {
            let groups = if root.groups.is_empty() {
                vec!["default".to_string()]
            } else {
                root.groups.clone()
            };
            only.is_none_or(|wanted| groups.iter().any(|group| wanted.contains(group)))
                && without
                    .is_none_or(|excluded| !groups.iter().any(|group| excluded.contains(group)))
        })
        .collect())
}

async fn prepare_fresh(
    service: &FastSkillService,
    mut roots: Vec<SkillEntry>,
    max_levels: u32,
    skip_transitive: bool,
    offline: bool,
) -> CliResult<(Vec<PlannedSkill>, Vec<String>)> {
    roots.sort_by(|left, right| left.id.cmp(&right.id));
    let selected = roots
        .into_iter()
        .map(|entry| fastskill_core::core::resolution::ResolutionRoot {
            expected_id: Some(entry.id),
            origin: entry.origin,
            groups: entry.groups,
            locked: None,
        })
        .collect();
    let resolution = fastskill_core::core::resolution::prepare_resolution(
        service,
        selected,
        &HashMap::new(),
        max_levels,
        offline,
    )
    .await
    .map_err(CliError::Service)?;
    if skip_transitive {
        if let Some(candidate) = resolution
            .candidates
            .iter()
            .find(|candidate| !candidate.dependencies.is_empty())
        {
            return Err(CliError::Config(format!(
                "{} has required dependencies; skip_transitive cannot produce a complete installation",
                candidate.prepared.id()
            )));
        }
    }
    Ok((
        planned_from_resolution(resolution.candidates),
        resolution.refreshed_repositories,
    ))
}

async fn prepare_locked(
    service: &FastSkillService,
    lock: &ProjectSkillsLock,
    selected_roots: &[SkillEntry],
    manifest_dir: &Path,
    max_levels: u32,
    offline: bool,
) -> CliResult<Vec<PlannedSkill>> {
    let entries = lock
        .skills
        .iter()
        .map(|entry| {
            (
                entry.id.clone(),
                fastskill_core::core::resolution::ResolutionLockEntry {
                    origin: entry.origin.resolved_against(manifest_dir),
                    resolved: entry.resolved.clone(),
                    dependencies: entry.dependencies.clone(),
                    groups: entry.groups.clone(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let roots = selected_roots
        .iter()
        .map(|root| {
            let locked = entries
                .get(&root.id)
                .ok_or_else(|| CliError::Config(format!("skills.lock is missing {}", root.id)))?;
            Ok(fastskill_core::core::resolution::ResolutionRoot {
                origin: locked.origin.clone(),
                expected_id: Some(root.id.clone()),
                groups: locked.groups.clone(),
                locked: Some(locked.resolved.clone()),
            })
        })
        .collect::<CliResult<Vec<_>>>()?;
    let resolution = fastskill_core::core::resolution::prepare_locked_resolution_entries(
        service, roots, &entries, max_levels, offline,
    )
    .await
    .map_err(CliError::Service)?;
    Ok(planned_from_resolution(resolution.candidates))
}

fn planned_from_resolution(
    candidates: Vec<fastskill_core::core::resolution::ResolutionCandidate>,
) -> Vec<PlannedSkill> {
    candidates
        .into_iter()
        .map(|candidate| PlannedSkill {
            prepared: candidate.prepared,
            origin: candidate.origin,
            groups: candidate.groups,
            depth: candidate.depth,
            required_by: candidate.required_by,
            dependencies: candidate.dependencies,
        })
        .collect()
}

#[cfg(test)]
#[path = "plan/tests.rs"]
mod tests;

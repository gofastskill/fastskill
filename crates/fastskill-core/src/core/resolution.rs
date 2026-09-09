//! Shared dependency-closure preparation for CLI and HTTP lifecycle surfaces.

use crate::core::install::PreparedSkill;
use crate::core::manifest::SkillEntry;
use crate::core::origin::{GitRef, Origin, Resolved};
use crate::core::service::{FastSkillService, ServiceError};
use crate::core::version::VersionConstraint;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Debug, Clone)]
pub struct ResolutionRoot {
    pub origin: Origin,
    pub expected_id: Option<String>,
    pub groups: Vec<String>,
    pub locked: Option<Resolved>,
}

#[derive(Debug, Clone)]
pub struct ResolutionLockEntry {
    pub origin: Origin,
    pub resolved: Resolved,
    pub dependencies: Vec<String>,
    pub groups: Vec<String>,
}

#[derive(Debug)]
pub struct ResolutionCandidate {
    pub prepared: PreparedSkill,
    pub origin: Origin,
    pub groups: Vec<String>,
    pub depth: u32,
    pub required_by: BTreeSet<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug)]
pub struct ResolutionPlan {
    pub candidates: Vec<ResolutionCandidate>,
    pub root_ids: Vec<String>,
    pub refreshed_repositories: Vec<String>,
}

struct QueueItem {
    entry: SkillEntry,
    depth: u32,
    required_by: String,
    chain: Vec<String>,
}

enum ResolutionAttempt {
    Complete {
        candidates: Vec<ResolutionCandidate>,
        root_ids: Vec<String>,
    },
    Retry {
        id: String,
        origin: Origin,
    },
}

/// Resolve every root and required child before callers mutate installed state.
/// The same plan can drive project CLI, HTTP, or another adapter.
pub async fn prepare_resolution(
    service: &FastSkillService,
    roots: Vec<ResolutionRoot>,
    locked: &HashMap<String, Resolved>,
    max_levels: u32,
    offline: bool,
) -> Result<ResolutionPlan, ServiceError> {
    prepare_resolution_with_policy(service, roots, locked, None, max_levels, offline, false).await
}

/// Resolve a fresh closure against current remote metadata while keeping the
/// configured catalog, cache, installed files, and timestamps unchanged.
pub async fn prepare_resolution_preview(
    service: &FastSkillService,
    roots: Vec<ResolutionRoot>,
    locked: &HashMap<String, Resolved>,
    max_levels: u32,
) -> Result<ResolutionPlan, ServiceError> {
    let preview_storage = tempfile::TempDir::new().map_err(ServiceError::Io)?;
    let preview_cache = tempfile::TempDir::new().map_err(ServiceError::Io)?;
    let mut config = service.config().clone();
    config.skill_storage_path = preview_storage.path().to_path_buf();
    config.skill_cache_root = Some(preview_cache.path().to_path_buf());
    let mut isolated = FastSkillService::new(config).await?;
    if let Some(manager) = service.repository_manager() {
        isolated = isolated.with_repository_manager(manager.clone());
    }
    isolated.initialize().await?;
    prepare_resolution(&isolated, roots, locked, max_levels, false).await
}

/// Restore a dependency closure from recorded immutable facts. Online mode may
/// acquire missing content, but it never advances a version, commit, or digest.
pub async fn prepare_locked_resolution(
    service: &FastSkillService,
    roots: Vec<ResolutionRoot>,
    locked: &HashMap<String, Resolved>,
    max_levels: u32,
    offline: bool,
) -> Result<ResolutionPlan, ServiceError> {
    prepare_resolution_with_policy(service, roots, locked, None, max_levels, offline, true).await
}

/// Restore a closure from complete Lock entries, using recorded origins for
/// every transitive candidate and verifying the fetched dependency graph.
pub async fn prepare_locked_resolution_entries(
    service: &FastSkillService,
    roots: Vec<ResolutionRoot>,
    locked: &HashMap<String, ResolutionLockEntry>,
    max_levels: u32,
    offline: bool,
) -> Result<ResolutionPlan, ServiceError> {
    let resolved = locked
        .iter()
        .map(|(id, entry)| (id.clone(), entry.resolved.clone()))
        .collect();
    prepare_resolution_with_policy(
        service,
        roots,
        &resolved,
        Some(locked),
        max_levels,
        offline,
        true,
    )
    .await
}

/// Preview an exact Lock restoration without populating the caller's
/// persistent catalog or content caches.
pub async fn prepare_locked_resolution_entries_preview(
    service: &FastSkillService,
    roots: Vec<ResolutionRoot>,
    locked: &HashMap<String, ResolutionLockEntry>,
    max_levels: u32,
) -> Result<ResolutionPlan, ServiceError> {
    let preview_storage = tempfile::TempDir::new().map_err(ServiceError::Io)?;
    let preview_cache = tempfile::TempDir::new().map_err(ServiceError::Io)?;
    let mut config = service.config().clone();
    config.skill_storage_path = preview_storage.path().to_path_buf();
    config.skill_cache_root = Some(preview_cache.path().to_path_buf());
    let mut isolated = FastSkillService::new(config).await?;
    if let Some(manager) = service.repository_manager() {
        isolated = isolated.with_repository_manager(manager.clone());
    }
    isolated.initialize().await?;
    prepare_locked_resolution_entries(&isolated, roots, locked, max_levels, false).await
}

async fn prepare_resolution_with_policy(
    service: &FastSkillService,
    roots: Vec<ResolutionRoot>,
    locked: &HashMap<String, Resolved>,
    locked_entries: Option<&HashMap<String, ResolutionLockEntry>>,
    max_levels: u32,
    offline: bool,
    lock_first: bool,
) -> Result<ResolutionPlan, ServiceError> {
    if max_levels == 0 {
        return Err(ServiceError::Validation(
            "dependency depth must include at least the root level".to_string(),
        ));
    }
    let mut refreshed = HashSet::new();
    let mut refreshed_names = BTreeSet::new();
    let mut forced = HashMap::new();
    loop {
        match prepare_resolution_attempt(
            service,
            &roots,
            locked,
            locked_entries,
            max_levels,
            offline,
            lock_first,
            &forced,
            &mut refreshed,
            &mut refreshed_names,
        )
        .await?
        {
            ResolutionAttempt::Complete {
                candidates,
                root_ids,
            } => {
                return Ok(ResolutionPlan {
                    candidates,
                    root_ids,
                    refreshed_repositories: refreshed_names.into_iter().collect(),
                });
            }
            ResolutionAttempt::Retry { id, origin } => {
                if forced.insert(id.clone(), origin.clone()).as_ref() == Some(&origin) {
                    return Err(ServiceError::Validation(format!(
                        "dependency '{id}' could not be resolved to satisfy every parent constraint"
                    )));
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn prepare_resolution_attempt(
    service: &FastSkillService,
    roots: &[ResolutionRoot],
    locked: &HashMap<String, Resolved>,
    locked_entries: Option<&HashMap<String, ResolutionLockEntry>>,
    max_levels: u32,
    offline: bool,
    lock_first: bool,
    forced: &HashMap<String, Origin>,
    refreshed: &mut HashSet<String>,
    refreshed_names: &mut BTreeSet<String>,
) -> Result<ResolutionAttempt, ServiceError> {
    let mut candidates = Vec::new();
    let mut positions = HashMap::new();
    let mut queue = VecDeque::new();
    let mut root_ids = Vec::new();

    for root in roots.iter().cloned() {
        refresh_if_floating(
            service,
            &root.origin,
            offline || lock_first,
            refreshed,
            refreshed_names,
        )
        .await?;
        let prepared = prepare_candidate(
            service,
            root.origin.clone(),
            root.expected_id.as_deref(),
            root.locked.as_ref(),
            offline,
            lock_first,
        )
        .await?;
        let id = prepared.id().to_string();
        if positions.contains_key(&id) {
            return Err(ServiceError::Validation(format!(
                "skill '{id}' was selected more than once"
            )));
        }
        validate_locked_dependencies(
            &prepared,
            locked_entries.and_then(|entries| entries.get(&id)),
        )?;
        let dependencies = checked_dependencies(&prepared, &id, 0, max_levels, &[])?;
        enqueue_dependencies(&mut queue, &dependencies, &id, 1, vec![id.clone()]);
        positions.insert(id.clone(), candidates.len());
        root_ids.push(id.clone());
        candidates.push(ResolutionCandidate {
            prepared,
            origin: root.origin,
            groups: root.groups,
            depth: 0,
            required_by: BTreeSet::new(),
            dependencies: dependencies.into_iter().map(|entry| entry.id).collect(),
        });
    }

    while let Some(mut item) = queue.pop_front() {
        let mut origins = vec![item.entry.origin.clone()];
        let mut parents = vec![item.required_by.clone()];
        queue.retain(|queued| {
            if queued.entry.id == item.entry.id {
                origins.push(queued.entry.origin.clone());
                parents.push(queued.required_by.clone());
                false
            } else {
                true
            }
        });
        if let Some(entry) = locked_entries.and_then(|entries| entries.get(&item.entry.id)) {
            for requested in &origins {
                validate_locked_requirement(&item.entry.id, requested, entry)?;
            }
            origins.push(entry.origin.clone());
            item.entry.groups = entry.groups.clone();
        }
        if let Some(origin) = forced.get(&item.entry.id) {
            origins.push(origin.clone());
        }
        item.entry.origin = merge_requirement_origins(&item.entry.id, &origins)?;
        if item.chain.contains(&item.entry.id) {
            let mut cycle = item.chain.clone();
            cycle.push(item.entry.id.clone());
            return Err(ServiceError::Validation(format!(
                "circular dependency detected: {}",
                cycle.join(" -> ")
            )));
        }
        if let Some(position) = positions.get(&item.entry.id).copied() {
            let existing = &mut candidates[position];
            let merged = merge_requirement_origins(
                &item.entry.id,
                &[existing.origin.clone(), item.entry.origin],
            )?;
            if let Err(error) = ensure_origin_accepts_version(
                &item.entry.id,
                &merged,
                &existing.prepared.resolved().version,
            ) {
                if lock_first {
                    return Err(error);
                }
                return Ok(ResolutionAttempt::Retry {
                    id: item.entry.id,
                    origin: merged,
                });
            }
            existing.origin = merged;
            existing.required_by.extend(parents);
            continue;
        }
        refresh_if_floating(
            service,
            &item.entry.origin,
            offline || lock_first,
            refreshed,
            refreshed_names,
        )
        .await?;
        let prepared = prepare_candidate(
            service,
            item.entry.origin.clone(),
            Some(&item.entry.id),
            locked.get(&item.entry.id),
            offline,
            lock_first,
        )
        .await?;
        validate_locked_dependencies(
            &prepared,
            locked_entries.and_then(|entries| entries.get(&item.entry.id)),
        )?;
        let dependencies = checked_dependencies(
            &prepared,
            &item.entry.id,
            item.depth,
            max_levels,
            &item.chain,
        )?;
        enqueue_dependencies(
            &mut queue,
            &dependencies,
            &item.entry.id,
            item.depth + 1,
            item.chain,
        );
        let mut required_by = BTreeSet::new();
        required_by.extend(parents);
        positions.insert(item.entry.id.clone(), candidates.len());
        candidates.push(ResolutionCandidate {
            prepared,
            origin: item.entry.origin,
            groups: item.entry.groups,
            depth: item.depth,
            required_by,
            dependencies: dependencies.into_iter().map(|entry| entry.id).collect(),
        });
    }

    validate_acyclic(&candidates)?;

    Ok(ResolutionAttempt::Complete {
        candidates,
        root_ids,
    })
}

fn validate_locked_requirement(
    id: &str,
    requested: &Origin,
    locked: &ResolutionLockEntry,
) -> Result<(), ServiceError> {
    if origins_accept_same_resolution(requested, &locked.origin, &locked.resolved.version) {
        Ok(())
    } else {
        Err(ServiceError::Validation(format!(
            "locked selection for '{id}' does not satisfy its parent's declared origin or version constraint"
        )))
    }
}

fn ensure_origin_accepts_version(
    id: &str,
    origin: &Origin,
    version: &str,
) -> Result<(), ServiceError> {
    let accepts = match origin {
        Origin::Repository {
            version: Some(constraint),
            ..
        } => constraint.satisfies(version).map_err(|error| {
            ServiceError::Validation(format!(
                "resolved version '{version}' for '{id}' is invalid: {error}"
            ))
        })?,
        _ => true,
    };
    if accepts {
        Ok(())
    } else {
        Err(ServiceError::Validation(format!(
            "resolved version '{version}' for '{id}' does not satisfy every parent constraint"
        )))
    }
}

fn validate_acyclic(candidates: &[ResolutionCandidate]) -> Result<(), ServiceError> {
    let graph: HashMap<_, _> = candidates
        .iter()
        .map(|candidate| (candidate.prepared.id(), candidate.dependencies.as_slice()))
        .collect();
    let mut complete = HashSet::new();
    let mut active = HashSet::new();
    let mut chain = Vec::new();
    for id in graph.keys().copied() {
        visit_dependency(id, &graph, &mut active, &mut complete, &mut chain)?;
    }
    Ok(())
}

fn visit_dependency<'a>(
    id: &'a str,
    graph: &HashMap<&'a str, &'a [String]>,
    active: &mut HashSet<&'a str>,
    complete: &mut HashSet<&'a str>,
    chain: &mut Vec<&'a str>,
) -> Result<(), ServiceError> {
    if complete.contains(id) {
        return Ok(());
    }
    if !active.insert(id) {
        let start = chain.iter().position(|entry| *entry == id).unwrap_or(0);
        let mut cycle = chain[start..].to_vec();
        cycle.push(id);
        return Err(ServiceError::Validation(format!(
            "circular dependency detected: {}",
            cycle.join(" -> ")
        )));
    }
    chain.push(id);
    if let Some(dependencies) = graph.get(id) {
        for dependency in *dependencies {
            visit_dependency(dependency, graph, active, complete, chain)?;
        }
    }
    chain.pop();
    active.remove(id);
    complete.insert(id);
    Ok(())
}

fn validate_locked_dependencies(
    prepared: &PreparedSkill,
    locked: Option<&ResolutionLockEntry>,
) -> Result<(), ServiceError> {
    let Some(locked) = locked else {
        return Ok(());
    };
    let mut actual: Vec<_> = prepared
        .dependencies()
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    actual.sort();
    let mut expected = locked.dependencies.clone();
    expected.sort();
    if actual != expected {
        return Err(ServiceError::Validation(format!(
            "locked dependency graph for '{}' differs from its verified manifest",
            prepared.id()
        )));
    }
    Ok(())
}

async fn prepare_candidate(
    service: &FastSkillService,
    origin: Origin,
    expected_id: Option<&str>,
    locked: Option<&Resolved>,
    offline: bool,
    lock_first: bool,
) -> Result<PreparedSkill, ServiceError> {
    if offline || lock_first {
        if let Some(resolved) = locked {
            validate_locked_resolution(
                &origin,
                resolved,
                expected_id.unwrap_or("the selected skill"),
            )?;
        }
    }
    if offline {
        if let Some(resolved) = locked {
            let id = expected_id.ok_or_else(|| {
                ServiceError::Validation("offline update requires a locked skill ID".to_string())
            })?;
            service.prepare_install_offline(origin, id, resolved).await
        } else {
            service.prepare_add_offline(origin, expected_id).await
        }
    } else if lock_first {
        let resolved = locked.ok_or_else(|| {
            ServiceError::Validation(format!(
                "locked restoration is missing immutable facts for '{}'",
                expected_id.unwrap_or("the selected skill")
            ))
        })?;
        let id = expected_id.ok_or_else(|| {
            ServiceError::Validation("locked restoration requires a skill ID".to_string())
        })?;
        let acquisition = pinned_origin(&origin, resolved, id)?;
        service
            .prepare_install_from(acquisition, origin, id, Some(resolved))
            .await
    } else {
        service.prepare_add(origin, expected_id).await
    }
}

/// Reject legacy/incomplete Lock entries before observing source bytes. Every
/// immutable source requires a canonical content digest; Git also requires the
/// exact commit used for acquisition. Editable local sources are intentionally
/// mutable and therefore exempt from digest pinning.
pub fn validate_locked_resolution(
    origin: &Origin,
    resolved: &Resolved,
    id: &str,
) -> Result<(), ServiceError> {
    if matches!(origin, Origin::Local { editable: true, .. }) {
        return Ok(());
    }
    if resolved.checksum.as_deref().is_none_or(str::is_empty) {
        return Err(ServiceError::Validation(format!(
            "locked skill '{id}' has no content digest; run an explicit update online"
        )));
    }
    if matches!(origin, Origin::Git { .. })
        && resolved.commit_hash.as_deref().is_none_or(str::is_empty)
    {
        return Err(ServiceError::Validation(format!(
            "locked Git skill '{id}' has no commit; run an explicit update online"
        )));
    }
    Ok(())
}

fn pinned_origin(origin: &Origin, resolved: &Resolved, id: &str) -> Result<Origin, ServiceError> {
    Ok(match origin {
        Origin::Git { url, subdir, .. } => Origin::Git {
            url: url.clone(),
            r#ref: GitRef::Commit(resolved.commit_hash.clone().ok_or_else(|| {
                ServiceError::Validation(format!(
                    "locked Git skill '{id}' has no commit; run an explicit update"
                ))
            })?),
            subdir: subdir.clone(),
        },
        Origin::Repository { repo, skill, .. } => Origin::Repository {
            repo: repo.clone(),
            skill: skill.clone(),
            version: Some(
                VersionConstraint::parse(&resolved.version).map_err(|error| {
                    ServiceError::Validation(format!(
                        "locked repository skill '{id}' has an invalid version: {error}"
                    ))
                })?,
            ),
        },
        other => other.clone(),
    })
}

async fn refresh_if_floating(
    service: &FastSkillService,
    origin: &Origin,
    offline: bool,
    refreshed: &mut HashSet<String>,
    names: &mut BTreeSet<String>,
) -> Result<(), ServiceError> {
    let Origin::Repository {
        repo,
        skill,
        version,
    } = origin
    else {
        return Ok(());
    };
    if offline
        || version
            .as_ref()
            .and_then(VersionConstraint::as_exact)
            .is_some()
    {
        return Ok(());
    }
    if refreshed.insert(repo.clone()) {
        names.insert(service.refresh_repository_requirement(repo, skill).await?);
    }
    Ok(())
}

fn checked_dependencies(
    prepared: &PreparedSkill,
    id: &str,
    depth: u32,
    max_levels: u32,
    chain: &[String],
) -> Result<Vec<SkillEntry>, ServiceError> {
    let mut dependencies = prepared.dependencies().to_vec();
    dependencies.sort_by(|left, right| left.id.cmp(&right.id));
    for dependency in &dependencies {
        if dependency.id == id || chain.contains(&dependency.id) {
            let mut cycle = chain.to_vec();
            if cycle.last().is_none_or(|current| current != id) {
                cycle.push(id.to_string());
            }
            cycle.push(dependency.id.clone());
            return Err(ServiceError::Validation(format!(
                "circular dependency detected: {}",
                cycle.join(" -> ")
            )));
        }
        if depth + 1 >= max_levels {
            let mut unresolved = chain.to_vec();
            if unresolved.is_empty() {
                unresolved.push(id.to_string());
            }
            unresolved.push(dependency.id.clone());
            return Err(ServiceError::Validation(format!(
                "dependency depth limit {max_levels} leaves required dependency unresolved: {}",
                unresolved.join(" -> ")
            )));
        }
    }
    Ok(dependencies)
}

fn enqueue_dependencies(
    queue: &mut VecDeque<QueueItem>,
    dependencies: &[SkillEntry],
    parent: &str,
    depth: u32,
    mut chain: Vec<String>,
) {
    if chain.last().is_none_or(|last| last != parent) {
        chain.push(parent.to_string());
    }
    for dependency in dependencies {
        queue.push_back(QueueItem {
            entry: dependency.clone(),
            depth,
            required_by: parent.to_string(),
            chain: chain.clone(),
        });
    }
}

fn merge_requirement_origins(id: &str, origins: &[Origin]) -> Result<Origin, ServiceError> {
    let Some(first) = origins.first() else {
        return Err(ServiceError::Validation(format!(
            "dependency '{id}' has no origin"
        )));
    };
    if origins.iter().all(|origin| origin == first) {
        return Ok(first.clone());
    }
    let mut constraints = Vec::new();
    let mut repo_skill = None;
    for origin in origins {
        let Origin::Repository {
            repo,
            skill,
            version,
        } = origin
        else {
            return Err(ServiceError::Validation(format!(
                "dependency '{id}' is required from incompatible origins"
            )));
        };
        if repo_skill
            .as_ref()
            .is_some_and(|(known_repo, known_skill)| known_repo != repo || known_skill != skill)
        {
            return Err(ServiceError::Validation(format!(
                "dependency '{id}' is required from incompatible repositories"
            )));
        }
        repo_skill = Some((repo.clone(), skill.clone()));
        if let Some(version) = version {
            let value = version.to_string();
            if value != "*" {
                constraints.push(value);
            }
        }
    }
    constraints.sort();
    constraints.dedup();
    let (repo, skill) = repo_skill.ok_or_else(|| {
        ServiceError::Validation(format!("dependency '{id}' has no repository origin"))
    })?;
    let version = if constraints.is_empty() {
        None
    } else {
        Some(
            VersionConstraint::parse(&constraints.join(", ")).map_err(|error| {
                ServiceError::Validation(format!(
                    "dependency '{id}' has incompatible version requirements: {error}"
                ))
            })?,
        )
    };
    Ok(Origin::Repository {
        repo,
        skill,
        version,
    })
}

pub fn origins_accept_same_resolution(left: &Origin, right: &Origin, version: &str) -> bool {
    if left == right {
        return true;
    }
    match (left, right) {
        (
            Origin::Repository {
                repo: left_repo,
                skill: left_skill,
                version: left_version,
            },
            Origin::Repository {
                repo: right_repo,
                skill: right_skill,
                version: right_version,
            },
        ) if left_repo == right_repo && left_skill == right_skill => {
            left_version
                .as_ref()
                .is_none_or(|constraint| constraint.satisfies(version).unwrap_or(false))
                && right_version
                    .as_ref()
                    .is_none_or(|constraint| constraint.satisfies(version).unwrap_or(false))
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "resolution/tests.rs"]
mod tests;

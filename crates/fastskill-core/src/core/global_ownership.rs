//! Reachability planning for global skill removal.

use crate::core::lock::GlobalSkillsLock;
use crate::core::service::ServiceError;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRemovalPlan {
    pub remove_roots: Vec<String>,
    pub remove_lock_entries: Vec<String>,
    pub delete_files: Vec<String>,
    pub retained_files: Vec<String>,
    pub unchanged: Vec<String>,
    pub remaining_roots: Vec<String>,
}

pub fn plan_removal(
    lock: &GlobalSkillsLock,
    requested: &[String],
) -> Result<GlobalRemovalPlan, ServiceError> {
    let dependencies: BTreeMap<&str, Vec<&str>> = lock
        .skills
        .iter()
        .map(|entry| {
            (
                entry.id.as_str(),
                entry.dependencies.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let roots: BTreeSet<&str> = lock.covered_roots.iter().map(String::as_str).collect();
    let locked: BTreeSet<&str> = lock.skills.iter().map(|entry| entry.id.as_str()).collect();
    let mut removed_roots = BTreeSet::new();
    let mut cleanup = BTreeSet::new();
    let mut unchanged = BTreeSet::new();
    for id in requested {
        crate::core::service::SkillId::new(id.clone())?;
        if roots.contains(id.as_str()) {
            removed_roots.insert(id.as_str());
            continue;
        }
        let requiring = roots
            .iter()
            .copied()
            .filter(|root| closure(&dependencies, &BTreeSet::from([*root])).contains(id.as_str()))
            .collect::<Vec<_>>();
        if !requiring.is_empty() {
            return Err(ServiceError::InvalidOperation(format!(
                "Global skill '{id}' is required by retained root(s): {}",
                requiring.join(", ")
            )));
        }
        if locked.contains(id.as_str()) {
            cleanup.insert(id.as_str());
        } else {
            unchanged.insert(id.as_str());
        }
    }
    let remaining_roots: BTreeSet<&str> = roots.difference(&removed_roots).copied().collect();
    let retained = closure(&dependencies, &remaining_roots);
    let mut removed = closure(&dependencies, &removed_roots);
    removed.extend(cleanup);
    let delete: BTreeSet<&str> = removed.difference(&retained).copied().collect();
    let kept: BTreeSet<&str> = removed.intersection(&retained).copied().collect();
    Ok(GlobalRemovalPlan {
        remove_roots: removed_roots.into_iter().map(str::to_string).collect(),
        remove_lock_entries: delete.iter().map(|id| (*id).to_string()).collect(),
        delete_files: delete.into_iter().map(str::to_string).collect(),
        retained_files: kept.into_iter().map(str::to_string).collect(),
        unchanged: unchanged.into_iter().map(str::to_string).collect(),
        remaining_roots: remaining_roots.into_iter().map(str::to_string).collect(),
    })
}

fn closure<'a>(
    dependencies: &BTreeMap<&'a str, Vec<&'a str>>,
    roots: &BTreeSet<&'a str>,
) -> BTreeSet<&'a str> {
    let mut reached = BTreeSet::new();
    let mut pending: VecDeque<_> = roots.iter().copied().collect();
    while let Some(id) = pending.pop_front() {
        if reached.insert(id) {
            if let Some(children) = dependencies.get(id) {
                pending.extend(children.iter().copied());
            }
        }
    }
    reached
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::lock::GlobalLockedSkillEntry;
    use crate::core::origin::{Origin, Resolved};
    use chrono::Utc;

    fn entry(id: &str, dependencies: &[&str]) -> GlobalLockedSkillEntry {
        GlobalLockedSkillEntry {
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
            installed_at: Utc::now(),
            last_checked_at: None,
            last_updated_at: None,
        }
    }

    #[test]
    fn detaching_one_root_retains_shared_reachable_entries() {
        let mut lock = GlobalSkillsLock::new_empty();
        lock.covered_roots = vec!["alpha".to_string(), "beta".to_string()];
        lock.skills = vec![
            entry("alpha", &["shared"]),
            entry("beta", &["shared"]),
            entry("shared", &[]),
        ];
        let plan = plan_removal(&lock, &["alpha".to_string()]).unwrap();
        assert_eq!(plan.remove_lock_entries, vec!["alpha"]);
        assert_eq!(plan.retained_files, vec!["shared"]);
        assert_eq!(plan.remaining_roots, vec!["beta"]);
    }

    #[test]
    fn required_only_entry_is_rejected_and_cycles_terminate() {
        let mut lock = GlobalSkillsLock::new_empty();
        lock.covered_roots = vec!["app".to_string()];
        lock.skills = vec![entry("app", &["child"]), entry("child", &["app"])];
        let error = plan_removal(&lock, &["child".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("required by retained root(s): app"));
        let plan = plan_removal(&lock, &["app".to_string()]).unwrap();
        assert_eq!(plan.delete_files, vec!["app", "child"]);
    }

    #[test]
    fn stale_orphan_is_cleaned_and_absent_or_invalid_ids_are_reported() {
        let mut lock = GlobalSkillsLock::new_empty();
        lock.skills = vec![entry("orphan", &[])];
        let plan = plan_removal(&lock, &["orphan".to_string(), "absent".to_string()]).unwrap();
        assert_eq!(plan.remove_lock_entries, vec!["orphan"]);
        assert_eq!(plan.unchanged, vec!["absent"]);
        assert!(plan_removal(&lock, &["../escape".to_string()]).is_err());
    }
}

#![allow(clippy::unwrap_used)]

use fastskill_core::core::lock::{
    ProjectLockedBundleEntry, ProjectLockedBundleMember, ProjectLockedPersonalOverride,
    ProjectLockedSkillEntry, ProjectSkillsLock,
};
use fastskill_core::core::manifest::{DependenciesSection, DependencySpec, SkillProjectToml};
use fastskill_core::core::origin::{Origin, Resolved};
use fastskill_core::core::ownership::ProjectOwnership;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn entry(id: &str, depth: u32, dependencies: &[&str]) -> ProjectLockedSkillEntry {
    ProjectLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin: Origin::Local {
            path: PathBuf::from(id),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: None,
        },
        dependencies: dependencies.iter().map(|value| value.to_string()).collect(),
        groups: Vec::new(),
        depth,
        parent_skill: None,
        required_by: Vec::new(),
    }
}

fn project(roots: &[&str]) -> SkillProjectToml {
    SkillProjectToml {
        schema_version: None,
        metadata: None,
        dependencies: Some(DependenciesSection {
            dependencies: roots
                .iter()
                .map(|id| {
                    (
                        (*id).to_string(),
                        DependencySpec::Version("1.0.0".to_string()),
                    )
                })
                .collect::<HashMap<_, _>>(),
        }),
        tool: None,
    }
}

#[test]
fn removing_one_root_keeps_a_dependency_reachable_from_another_root() {
    let manifest = project(&["alpha", "beta"]);
    let mut lock = ProjectSkillsLock::new_empty();
    lock.skills = vec![
        entry("alpha", 0, &["shared"]),
        entry("beta", 0, &["shared"]),
        entry("shared", 1, &[]),
    ];
    let plan = ProjectOwnership::new(&manifest, &lock)
        .plan_removal(&["alpha".to_string()])
        .unwrap();
    assert_eq!(plan.remove_lock_entries, vec!["alpha"]);
    assert!(plan.retained_files.contains(&"shared".to_string()));
    assert!(!plan.delete_files.contains(&"shared".to_string()));
}

#[test]
fn removing_a_required_dependency_names_the_retained_roots() {
    let manifest = project(&["alpha", "beta"]);
    let mut lock = ProjectSkillsLock::new_empty();
    lock.skills = vec![
        entry("alpha", 0, &["shared"]),
        entry("beta", 0, &["shared"]),
        entry("shared", 1, &[]),
    ];
    let error = ProjectOwnership::new(&manifest, &lock)
        .plan_removal(&["shared".to_string()])
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("alpha"), "{message}");
    assert!(message.contains("beta"), "{message}");
}

#[test]
fn removing_a_direct_bundle_member_drops_direct_state_and_retains_files() {
    let manifest = project(&["shared"]);
    let mut lock = ProjectSkillsLock::new_empty();
    lock.skills = vec![entry("shared", 0, &[])];
    lock.bundles = vec![ProjectLockedBundleEntry {
        id: "team".to_string(),
        version: "1.0.0".to_string(),
        artifact: "team.zip".to_string(),
        digest: "release".to_string(),
        members: vec![ProjectLockedBundleMember {
            id: "shared".to_string(),
            digest: "member".to_string(),
            overridable: false,
        }],
    }];
    let plan = ProjectOwnership::new(&manifest, &lock)
        .plan_removal(&["shared".to_string()])
        .unwrap();
    assert_eq!(plan.remove_manifest_dependencies, vec!["shared"]);
    assert_eq!(plan.remove_lock_entries, vec!["shared"]);
    assert_eq!(plan.retained_files, vec!["shared"]);
    assert!(plan.delete_files.is_empty());
}

#[test]
fn missing_direct_root_can_be_removed_and_absent_id_is_unchanged() {
    let manifest = project(&["missing"]);
    let mut lock = ProjectSkillsLock::new_empty();
    lock.skills = vec![entry("missing", 0, &[])];
    let plan = ProjectOwnership::new(&manifest, &lock)
        .plan_removal(&["missing".to_string(), "absent".to_string()])
        .unwrap();
    assert_eq!(plan.remove_manifest_dependencies, vec!["missing"]);
    assert_eq!(plan.remove_lock_entries, vec!["missing"]);
    assert_eq!(plan.delete_files, vec!["missing"]);
    assert_eq!(plan.unchanged, vec!["absent"]);
}

#[test]
fn ordinary_remove_rejects_a_personal_bundle_override_even_if_its_owner_is_stale() {
    let manifest = project(&[]);
    let mut lock = ProjectSkillsLock::new_empty();
    lock.overrides.push(ProjectLockedPersonalOverride {
        id: "personal".to_string(),
        origin: "./personal".to_string(),
        digest: "digest".to_string(),
    });

    let error = ProjectOwnership::new(&manifest, &lock)
        .plan_removal(&["personal".to_string()])
        .unwrap_err()
        .to_string();

    assert!(error.contains("personal bundle override"), "{error}");
    assert!(error.contains("reset"), "{error}");
}

#[test]
fn removing_a_direct_root_keeps_and_demotes_it_when_another_root_requires_it() {
    let manifest = project(&["alpha", "beta"]);
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = vec!["alpha".to_string(), "beta".to_string()];
    lock.skills = vec![entry("alpha", 0, &[]), entry("beta", 0, &["alpha"])];
    let plan = ProjectOwnership::new(&manifest, &lock)
        .plan_removal(&["alpha".to_string()])
        .unwrap();
    assert!(plan.remove_lock_entries.is_empty());
    assert_eq!(plan.remaining_individual_roots, vec!["beta"]);
    fastskill_core::core::ownership::normalize_lock_ownership(
        &mut lock,
        &plan.remaining_individual_roots,
    );
    let alpha = lock
        .skills
        .iter()
        .find(|entry| entry.id == "alpha")
        .unwrap();
    assert_eq!(alpha.depth, 1);
    assert_eq!(alpha.required_by, vec!["beta"]);
    assert_eq!(lock.covered_roots, vec!["beta"]);
}

#[test]
fn removal_service_prunes_a_root_closure_and_persists_normalized_state() {
    let project_root = TempDir::new().unwrap();
    let skills = project_root.path().join("skills");
    fs::create_dir_all(&skills).unwrap();
    let manifest = project(&["app"]);
    manifest
        .save_to_file(&project_root.path().join("skill-project.toml"))
        .unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = vec!["app".to_string()];
    lock.skills = vec![entry("app", 0, &["child"]), entry("child", 1, &[])];
    for entry in &mut lock.skills {
        let directory = skills.join(&entry.id);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("SKILL.md"), &entry.id).unwrap();
        entry.resolved.checksum =
            Some(fastskill_core::core::project_removal::managed_tree_digest(&directory).unwrap());
    }
    lock.save_to_file(&project_root.path().join("skills.lock"))
        .unwrap();

    let plan = fastskill_core::core::project_removal::ProjectRemovalService::new(
        project_root.path(),
        &skills,
    )
    .remove(&["app".to_string()])
    .unwrap();

    assert_eq!(plan.delete_files, vec!["app", "child"]);
    assert!(!skills.join("app").exists());
    assert!(!skills.join("child").exists());
    let lock = ProjectSkillsLock::load_from_file(&project_root.path().join("skills.lock")).unwrap();
    assert!(lock.skills.is_empty());
    assert!(lock.covered_roots.is_empty());
}

#[test]
fn invalid_removal_does_not_poison_state_and_interruption_is_detected() {
    let project_root = TempDir::new().unwrap();
    fs::create_dir_all(project_root.path().join("skills")).unwrap();
    project(&[])
        .save_to_file(&project_root.path().join("skill-project.toml"))
        .unwrap();
    let service = fastskill_core::core::project_removal::ProjectRemovalService::new(
        project_root.path(),
        project_root.path().join("skills"),
    );
    assert!(service.remove(&["../escape".to_string()]).is_err());
    assert!(!project_root
        .path()
        .join(".fastskill/recovery-required")
        .exists());
    drop(
        fastskill_core::core::state_guard::StateMutationGuard::acquire(
            project_root.path(),
            "interrupted test",
        )
        .unwrap(),
    );
    let error = service
        .remove(&["absent".to_string()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("was interrupted"), "{error}");
}

#[test]
fn removal_without_a_lock_clears_a_missing_declaration_and_absent_is_unchanged() {
    let project_root = TempDir::new().unwrap();
    fs::create_dir_all(project_root.path().join("skills")).unwrap();
    project(&["missing"])
        .save_to_file(&project_root.path().join("skill-project.toml"))
        .unwrap();
    let service = fastskill_core::core::project_removal::ProjectRemovalService::new(
        project_root.path(),
        project_root.path().join("skills"),
    );
    let plan = service
        .remove(&["missing".to_string(), "absent".to_string()])
        .unwrap();
    assert_eq!(plan.remove_manifest_dependencies, vec!["missing"]);
    assert_eq!(plan.unchanged, vec!["absent"]);
    assert!(
        ProjectSkillsLock::load_from_file(&project_root.path().join("skills.lock"))
            .unwrap()
            .skills
            .is_empty()
    );
}

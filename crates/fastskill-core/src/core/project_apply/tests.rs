#![allow(clippy::unwrap_used)]

use super::*;
use crate::core::origin::Resolved;
use std::path::PathBuf;
use tempfile::TempDir;

fn locked(id: &str, dependencies: &[&str], depth: u32) -> ProjectLockedSkillEntry {
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
        depth,
        parent_skill: None,
        required_by: Vec::new(),
    }
}

fn empty_plan(expected_skills: Vec<ProjectLockedSkillEntry>) -> ProjectApplyPlan {
    ProjectApplyPlan {
        candidates: Vec::new(),
        base_lock: None,
        covered_roots: Vec::new(),
        manifest_updates: Vec::new(),
        removals: Vec::new(),
        write_lock: false,
        expected_skills,
        expected_manifest: Vec::new(),
    }
}

fn write_source(root: &Path, id: &str) -> PathBuf {
    let source = root.join(id);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        format!("---\nname: {id}\nversion: \"1.0.0\"\ndescription: fixture\n---\n"),
    )
    .unwrap();
    source
}

async fn prepared_root_plan(
    service: &FastSkillService,
    id: &str,
    path: PathBuf,
) -> ProjectApplyPlan {
    let origin = Origin::Local {
        path,
        editable: false,
    };
    let prepared = service.prepare_add(origin.clone(), Some(id)).await.unwrap();
    ProjectApplyPlan {
        candidates: vec![ProjectApplyCandidate {
            prepared,
            origin: origin.clone(),
            groups: Vec::new(),
            depth: 0,
            required_by: BTreeSet::new(),
            dependencies: Vec::new(),
        }],
        base_lock: None,
        covered_roots: vec![id.to_string()],
        manifest_updates: vec![SkillEntry {
            id: id.to_string(),
            origin,
            groups: Vec::new(),
        }],
        removals: Vec::new(),
        write_lock: true,
        expected_skills: Vec::new(),
        expected_manifest: vec![ExpectedManifestSkill {
            id: id.to_string(),
            entry: None,
        }],
    }
}

#[test]
fn retained_graph_recomputes_all_owners_and_minimum_depth() {
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = vec!["alpha".into(), "beta".into()];
    lock.skills = vec![
        locked("alpha", &["mid"], 0),
        locked("mid", &["shared"], 1),
        locked("beta", &["shared"], 0),
        locked("shared", &[], 1),
    ];

    let (retained, removed) = retain_unaffected_roots(lock, &["beta".into()]);

    assert_eq!(removed, vec!["beta"]);
    assert_eq!(retained.covered_roots, vec!["alpha"]);
    let shared = retained
        .skills
        .iter()
        .find(|entry| entry.id == "shared")
        .unwrap();
    assert_eq!(shared.required_by, vec!["mid"]);
    assert_eq!(shared.parent_skill.as_deref(), Some("mid"));
    assert_eq!(shared.depth, 2);
}

#[test]
fn manifest_validation_rejects_same_root_drift_and_allows_unrelated_additions() {
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("skill-project.toml"),
        "[dependencies]\nother = { origin = { type = \"local\", path = \"other\" } }\n",
    )
    .unwrap();
    let absent = ExpectedManifestSkill {
        id: "demo".to_string(),
        entry: None,
    };
    validate_expected_manifest(project.path(), std::slice::from_ref(&absent)).unwrap();

    std::fs::write(
        project.path().join("skill-project.toml"),
        "[dependencies]\ndemo = { origin = { type = \"local\", path = \"source-two\" } }\n",
    )
    .unwrap();
    let error = validate_expected_manifest(project.path(), &[absent]).unwrap_err();
    assert!(error.to_string().contains("changed after planning"));
}

#[test]
fn disappearing_lock_is_stale_and_non_ordinary_owners_retain_content() {
    let plan = empty_plan(vec![locked("demo", &[], 0)]);
    assert!(validate_expected_lock_state(None, &plan)
        .unwrap_err()
        .to_string()
        .contains("changed after planning"));

    let mut lock = ProjectSkillsLock::new_empty();
    lock.bundles
        .push(crate::core::lock::ProjectLockedBundleEntry {
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            artifact: "team.fskill".to_string(),
            digest: "bundle".to_string(),
            members: vec![crate::core::lock::ProjectLockedBundleMember {
                id: "demo".to_string(),
                digest: "content".to_string(),
                overridable: false,
            }],
        });
    assert!(has_non_ordinary_owner(&lock, "demo"));
    assert!(!should_remove_managed_path(Some(&lock), "demo"));
    assert!(should_remove_managed_path(Some(&lock), "other"));
}

#[tokio::test]
async fn apply_rejects_a_lock_removed_after_planning_without_mutation() {
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("skill-project.toml"),
        "[dependencies]\n",
    )
    .unwrap();
    let storage = project.path().join("skills");
    let mut service = FastSkillService::new(crate::ServiceConfig {
        skill_storage_path: storage.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let manifest_before = std::fs::read(project.path().join("skill-project.toml")).unwrap();

    let error = service
        .apply_project_plan(project.path(), empty_plan(vec![locked("demo", &[], 0)]))
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("skills.lock changed after planning"));
    assert_eq!(
        std::fs::read(project.path().join("skill-project.toml")).unwrap(),
        manifest_before
    );
    assert!(!project.path().join("skills.lock").exists());
    assert!(!storage.join("demo").exists());
}

#[tokio::test]
async fn independently_prepared_roots_merge_after_the_writer_lease() {
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let sources = project.path().join("sources");
    let alpha = write_source(&sources, "alpha");
    let beta = write_source(&sources, "beta");
    let storage = project.path().join("skills");
    let mut service = FastSkillService::new(crate::ServiceConfig {
        skill_storage_path: storage.clone(),
        skill_cache_root: Some(project.path().join("cache")),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();

    // Both plans observe the same empty state, like two concurrent
    // commands that finish resolution before either acquires the writer.
    let alpha_plan = prepared_root_plan(&service, "alpha", alpha).await;
    let beta_plan = prepared_root_plan(&service, "beta", beta).await;
    service
        .apply_project_plan(project.path(), beta_plan)
        .await
        .unwrap();
    service
        .apply_project_plan(project.path(), alpha_plan)
        .await
        .unwrap();

    let manifest =
        SkillProjectToml::load_from_file(&project.path().join("skill-project.toml")).unwrap();
    let entries = manifest.to_skill_entries(project.path()).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["alpha", "beta"])
    );
    let lock = ProjectSkillsLock::load_from_file(&project.path().join("skills.lock")).unwrap();
    assert_eq!(
        lock.skills
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["alpha", "beta"])
    );
    assert_eq!(
        lock.covered_roots.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from(["alpha".to_string(), "beta".to_string()])
    );
    assert!(storage.join("alpha/SKILL.md").exists());
    assert!(storage.join("beta/SKILL.md").exists());
}

#[test]
fn bundle_only_installed_content_is_verified_before_replacement() {
    let storage = TempDir::new().unwrap();
    let installed = storage.path().join("demo");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("SKILL.md"), "original").unwrap();
    let digest = crate::core::install::content_digest(&installed).unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    lock.bundles
        .push(crate::core::lock::ProjectLockedBundleEntry {
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            artifact: "team.fskill".to_string(),
            digest: "bundle".to_string(),
            members: vec![crate::core::lock::ProjectLockedBundleMember {
                id: "demo".to_string(),
                digest,
                overridable: false,
            }],
        });
    validate_installed_content(&["demo".to_string()], &lock, storage.path()).unwrap();

    std::fs::write(installed.join("SKILL.md"), "local edit").unwrap();
    let error =
        validate_installed_content(&["demo".to_string()], &lock, storage.path()).unwrap_err();
    assert!(error.to_string().contains("was modified"));
}

#[test]
fn immutable_ordinary_content_without_a_digest_cannot_be_replaced() {
    let storage = TempDir::new().unwrap();
    let installed = storage.path().join("demo");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("SKILL.md"), "existing").unwrap();
    let mut entry = locked("demo", &[], 0);
    entry.resolved.checksum = None;
    let mut lock = ProjectSkillsLock::new_empty();
    lock.skills.push(entry);

    let error =
        validate_installed_content(&["demo".to_string()], &lock, storage.path()).unwrap_err();

    assert!(error.to_string().contains("no recorded integrity digest"));
}

#[test]
fn managed_path_removal_handles_absent_file_and_directory_destinations() {
    let root = TempDir::new().unwrap();
    remove_managed_path(&root.path().join("absent")).unwrap();

    let file = root.path().join("file");
    std::fs::write(&file, "content").unwrap();
    remove_managed_path(&file).unwrap();
    assert!(!file.exists());

    let directory = root.path().join("directory");
    std::fs::create_dir_all(directory.join("nested")).unwrap();
    remove_managed_path(&directory).unwrap();
    assert!(!directory.exists());
}

#[test]
fn manifest_updates_create_dependencies_and_validation_normalizes_groups() {
    let project = TempDir::new().unwrap();
    std::fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n",
    )
    .unwrap();
    let origin = Origin::Local {
        path: project.path().join("source"),
        editable: false,
    };
    apply_manifest_updates(
        project.path(),
        &[SkillEntry {
            id: "demo".to_string(),
            origin: origin.clone(),
            groups: vec!["dev".to_string(), "default".to_string()],
        }],
    )
    .unwrap();
    validate_expected_manifest(
        project.path(),
        &[ExpectedManifestSkill {
            id: "demo".to_string(),
            entry: Some(SkillEntry {
                id: "demo".to_string(),
                origin,
                groups: vec!["default".to_string(), "dev".to_string(), "dev".to_string()],
            }),
        }],
    )
    .unwrap();
}

#[test]
fn installed_content_rejects_disagreeing_bundle_owner_digests() {
    let storage = TempDir::new().unwrap();
    std::fs::create_dir_all(storage.path().join("demo")).unwrap();
    std::fs::write(storage.path().join("demo/SKILL.md"), "content").unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    for (id, digest) in [("one", "first"), ("two", "second")] {
        lock.bundles
            .push(crate::core::lock::ProjectLockedBundleEntry {
                id: id.to_string(),
                version: "1.0.0".to_string(),
                artifact: format!("{id}.fskill"),
                digest: format!("{id}-bundle"),
                members: vec![crate::core::lock::ProjectLockedBundleMember {
                    id: "demo".to_string(),
                    digest: digest.to_string(),
                    overridable: false,
                }],
            });
    }
    assert!(
        validate_installed_content(&["demo".to_string()], &lock, storage.path())
            .unwrap_err()
            .to_string()
            .contains("conflicting recorded ownership")
    );
}

#[tokio::test]
async fn active_override_must_be_permitted_and_match_the_candidate() {
    let root = TempDir::new().unwrap();
    let source = write_source(root.path(), "demo");
    let mut service = FastSkillService::new(crate::ServiceConfig {
        skill_storage_path: root.path().join("skills"),
        skill_cache_root: Some(root.path().join("cache")),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let origin = Origin::Local {
        path: source,
        editable: false,
    };
    let prepared = service
        .prepare_add(origin.clone(), Some("demo"))
        .await
        .unwrap();
    let candidate = ProjectApplyCandidate {
        prepared,
        origin,
        groups: Vec::new(),
        depth: 0,
        required_by: BTreeSet::new(),
        dependencies: Vec::new(),
    };
    let mut lock = ProjectSkillsLock::new_empty();
    lock.overrides
        .push(crate::core::lock::ProjectLockedPersonalOverride {
            id: "demo".to_string(),
            origin: "override".to_string(),
            digest: "wrong".to_string(),
        });
    lock.bundles
        .push(crate::core::lock::ProjectLockedBundleEntry {
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            artifact: "team.fskill".to_string(),
            digest: "bundle".to_string(),
            members: vec![crate::core::lock::ProjectLockedBundleMember {
                id: "demo".to_string(),
                digest: "packaged".to_string(),
                overridable: false,
            }],
        });
    assert!(
        validate_bundle_ownership(std::slice::from_ref(&candidate), &lock)
            .unwrap_err()
            .to_string()
            .contains("not permitted")
    );
    lock.bundles[0].members[0].overridable = true;
    assert!(
        validate_bundle_ownership(std::slice::from_ref(&candidate), &lock)
            .unwrap_err()
            .to_string()
            .contains("conflicts")
    );

    lock.skills.push(locked("demo", &[], 0));
    let plan = ProjectApplyPlan {
        candidates: vec![candidate],
        base_lock: None,
        covered_roots: vec!["demo".to_string()],
        manifest_updates: Vec::new(),
        removals: Vec::new(),
        write_lock: true,
        expected_skills: Vec::new(),
        expected_manifest: Vec::new(),
    };
    assert!(validate_expected_skills(&lock, &plan)
        .unwrap_err()
        .to_string()
        .contains("changed after planning"));
}

#[test]
fn installed_override_and_editable_content_follow_effective_ownership() {
    let storage = TempDir::new().unwrap();
    let installed = storage.path().join("demo");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("SKILL.md"), "personal").unwrap();
    let digest = crate::core::install::content_digest(&installed).unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    lock.overrides
        .push(crate::core::lock::ProjectLockedPersonalOverride {
            id: "demo".to_string(),
            origin: "override".to_string(),
            digest: digest.clone(),
        });
    lock.bundles
        .push(crate::core::lock::ProjectLockedBundleEntry {
            id: "team".to_string(),
            version: "1.0.0".to_string(),
            artifact: "team.fskill".to_string(),
            digest: "bundle".to_string(),
            members: vec![crate::core::lock::ProjectLockedBundleMember {
                id: "demo".to_string(),
                digest: "packaged".to_string(),
                overridable: false,
            }],
        });
    assert!(
        validate_installed_content(&["demo".to_string()], &lock, storage.path())
            .unwrap_err()
            .to_string()
            .contains("no longer permitted")
    );

    lock.bundles[0].members[0].overridable = true;
    validate_installed_content(&["demo".to_string()], &lock, storage.path()).unwrap();
    std::fs::write(installed.join("SKILL.md"), "edited").unwrap();
    assert!(
        validate_installed_content(&["demo".to_string()], &lock, storage.path())
            .unwrap_err()
            .to_string()
            .contains("was modified")
    );

    lock.overrides.clear();
    lock.bundles.clear();
    let mut editable = locked("demo", &[], 0);
    editable.origin = Origin::Local {
        path: "demo".into(),
        editable: true,
    };
    editable.resolved.checksum = None;
    lock.skills.push(editable);
    validate_installed_content(&["demo".to_string()], &lock, storage.path()).unwrap();
}

#[test]
fn expected_lock_validation_rejects_changed_and_unexpected_same_id_entries() {
    let expected = locked("demo", &[], 0);
    let mut current = ProjectSkillsLock::new_empty();
    let mut changed = expected.clone();
    changed.resolved.version = "2.0.0".to_string();
    current.skills.push(changed);
    let plan = empty_plan(vec![expected]);
    assert!(validate_expected_skills(&current, &plan)
        .unwrap_err()
        .to_string()
        .contains("changed after planning"));
}

#[tokio::test]
async fn public_validation_reports_malformed_authoritative_state() {
    let project = TempDir::new().unwrap();
    let storage = project.path().join("skills");
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::write(
        project.path().join("skill-project.toml"),
        "[dependencies]\n",
    )
    .unwrap();
    std::fs::write(project.path().join("skills.lock"), "not = [valid").unwrap();
    let mut service = FastSkillService::new(crate::ServiceConfig {
        skill_storage_path: storage,
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();

    let error = service
        .validate_project_plan(project.path(), &empty_plan(Vec::new()))
        .unwrap_err();
    assert!(error.to_string().contains("Failed to load skills.lock"));
}

#[tokio::test]
async fn registry_snapshot_restore_handles_present_and_absent_entries() {
    let storage = TempDir::new().unwrap();
    let mut service = FastSkillService::new(crate::ServiceConfig {
        skill_storage_path: storage.path().to_path_buf(),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let id = SkillId::new("demo".to_string()).unwrap();
    let definition = SkillDefinition::new(
        id.clone(),
        "Demo".to_string(),
        "fixture".to_string(),
        "1.0.0".to_string(),
        Origin::Local {
            path: "demo".into(),
            editable: false,
        },
    );

    restore_registry(&service, &[(id.clone(), Some(definition.clone()))])
        .await
        .unwrap();
    assert!(service
        .skill_manager()
        .get_skill(&id)
        .await
        .unwrap()
        .is_some());
    restore_registry(&service, &[(id.clone(), None)])
        .await
        .unwrap();
    assert!(service
        .skill_manager()
        .get_skill(&id)
        .await
        .unwrap()
        .is_none());
}

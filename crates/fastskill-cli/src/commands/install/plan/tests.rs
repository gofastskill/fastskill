use super::*;
use fastskill_core::ServiceConfig;
use std::path::PathBuf;

fn root(id: &str, groups: &[&str]) -> SkillEntry {
    SkillEntry {
        id: id.to_string(),
        origin: Origin::Local {
            path: PathBuf::from(id),
            editable: false,
        },
        groups: groups.iter().map(|group| (*group).to_string()).collect(),
    }
}

#[test]
fn ungrouped_roots_are_in_default_group() {
    let selected = select_roots(
        vec![root("main", &[]), root("tool", &["dev"])],
        Some(&["default".to_string()]),
        None,
    )
    .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, "main");
}

#[test]
fn group_selectors_are_exclusive_and_known() {
    let roots = vec![root("main", &[]), root("tool", &["dev"])];
    assert!(select_roots(
        roots.clone(),
        Some(&["dev".to_string()]),
        Some(&["default".to_string()])
    )
    .is_err());
    assert!(select_roots(roots, Some(&["missing".to_string()]), None).is_err());
}

fn locked_entry(origin: Origin) -> fastskill_core::core::lock::ProjectLockedSkillEntry {
    fastskill_core::core::lock::ProjectLockedSkillEntry {
        id: "demo".to_string(),
        name: "demo".to_string(),
        origin,
        resolved: fastskill_core::core::origin::Resolved {
            version: "1.0.0".to_string(),
            commit_hash: Some("a".repeat(40)),
            checksum: Some("digest".to_string()),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        depth: 0,
        parent_skill: None,
        required_by: Vec::new(),
    }
}

#[test]
fn recorded_root_validation_rejects_missing_changed_and_incomplete_facts() {
    let project = tempfile::tempdir().unwrap();
    let root_entry = SkillEntry {
        id: "demo".to_string(),
        origin: Origin::Local {
            path: project.path().join("demo"),
            editable: false,
        },
        groups: Vec::new(),
    };
    let empty = ProjectSkillsLock::new_empty();
    assert!(
        validate_recorded_roots(&empty, std::slice::from_ref(&root_entry), project.path())
            .unwrap_err()
            .to_string()
            .contains("missing")
    );

    let mut lock = ProjectSkillsLock::new_empty();
    lock.skills.push(locked_entry(Origin::Local {
        path: "other".into(),
        editable: false,
    }));
    assert!(
        validate_recorded_roots(&lock, std::slice::from_ref(&root_entry), project.path())
            .unwrap_err()
            .to_string()
            .contains("differs")
    );

    lock.skills[0].origin = Origin::Local {
        path: "demo".into(),
        editable: false,
    };
    lock.skills[0].resolved.checksum = None;
    assert!(
        validate_recorded_roots(&lock, std::slice::from_ref(&root_entry), project.path())
            .unwrap_err()
            .to_string()
            .contains("integrity evidence")
    );

    let git_root = SkillEntry {
        id: "demo".to_string(),
        origin: Origin::Git {
            url: "https://example.test/repo.git".to_string(),
            r#ref: fastskill_core::core::origin::GitRef::Branch("main".to_string()),
            subdir: None,
        },
        groups: Vec::new(),
    };
    lock.skills[0] = locked_entry(git_root.origin.clone());
    lock.skills[0].resolved.commit_hash = None;
    assert!(validate_recorded_roots(&lock, &[git_root], Path::new("."))
        .unwrap_err()
        .to_string()
        .contains("no commit"));
}

fn write_skill(path: &Path, id: &str, version: &str, body: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {id}\nversion: {version}\ndescription: test\n---\n{body}\n"),
    )
    .unwrap();
}

#[tokio::test]
async fn lock_write_failure_restores_files_registry_and_marker() {
    let project = tempfile::tempdir().unwrap();
    let storage = project.path().join("skills");
    write_skill(&storage.join("demo"), "demo", "1.0.0", "old");
    let source = project.path().join("source");
    write_skill(&source, "demo", "2.0.0", "new");
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: storage.clone(),
        skill_cache_root: Some(project.path().join("cache")),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let lock_path = project.path().join("skills.lock");
    let prepared = prepare(
        &service,
        &lock_path,
        project.path(),
        InstallSelection {
            roots: vec![SkillEntry {
                id: "demo".to_string(),
                origin: Origin::Local {
                    path: source,
                    editable: false,
                },
                groups: vec![],
            }],
            only: None,
            without: None,
            max_levels: 5,
            skip_transitive: false,
            strict: false,
            offline: false,
        },
    )
    .await
    .unwrap();
    std::fs::create_dir(&lock_path).unwrap();
    assert!(
        apply_prepared(&service, &lock_path, project.path(), prepared)
            .await
            .is_err()
    );
    let content = std::fs::read_to_string(storage.join("demo/SKILL.md")).unwrap();
    assert!(content.contains("1.0.0"));
    let registered = service
        .skill_manager()
        .get_skill(&fastskill_core::SkillId::new("demo".to_string()).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(registered.version, "1.0.0");
    assert!(!project.path().join(".fastskill/recovery-required").exists());
}

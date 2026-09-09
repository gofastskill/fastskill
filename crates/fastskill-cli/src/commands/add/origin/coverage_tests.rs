#![allow(clippy::unwrap_used, clippy::await_holding_lock)]

use super::tests::{args, global_fixture, write_skill};
use super::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct WorkingDirectory(PathBuf);

impl WorkingDirectory {
    fn enter(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(previous)
    }
}

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[tokio::test]
async fn repository_selectors_preserve_constraints_and_require_configured_names() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (root, _xdg, _cache, service, _) = global_fixture().await;
    let _cwd = WorkingDirectory::enter(root.path());
    let manifest = root.path().join("skill-project.toml");
    fs::write(&manifest, "[dependencies]\n").unwrap();
    let reference = SkillSource::SkillId("team/demo@^1.2.0".to_string());
    let error = build_explicit_origin(&reference, &args()).unwrap_err();
    assert!(error
        .to_string()
        .contains("No default repository configured"));
    let invalid = SkillSource::SkillId("team/demo@not-semver".to_string());
    assert!(build_explicit_origin(&invalid, &args())
        .unwrap_err()
        .to_string()
        .contains("Invalid version constraint"));

    fs::write(
        &manifest,
        "[dependencies]\n\
         [[tool.fastskill.repositories]]\nname = \"default\"\ntype = \"local\"\npath = \"catalog\"\npriority = 0\n\
         [[tool.fastskill.repositories]]\nname = \"private\"\ntype = \"local\"\npath = \"private\"\npriority = 1\n",
    )
    .unwrap();
    let constraint = Some(VersionConstraint::parse("^1.2.0").unwrap());
    let expected = |repo: &str| Origin::Repository {
        repo: repo.to_string(),
        skill: "team/demo".to_string(),
        version: constraint.clone(),
    };
    assert_eq!(
        build_explicit_origin(&reference, &args()).unwrap(),
        expected("default")
    );
    let mut selected = args();
    selected.repository = Some("private".to_string());
    assert_eq!(
        build_explicit_origin(&reference, &selected).unwrap(),
        expected("private")
    );
    selected.repository = Some("missing".to_string());
    assert!(build_explicit_origin(&reference, &selected)
        .unwrap_err()
        .to_string()
        .contains("Repository 'missing' is not configured"));

    let manager = RepositoryManager::from_definitions(
        crate::config::load_repositories_from_project().unwrap(),
    );
    let service = service.with_repository_manager(Arc::new(manager));
    selected.source_type = None;
    selected.source = "team/demo@1.2.0".to_string();
    assert!(build_origin(&service, &reference, &selected)
        .await
        .unwrap_err()
        .to_string()
        .contains("Repository 'missing' is not configured"));
    selected.repository = Some("private".to_string());
    assert_eq!(
        build_origin(&service, &reference, &selected).await.unwrap(),
        Origin::Repository {
            repo: "private".to_string(),
            skill: "team/demo".to_string(),
            version: Some(VersionConstraint::parse("1.2.0").unwrap()),
        }
    );
    assert!(!root.path().join("catalog").exists());
    assert!(!global_lock_path().unwrap().exists());
}

#[tokio::test]
async fn inferred_git_selectors_have_explicit_precedence_without_fetching() {
    let root = TempDir::new().unwrap();
    let service = FastSkillService::new(fastskill_core::ServiceConfig {
        skill_storage_path: root.path().join("skills"),
        ..Default::default()
    })
    .await
    .unwrap();
    let mut selected = args();
    selected.source_type = None;
    for (url, branch, tag, expected) in [
        (
            "https://example.com/team/demo.git",
            None,
            None,
            GitRef::Default,
        ),
        (
            "https://example.com/team/demo.git",
            None,
            Some("v1.2.0"),
            GitRef::Tag("v1.2.0".to_string()),
        ),
        (
            "https://github.com/team/demo/tree/release/skills/demo",
            None,
            Some("v1.2.0"),
            GitRef::Branch("release".to_string()),
        ),
        (
            "https://github.com/team/demo/tree/release/skills/demo",
            Some("develop"),
            Some("v1.2.0"),
            GitRef::Branch("develop".to_string()),
        ),
    ] {
        selected.source = url.to_string();
        selected.branch = branch.map(str::to_string);
        selected.tag = tag.map(str::to_string);
        let origin = build_origin(&service, &SkillSource::GitUrl(url.to_string()), &selected)
            .await
            .unwrap();
        let subdir = url.contains("/tree/").then(|| PathBuf::from("skills/demo"));
        assert_eq!(
            origin,
            Origin::Git {
                url: url.to_string(),
                r#ref: expected,
                subdir
            }
        );
    }
    selected.source = root.path().join("missing").display().to_string();
    let missing = SkillSource::Folder(PathBuf::from(&selected.source));
    assert!(build_origin(&service, &missing, &selected)
        .await
        .unwrap_err()
        .to_string()
        .contains("Failed to resolve absolute path"));
}

#[tokio::test]
async fn global_replacement_updates_one_lock_entry_and_repeat_is_unchanged() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (root, _xdg, _cache, service, storage) = global_fixture().await;
    let source = root.path().join("source");
    write_skill(&source, "demo", "1.0.0", "first");
    let mut selected = args();
    selected.json = true;
    selected.group = Some("dev".to_string());
    let (result, output) = crate::output::capture(add_global_skill(
        &service,
        &SkillSource::Folder(source.clone()),
        &selected,
    ))
    .await;
    result.unwrap();
    let output: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(output["outcome"], "changed");
    let lock_path = global_lock_path().unwrap();
    let lock = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(lock.skills[0].groups, ["dev"]);
    let initial_install = lock.skills[0].installed_at;

    write_skill(&source, "demo", "2.0.0", "second");
    add_global_skill(&service, &SkillSource::Folder(source.clone()), &selected)
        .await
        .unwrap();
    let lock = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(lock.skills.len(), 1);
    assert_eq!(lock.skills[0].resolved.version, "2.0.0");
    assert_eq!(lock.skills[0].installed_at, initial_install);
    assert_eq!(lock.skills[0].groups, ["dev"]);
    assert!(fs::read_to_string(storage.join("demo/SKILL.md"))
        .unwrap()
        .contains("second"));
    let previous_lock = fs::read(&lock_path).unwrap();
    let (result, output) = crate::output::capture(add_global_skill(
        &service,
        &SkillSource::Folder(source.clone()),
        &selected,
    ))
    .await;
    result.unwrap();
    let output: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(output["outcome"], "unchanged");
    assert_eq!(fs::read(&lock_path).unwrap(), previous_lock);

    let installed_before = fs::read(storage.join("demo/SKILL.md")).unwrap();
    selected.group = Some("production".to_string());
    let (result, output) = crate::output::capture(add_global_skill(
        &service,
        &SkillSource::Folder(source),
        &selected,
    ))
    .await;
    result.unwrap();
    let output: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(output["outcome"], "changed");
    assert_eq!(output["targets"][0]["changes"][0], "groups changed");
    let lock = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(lock.skills[0].groups, ["production"]);
    assert_eq!(
        fs::read(storage.join("demo/SKILL.md")).unwrap(),
        installed_before,
        "a group-only change must not rewrite installed content"
    );
}

#[tokio::test]
async fn global_add_rejects_incompatible_shared_content_required_by_a_retained_root() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (root, _xdg, _cache, service, storage) = global_fixture().await;
    let shared_v1 = root.path().join("shared-v1");
    let shared_v2 = root.path().join("shared-v2");
    let alpha = root.path().join("alpha");
    let beta = root.path().join("beta");
    write_skill(&shared_v1, "shared", "1.0.0", "shared one");
    write_skill(&shared_v2, "shared", "2.0.0", "shared two");
    write_skill(&alpha, "alpha", "1.0.0", "alpha");
    write_skill(&beta, "beta", "1.0.0", "beta");
    fs::write(
        alpha.join("skill-project.toml"),
        format!(
            "[dependencies.shared.origin]\ntype = \"local\"\npath = {:?}\n",
            shared_v1
        ),
    )
    .unwrap();
    fs::write(
        beta.join("skill-project.toml"),
        format!(
            "[dependencies.shared.origin]\ntype = \"local\"\npath = {:?}\n",
            shared_v2
        ),
    )
    .unwrap();

    add_global_skill(&service, &SkillSource::Folder(alpha), &args())
        .await
        .unwrap();
    let lock_path = global_lock_path().unwrap();
    let lock_before = fs::read(&lock_path).unwrap();
    let shared_before = fs::read(storage.join("shared/SKILL.md")).unwrap();

    let error = add_global_skill(&service, &SkillSource::Folder(beta), &args())
        .await
        .unwrap_err();

    assert!(error.to_string().contains("required by a retained root"));
    assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
    assert_eq!(
        fs::read(storage.join("shared/SKILL.md")).unwrap(),
        shared_before
    );
    assert!(!storage.join("beta").exists());
}

#[tokio::test]
async fn global_force_add_rejects_locally_modified_managed_content() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (root, _xdg, _cache, service, storage) = global_fixture().await;
    let source = root.path().join("source");
    write_skill(&source, "demo", "1.0.0", "original");
    add_global_skill(&service, &SkillSource::Folder(source.clone()), &args())
        .await
        .unwrap();
    let lock_path = global_lock_path().unwrap();
    let lock_before = fs::read(&lock_path).unwrap();
    let installed = storage.join("demo/SKILL.md");
    fs::write(&installed, "locally edited content\n").unwrap();
    write_skill(&source, "demo", "2.0.0", "replacement");
    let mut forced = args();
    forced.force = true;

    for dry_run in [true, false] {
        forced.dry_run = dry_run;
        let error = add_global_skill(&service, &SkillSource::Folder(source.clone()), &forced)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("was modified"));
        assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
        assert_eq!(
            fs::read_to_string(&installed).unwrap(),
            "locally edited content\n"
        );
    }
}

#[tokio::test]
async fn global_add_rejects_untracked_file_before_creating_lock_or_recovery_marker() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (root, _xdg, _cache, service, storage) = global_fixture().await;
    let source = root.path().join("source");
    write_skill(&source, "demo", "1.0.0", "candidate");
    fs::create_dir_all(&storage).unwrap();
    fs::write(storage.join("demo"), "user-owned file").unwrap();
    let error = add_global_skill(&service, &SkillSource::Folder(source), &args())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("contains untracked content"));
    assert_eq!(
        fs::read_to_string(storage.join("demo")).unwrap(),
        "user-owned file"
    );
    let lock_path = global_lock_path().unwrap();
    assert!(!lock_path.exists());
    assert!(!lock_path
        .parent()
        .unwrap()
        .join(".fastskill/recovery-required")
        .exists());
    assert!(!storage.join(".fastskill-recovery-required").exists());
}

#[tokio::test]
async fn global_add_and_preview_preserve_untracked_root_and_dependency_directories() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (root, _xdg, _cache, service, storage) = global_fixture().await;
    let source = root.path().join("source");
    let child = root.path().join("child");
    write_skill(&source, "demo", "1.0.0", "candidate");
    write_skill(&child, "child", "1.0.0", "dependency");
    fs::write(
        source.join("skill-project.toml"),
        "[dependencies]\nchild = { origin = { type = \"local\", path = \"../child\" } }\n",
    )
    .unwrap();
    for id in ["demo", "child"] {
        let destination = storage.join(id);
        write_skill(&destination, id, "0.1.0", "personal skill");
        fs::write(destination.join("USER.md"), "user-owned notes\n").unwrap();
        let original = fs::read(destination.join("SKILL.md")).unwrap();
        for dry_run in [true, false] {
            for force in [false, true] {
                let mut selected = args();
                selected.dry_run = dry_run;
                selected.force = force;
                let error =
                    add_global_skill(&service, &SkillSource::Folder(source.clone()), &selected)
                        .await
                        .unwrap_err();
                assert!(error.to_string().contains("contains untracked content"));
                assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), original);
                assert_eq!(
                    fs::read_to_string(destination.join("USER.md")).unwrap(),
                    "user-owned notes\n"
                );
                assert!(!global_lock_path().unwrap().exists());
            }
        }
        fs::remove_dir_all(destination).unwrap();
    }
    assert!(!storage.join("demo").exists());
    assert!(!storage.join("child").exists());
}

#[tokio::test]
async fn applying_consumed_global_plan_fails_before_changing_lock() {
    let root = TempDir::new().unwrap();
    let service = FastSkillService::new(fastskill_core::ServiceConfig {
        skill_storage_path: root.path().join("skills"),
        ..Default::default()
    })
    .await
    .unwrap();
    let mut plans = [GlobalAddPlan {
        id: "demo".to_string(),
        current_revision: None,
        target_revision: "1.0.0".to_string(),
        origin: Origin::Local {
            path: root.path().join("source"),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: None,
        },
        groups: Vec::new(),
        dependencies: Vec::new(),
        changes: vec!["resolved content changed".to_string()],
        candidate: None,
    }];
    let mut lock = GlobalSkillsLock::new_empty();
    let error = apply_global_plans(&service, &mut lock, &mut plans)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("already applied"));
    assert!(lock.skills.is_empty());
    assert!(!root.path().join("skills/demo").exists());
}

#[tokio::test]
async fn restoring_an_absent_global_lock_is_idempotent_and_reports_directory_conflicts() {
    let root = TempDir::new().unwrap();
    let service = FastSkillService::new(fastskill_core::ServiceConfig {
        skill_storage_path: root.path().join("skills"),
        ..Default::default()
    })
    .await
    .unwrap();
    let lock_path = root.path().join("global-skills.lock");
    restore_global_add(&service, &lock_path, None, &[], Vec::new())
        .await
        .unwrap();
    fs::create_dir(&lock_path).unwrap();
    let error = restore_global_add(&service, &lock_path, None, &[], Vec::new())
        .await
        .unwrap_err();
    assert!(matches!(error, CliError::Io(_)));
    assert!(lock_path.is_dir());
}

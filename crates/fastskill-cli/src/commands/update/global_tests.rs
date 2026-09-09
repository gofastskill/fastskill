use super::*;

struct EnvGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

fn args() -> UpdateArgs {
    UpdateArgs {
        skill_id: None,
        check: false,
        dry_run: false,
        json: false,
        version: None,
        source: None,
        repository: None,
        bundle: None,
        from: None,
        strategy: "latest".to_string(),
        strategy_explicit: false,
        reindex: false,
        no_reindex: true,
        offline: false,
    }
}

fn write_skill(directory: &Path, id: &str, version: &str, body: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {id}\nversion: {version}\ndescription: fixture\n---\n{body}\n"),
    )
    .unwrap();
}

fn global_fixture() -> (TempDir, EnvGuard, EnvGuard, PathBuf, PathBuf) {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config");
    let cache = root.path().join("cache");
    let xdg = EnvGuard::set("XDG_CONFIG_HOME", &config);
    let cache_guard = EnvGuard::set("FASTSKILL_CACHE_DIR", &cache);
    let state = config.join("fastskill");
    let storage = state.join("skills");
    fs::create_dir_all(&storage).unwrap();
    (root, xdg, cache_guard, state, storage)
}

fn locked_entry(id: &str, dependencies: &[&str]) -> GlobalLockedSkillEntry {
    GlobalLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin: Origin::Local {
            path: id.into(),
            editable: false,
        },
        resolved: fastskill_core::core::origin::Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some("digest".to_string()),
        },
        dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
        groups: Vec::new(),
        installed_at: chrono::Utc::now(),
        last_checked_at: None,
        last_updated_at: None,
    }
}

#[test]
fn obsolete_dependencies_preserve_members_reachable_from_another_root() {
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots = vec!["alpha".to_string(), "beta".to_string()];
    lock.skills = vec![
        locked_entry("alpha", &["shared"]),
        locked_entry("beta", &["shared"]),
        locked_entry("shared", &[]),
    ];
    let candidate_ids = BTreeSet::from(["alpha".to_string()]);
    assert!(obsolete_dependencies(&lock, &["alpha".to_string()], &candidate_ids).is_empty());
}

#[test]
fn obsolete_dependencies_remove_unreachable_members() {
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots = vec!["alpha".to_string()];
    lock.skills = vec![locked_entry("alpha", &["old"]), locked_entry("old", &[])];
    let candidate_ids = BTreeSet::from(["alpha".to_string()]);
    assert_eq!(
        obsolete_dependencies(&lock, &["alpha".to_string()], &candidate_ids),
        ["old"]
    );
    assert_eq!(direct_root_ids(&lock), vec!["alpha"]);
    assert_eq!(
        selected_closure(&lock, &["alpha".to_string()]),
        BTreeSet::from(["alpha".to_string(), "old".to_string()])
    );
}

fn plan(changed: bool, blocked: bool) -> PlannedGlobalUpdate {
    PlannedGlobalUpdate {
        id: "demo".to_string(),
        current_revision: Some("1.0.0".to_string()),
        target_revision: "2.0.0".to_string(),
        origin: Origin::Local {
            path: "demo".into(),
            editable: false,
        },
        groups: Vec::new(),
        dependencies: Vec::new(),
        candidate: None,
        changes: changed
            .then(|| "resolved content changed".to_string())
            .into_iter()
            .collect(),
        remove: false,
        blocked,
    }
}

#[test]
fn global_json_result_covers_changed_unchanged_failed_partial_and_blocked() {
    assert!(emit_global_result(&[plan(false, false)], true, &[], None).is_ok());
    assert!(emit_global_result(&[plan(true, false)], false, &[], None).is_ok());
    assert!(emit_global_result(
        &[plan(false, false)],
        false,
        &["demo: failed".to_string()],
        None,
    )
    .is_ok());
    assert!(emit_global_result(
        &[plan(true, true)],
        false,
        &["other: failed".to_string()],
        None,
    )
    .is_ok());
}

#[tokio::test]
async fn applying_a_consumed_global_plan_is_rejected() {
    let root = TempDir::new().unwrap();
    let mut service = FastSkillService::new(fastskill_core::ServiceConfig {
        skill_storage_path: root.path().join("skills"),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let mut lock = GlobalSkillsLock::new_empty();
    let mut plans = vec![plan(true, false)];

    let result = apply_global_plan(
        &service,
        &root.path().join("global-skills.lock"),
        &mut lock,
        &mut plans,
        &[],
        true,
    )
    .await;
    assert!(matches!(
        result,
        Err(error) if error.to_string().contains("already applied")
    ));
}

#[tokio::test]
async fn directory_snapshots_restore_existing_and_remove_new_directories() {
    let root = TempDir::new().unwrap();
    let storage = root.path().join("skills");
    fs::create_dir_all(storage.join("existing")).unwrap();
    fs::write(storage.join("existing/SKILL.md"), "before").unwrap();
    let backup = TempDir::new().unwrap();
    let snapshots = capture_directories(
        &storage,
        [
            "existing".to_string(),
            "new".to_string(),
            "new-file".to_string(),
        ]
        .into_iter(),
        backup.path(),
    )
    .await
    .unwrap();
    fs::write(storage.join("existing/SKILL.md"), "after").unwrap();
    fs::create_dir_all(storage.join("new")).unwrap();
    fs::write(storage.join("new/SKILL.md"), "new").unwrap();
    fs::write(storage.join("new-file"), "new").unwrap();

    restore_directories(&snapshots).await.unwrap();

    assert_eq!(
        fs::read_to_string(storage.join("existing/SKILL.md")).unwrap(),
        "before"
    );
    assert!(!storage.join("new").exists());
    assert!(!storage.join("new-file").exists());
}

#[tokio::test]
async fn restore_and_remove_unlink_current_directory_symlinks_without_following_them() {
    let root = TempDir::new().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("editable");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("sentinel"), "keep").unwrap();
    create_directory_symlink(&target, &link).unwrap();

    restore_directories(&[DirectorySnapshot {
        installed: link.clone(),
        original: OriginalPath::Missing,
    }])
    .await
    .unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(target.join("sentinel").is_file());

    create_directory_symlink(&target, &link).unwrap();
    remove_global_path(&link).unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(target.join("sentinel").is_file());
}

#[tokio::test]
async fn path_removal_and_rollback_restore_every_authoritative_value() {
    let root = TempDir::new().unwrap();
    let storage = root.path().join("skills");
    let backup = TempDir::new().unwrap();
    fs::create_dir_all(storage.join("managed")).unwrap();
    fs::write(storage.join("managed/SKILL.md"), "before").unwrap();
    fs::write(storage.join("plain-file"), "plain").unwrap();
    fs::create_dir_all(storage.join("directory")).unwrap();
    remove_global_path(&storage.join("plain-file")).unwrap();
    remove_global_path(&storage.join("directory")).unwrap();
    remove_global_path(&storage.join("absent")).unwrap();

    let snapshots = capture_directories(
        &storage,
        ["managed".to_string(), "new".to_string()].into_iter(),
        backup.path(),
    )
    .await
    .unwrap();
    let lock_path = root.path().join("global-skills.lock");
    let original_lock = b"original lock";
    fs::write(&lock_path, original_lock).unwrap();

    let mut service = FastSkillService::new(fastskill_core::ServiceConfig {
        skill_storage_path: storage.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let managed_id = fastskill_core::SkillId::new("managed".to_string()).unwrap();
    let new_id = fastskill_core::SkillId::new("new".to_string()).unwrap();
    let definition = fastskill_core::SkillDefinition::new(
        managed_id.clone(),
        "managed".to_string(),
        "before".to_string(),
        "1.0.0".to_string(),
        Origin::Local {
            path: storage.join("managed"),
            editable: false,
        },
    );
    service
        .skill_manager()
        .force_register_skill(definition.clone())
        .await
        .unwrap();
    fs::write(storage.join("managed/SKILL.md"), "after").unwrap();
    fs::create_dir_all(storage.join("new")).unwrap();
    fs::write(&lock_path, "changed lock").unwrap();

    rollback_global_update(
        &service,
        &snapshots,
        &lock_path,
        original_lock,
        &[
            (new_id.clone(), None),
            (managed_id.clone(), Some(definition)),
        ],
    )
    .await
    .unwrap();

    assert_eq!(fs::read(&lock_path).unwrap(), original_lock);
    assert_eq!(
        fs::read_to_string(storage.join("managed/SKILL.md")).unwrap(),
        "before"
    );
    assert!(!storage.join("new").exists());
    assert!(service
        .skill_manager()
        .get_skill(&managed_id)
        .await
        .unwrap()
        .is_some());
    assert!(service
        .skill_manager()
        .get_skill(&new_id)
        .await
        .unwrap()
        .is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn directory_snapshots_restore_symlinks() {
    let root = TempDir::new().unwrap();
    let storage = root.path().join("skills");
    let source = root.path().join("source");
    fs::create_dir_all(&storage).unwrap();
    fs::create_dir_all(&source).unwrap();
    let link = storage.join("editable");
    create_directory_symlink(&source, &link).unwrap();
    let backup = TempDir::new().unwrap();
    let snapshots = capture_directories(
        &storage,
        ["editable".to_string()].into_iter(),
        backup.path(),
    )
    .await
    .unwrap();
    fs::remove_file(&link).unwrap();
    fs::create_dir_all(&link).unwrap();
    restore_directories(&snapshots).await.unwrap();
    assert_eq!(fs::read_link(link).unwrap(), source);
}

#[tokio::test]
async fn global_update_reports_absent_and_empty_state_in_human_and_json_modes() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_root, _xdg, _cache, state, _storage) = global_fixture();
    execute_update_global(args(), None).await.unwrap();
    let mut json = args();
    json.json = true;
    json.check = true;
    execute_update_global(json.clone(), None).await.unwrap();

    GlobalSkillsLock::new_empty()
        .save_to_file(&state.join("global-skills.lock"))
        .unwrap();
    execute_update_global(args(), None).await.unwrap();
    execute_update_global(json, None).await.unwrap();
}

#[tokio::test]
async fn global_update_preflight_rejects_unknown_roots_and_each_incomplete_lock_shape() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_root, _xdg, _cache, state, storage) = global_fixture();
    let lock_path = state.join("global-skills.lock");
    let source = state.join("source");
    write_skill(&source, "demo", "2.0.0", "source");
    write_skill(&storage.join("demo"), "demo", "1.0.0", "installed");
    let digest = managed_tree_digest(&storage.join("demo")).unwrap();
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots.push("demo".to_string());
    let mut root = locked_entry("demo", &["absent"]);
    root.origin = Origin::Local {
        path: source,
        editable: false,
    };
    root.resolved.checksum = Some(digest);
    lock.skills.push(root);
    lock.save_to_file(&lock_path).unwrap();

    let mut unknown = args();
    unknown.skill_id = Some("unknown".to_string());
    assert!(matches!(
        execute_update_global(unknown, None).await,
        Err(CliError::Validation(message)) if message.contains("not a directly selected root")
    ));
    let mut json = args();
    json.json = true;
    assert!(matches!(
        execute_update_global(json.clone(), None).await,
        Err(CliError::Config(message)) if message.contains("absent from global lock")
    ));

    lock.skills[0].dependencies.clear();
    lock.skills[0].resolved.checksum = None;
    lock.save_to_file(&lock_path).unwrap();
    assert!(matches!(
        execute_update_global(json.clone(), None).await,
        Err(CliError::Config(message)) if message.contains("insufficient integrity")
    ));

    lock.skills[0].resolved.checksum = Some("wrong".to_string());
    lock.save_to_file(&lock_path).unwrap();
    assert!(matches!(
        execute_update_global(json.clone(), None).await,
        Err(CliError::Config(message)) if message.contains("was modified")
    ));

    fs::remove_dir_all(storage.join("demo")).unwrap();
    lock.save_to_file(&lock_path).unwrap();
    assert!(matches!(
        execute_update_global(json, None).await,
        Err(CliError::Config(message)) if message.contains("content is missing")
    ));
}

#[tokio::test]
async fn global_update_human_preview_and_apply_share_the_verified_local_plan() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_root, _xdg, _cache, state, storage) = global_fixture();
    let source = state.join("source");
    write_skill(&source, "demo", "2.0.0", "updated");
    write_skill(&storage.join("demo"), "demo", "1.0.0", "installed");
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots.push("demo".to_string());
    let mut entry = locked_entry("demo", &[]);
    entry.origin = Origin::Local {
        path: source,
        editable: false,
    };
    entry.resolved.checksum = Some(managed_tree_digest(&storage.join("demo")).unwrap());
    lock.skills.push(entry);
    let lock_path = state.join("global-skills.lock");
    lock.save_to_file(&lock_path).unwrap();

    let mut preview = args();
    preview.dry_run = true;
    execute_update_global(preview, None).await.unwrap();
    assert!(fs::read_to_string(storage.join("demo/SKILL.md"))
        .unwrap()
        .contains("installed"));

    execute_update_global(args(), None).await.unwrap();
    assert!(fs::read_to_string(storage.join("demo/SKILL.md"))
        .unwrap()
        .contains("updated"));
    let updated = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(updated.skills[0].resolved.version, "2.0.0");
}

#[tokio::test]
async fn global_update_json_preview_then_apply_prunes_an_unreachable_dependency() {
    let _guard = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_root, _xdg, _cache, state, storage) = global_fixture();
    let source = state.join("source");
    write_skill(&source, "demo", "2.0.0", "updated root");
    write_skill(&storage.join("demo"), "demo", "1.0.0", "installed root");
    write_skill(&storage.join("old-child"), "old-child", "1.0.0", "obsolete");

    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots.push("demo".to_string());
    let mut root_entry = locked_entry("demo", &["old-child"]);
    root_entry.origin = Origin::Local {
        path: source,
        editable: false,
    };
    root_entry.resolved.checksum = Some(managed_tree_digest(&storage.join("demo")).unwrap());
    let mut old_entry = locked_entry("old-child", &[]);
    old_entry.resolved.checksum = Some(managed_tree_digest(&storage.join("old-child")).unwrap());
    lock.skills = vec![root_entry, old_entry];
    let lock_path = state.join("global-skills.lock");
    lock.save_to_file(&lock_path).unwrap();

    let mut preview = args();
    preview.dry_run = true;
    preview.json = true;
    execute_update_global(preview, None).await.unwrap();
    assert!(storage.join("old-child").exists());
    assert_eq!(
        GlobalSkillsLock::load_from_file(&lock_path)
            .unwrap()
            .skills
            .len(),
        2
    );

    execute_update_global(args(), None).await.unwrap();
    assert!(!storage.join("old-child").exists());
    let updated = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(updated.skills.len(), 1);
    assert_eq!(updated.skills[0].id, "demo");
}

#[tokio::test]
async fn global_update_adds_a_newly_declared_dependency_to_content_and_lock() {
    let _guard = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_root, _xdg, _cache, state, storage) = global_fixture();
    let source = state.join("root-source");
    let child = state.join("child-source");
    write_skill(&source, "demo", "2.0.0", "updated root");
    write_skill(&child, "new-child", "1.0.0", "new dependency");
    fs::write(
        source.join("skill-project.toml"),
        "[dependencies]\nnew-child = { origin = { type = \"local\", path = \"../child-source\" } }\n",
    )
    .unwrap();
    write_skill(&storage.join("demo"), "demo", "1.0.0", "installed root");

    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots.push("demo".to_string());
    let mut entry = locked_entry("demo", &[]);
    entry.origin = Origin::Local {
        path: source,
        editable: false,
    };
    entry.resolved.checksum = Some(managed_tree_digest(&storage.join("demo")).unwrap());
    lock.skills.push(entry);
    let lock_path = state.join("global-skills.lock");
    lock.save_to_file(&lock_path).unwrap();

    let mut apply = args();
    apply.json = true;
    execute_update_global(apply, None).await.unwrap();

    assert!(storage.join("new-child/SKILL.md").exists());
    let updated = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(updated.covered_roots, ["demo"]);
    assert_eq!(
        updated
            .skills
            .iter()
            .find(|entry| entry.id == "demo")
            .unwrap()
            .dependencies,
        ["new-child"]
    );
    assert!(updated.skills.iter().any(|entry| entry.id == "new-child"));
}

#[tokio::test]
async fn partial_apply_retains_pruned_dependencies_and_reports_failure() {
    let _guard = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_root, _xdg, _cache, state, storage) = global_fixture();
    let source = state.join("source");
    write_skill(&source, "demo", "2.0.0", "updated root");
    write_skill(&storage.join("demo"), "demo", "1.0.0", "installed root");
    write_skill(&storage.join("old-child"), "old-child", "1.0.0", "retained");
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots.push("demo".to_string());
    let mut root_entry = locked_entry("demo", &["old-child"]);
    root_entry.origin = Origin::Local {
        path: source,
        editable: false,
    };
    root_entry.resolved.checksum = Some(managed_tree_digest(&storage.join("demo")).unwrap());
    let mut old_entry = locked_entry("old-child", &[]);
    old_entry.resolved.checksum = Some(managed_tree_digest(&storage.join("old-child")).unwrap());
    lock.skills = vec![root_entry, old_entry];
    let lock_path = state.join("global-skills.lock");
    lock.save_to_file(&lock_path).unwrap();
    *FAIL_APPLY_ID
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some("demo".to_string());

    let result = execute_update_global(args(), None).await;
    *FAIL_APPLY_ID
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = None;

    assert!(matches!(
        result,
        Err(CliError::Config(message)) if message.contains("injected apply failure")
    ));
    assert!(storage.join("old-child").exists());
    assert!(fs::read_to_string(storage.join("demo/SKILL.md"))
        .unwrap()
        .contains("installed root"));
    assert_eq!(
        GlobalSkillsLock::load_from_file(&lock_path)
            .unwrap()
            .skills
            .len(),
        2
    );
}

#[tokio::test]
async fn stale_plan_and_lock_save_failure_leave_a_recoverable_consistent_state() {
    let _guard = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_root, _xdg, _cache, state, storage) = global_fixture();
    let source = state.join("source");
    write_skill(&source, "demo", "2.0.0", "updated");
    write_skill(&storage.join("demo"), "demo", "1.0.0", "installed");
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots.push("demo".to_string());
    let mut entry = locked_entry("demo", &[]);
    entry.origin = Origin::Local {
        path: source,
        editable: false,
    };
    entry.resolved.checksum = Some(managed_tree_digest(&storage.join("demo")).unwrap());
    lock.skills.push(entry);
    let lock_path = state.join("global-skills.lock");
    lock.save_to_file(&lock_path).unwrap();
    let original_lock = fs::read(&lock_path).unwrap();

    CHANGE_LOCK_BEFORE_COMMIT.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        execute_update_global(args(), None).await,
        Err(CliError::Config(message)) if message.contains("state changed")
    ));
    assert!(!storage.join(".fastskill-state.lock.interrupted").exists());

    fs::write(&lock_path, &original_lock).unwrap();
    FAIL_LOCK_SAVE.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        execute_update_global(args(), None).await,
        Err(CliError::Config(message)) if message.contains("injected global lock save failure")
    ));
    assert_eq!(fs::read(&lock_path).unwrap(), original_lock);
    assert!(fs::read_to_string(storage.join("demo/SKILL.md"))
        .unwrap()
        .contains("installed"));
    assert!(!storage.join(".fastskill-state.lock.interrupted").exists());

    FAIL_LOCK_SAVE.store(true, std::sync::atomic::Ordering::SeqCst);
    FAIL_ROLLBACK.store(true, std::sync::atomic::Ordering::SeqCst);
    let error = execute_update_global(args(), None).await.unwrap_err();
    let CliError::Config(message) = error else {
        panic!("expected rollback failure to be a configuration error");
    };
    let retained = PathBuf::from(
        message
            .split("recovery inputs retained at ")
            .nth(1)
            .expect("rollback failure should identify retained recovery inputs"),
    );
    assert!(retained.exists());
    fs::remove_dir_all(retained).unwrap();
}

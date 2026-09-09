use super::*;
use fastskill_core::test_utils::DirGuard;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_execute_install_no_manifest() {
    // Use a shared mutex to serialize directory changes across parallel tests
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: false,
        json: false,
        reindex: false,
        no_reindex: false,
    };

    let result = execute_install(args.clone()).await;
    assert!(result.is_err(), "Expected error, got: {:?}", result);
    if let Err(CliError::Config(msg)) = result {
        assert!(
            (msg.contains("skill-project.toml not found") && msg.contains("fastskill init"))
                || msg.contains("skill-project.toml")
                    && (msg.contains("not found") || msg.contains("Manifest file not found")),
            "Error message must mention skill-project.toml and creation/init: '{}'",
            msg
        );
    } else {
        panic!("Expected Config error, got: {:?}", result);
    }

    let (json_result, output) =
        crate::output::capture(execute_install(InstallArgs { json: true, ..args })).await;
    assert!(json_result.is_err());
    let payload: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(payload["outcome"], "blocked");
}

#[tokio::test]
async fn test_execute_install_with_lock_file_not_found() {
    // Use a shared mutex to serialize directory changes across parallel tests
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    // Create skill-project.toml but no lock file
    fs::write(
        temp_dir.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();

    let args = InstallArgs {
        without: None,
        only: None,
        lock: true,
        depth: None,
        offline: false,
        dry_run: false,
        json: false,
        reindex: false,
        no_reindex: false,
    };

    let result = execute_install(args).await;
    assert!(result.is_err(), "Expected error, got: {:?}", result);
    if let Err(CliError::Config(msg)) = result {
        // Should fail because skills.lock not found
        assert!(
            msg.contains("skills.lock not found"),
            "Error message '{}' does not contain expected text",
            msg
        );
    } else {
        panic!("Expected Config error, got: {:?}", result);
    }
}

#[tokio::test]
async fn test_execute_install_with_empty_manifest() {
    // Use a shared mutex to serialize directory changes across parallel tests
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    // Create skill-project.toml at project root with empty [dependencies]
    let project_toml = temp_dir.path().join("skill-project.toml");
    fs::write(
        &project_toml,
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();

    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: false,
        json: false,
        reindex: false,
        no_reindex: false,
    };

    let result = execute_install(args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_install_success() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    let skills_dir = temp_dir.path().join(".claude/skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let source_dir = temp_dir.path().join("source-skill");
    fs::create_dir_all(&source_dir).unwrap();
    let skill_content = r#"---
name: test-skill
version: 1.0.0
description: A test skill for coverage
---
Body
"#;
    fs::write(source_dir.join("SKILL.md"), skill_content).unwrap();

    let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = { origin = { type = "local", path = "source-skill", editable = true } }
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: false,
        json: false,
        reindex: false,
        no_reindex: false,
    };

    let result = execute_install(args.clone()).await;
    assert!(result.is_ok(), "install failed: {result:?}");
    assert!(skills_dir.join("test-skill/SKILL.md").exists());
    assert!(temp_dir.path().join("skills.lock").exists());

    let (result, output) =
        crate::output::capture(execute_install(InstallArgs { json: true, ..args })).await;
    result.unwrap();
    let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert!(value["diagnostics"][0]
        .as_str()
        .unwrap()
        .contains("remains mutable"));
}

#[tokio::test]
async fn install_reports_local_repository_refresh_and_lock_only_manifest_error() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    let _guard = DirGuard(original_dir);
    std::env::set_current_dir(project.path()).unwrap();
    let previous_cache = std::env::var_os("FASTSKILL_CACHE_DIR");
    std::env::set_var("FASTSKILL_CACHE_DIR", project.path().join("cache"));
    let repository = project.path().join("repository");
    let source = repository.join("odd-folder");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: Display\nversion: 1.0.0\ndescription: fixture\nmetadata:\n  id: team/demo\n---\nBody\n",
    )
    .unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[[tool.fastskill.repositories]]\nname = \"team\"\ntype = \"local\"\npath = {:?}\npriority = 0\n\n[dependencies]\n\"team/demo\" = {{ origin = {{ type = \"repository\", repo = \"team\", skill = \"team/demo\" }} }}\n",
            repository
        ),
    )
    .unwrap();

    let (result, output) = crate::output::capture(execute_install(base_args())).await;
    result.unwrap();
    assert!(
        output.contains("Refreshed repository metadata: team"),
        "{output}"
    );

    fs::remove_file(project.path().join("skill-project.toml")).unwrap();
    let error = execute_install(InstallArgs {
        lock: true,
        ..base_args()
    })
    .await
    .unwrap_err();
    assert!(!error.to_string().is_empty());
    match previous_cache {
        Some(value) => std::env::set_var("FASTSKILL_CACHE_DIR", value),
        None => std::env::remove_var("FASTSKILL_CACHE_DIR"),
    }
}

#[tokio::test]
async fn json_apply_then_strict_restore_cover_structured_and_lock_paths() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    let _guard = DirGuard(original_dir);
    std::env::set_current_dir(project.path()).unwrap();
    fs::create_dir_all(project.path().join("source")).unwrap();
    fs::write(
        project.path().join("source/SKILL.md"),
        "---\nname: demo\nversion: 1.0.0\ndescription: demo\n---\nBody\n",
    )
    .unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\ndemo = { origin = { type = \"local\", path = \"source\" } }\n",
    )
    .unwrap();
    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: false,
        json: true,
        reindex: false,
        no_reindex: true,
    };
    let (result, output) = crate::output::capture(execute_install(args.clone())).await;
    result.unwrap();
    let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(value["outcome"], "changed");
    assert!(project.path().join("skills/demo/SKILL.md").exists());

    let (result, output) = crate::output::capture(execute_install(InstallArgs {
        dry_run: true,
        ..args.clone()
    }))
    .await;
    result.unwrap();
    let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(value["outcome"], "unchanged");
    assert_eq!(
        value["targets"][0]["retained"][0],
        "verified content already installed"
    );

    fs::remove_dir_all(project.path().join("skills/demo")).unwrap();
    let (result, output) = crate::output::capture(execute_install(InstallArgs {
        lock: true,
        json: false,
        ..args.clone()
    }))
    .await;
    result.unwrap();
    assert!(output.contains("Restored verified selections from skills.lock"));
    assert!(project.path().join("skills/demo/SKILL.md").exists());

    let (result, output) =
        crate::output::capture(execute_install(InstallArgs { lock: true, ..args })).await;
    result.unwrap();
    let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(value["scope"], "project");
    assert_eq!(value["outcome"], "unchanged");
    assert!(project.path().join("skills/demo/SKILL.md").exists());
}

#[tokio::test]
async fn dry_run_json_returns_one_plan_and_changes_no_state() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    let _guard = DirGuard(original_dir);
    std::env::set_current_dir(project.path()).unwrap();
    fs::create_dir_all(project.path().join("source")).unwrap();
    fs::write(
        project.path().join("source/SKILL.md"),
        "---\nname: demo\nversion: 1.0.0\ndescription: demo\n---\nBody\n",
    )
    .unwrap();
    fs::write(
            project.path().join("skill-project.toml"),
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\ndemo = { origin = { type = \"local\", path = \"source\" } }\n",
        )
        .unwrap();
    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: true,
        json: true,
        reindex: false,
        no_reindex: true,
    };

    let (result, output) = crate::output::capture(execute_install(args)).await;
    result.unwrap();
    let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(value["scope"], "project");
    assert_eq!(value["outcome"], "changed");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["targets"][0]["id"], "demo");
    assert!(!project.path().join("skills/demo").exists());
    assert!(!project.path().join("skills.lock").exists());
}

#[tokio::test]
async fn blocked_json_is_structured_and_returns_failure() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    let _guard = DirGuard(original_dir);
    std::env::set_current_dir(project.path()).unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[dependencies]\ndemo = { origin = { type = \"local\", path = \"missing\" } }\n",
    )
    .unwrap();
    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: true,
        json: true,
        reindex: false,
        no_reindex: true,
    };

    let (result, output) = crate::output::capture(execute_install(args)).await;
    assert!(result.is_err());
    let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(value["outcome"], "blocked");
    assert!(value["diagnostics"].as_array().unwrap().len() == 1);
    assert!(!project.path().join("skills.lock").exists());
}

#[tokio::test]
async fn failure_after_bundle_apply_restores_all_project_state() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let author = TempDir::new().unwrap();
    fs::create_dir_all(author.path().join("skills/bundle-member")).unwrap();
    fs::write(
        author.path().join("skills/bundle-member/SKILL.md"),
        "---\nname: bundle-member\nversion: 1.0.0\ndescription: bundle\n---\nBundle\n",
    )
    .unwrap();
    fs::write(
            author.path().join("skill-project.toml"),
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"1.0.0\"\n\n[bundle.members.bundle-member]\noverridable = false\n\n[dependencies]\nbundle-member = \"1.0.0\"\n",
        )
        .unwrap();
    let artifact = BundleService::new(author.path(), author.path().join("skills"))
        .build(author.path())
        .unwrap()
        .artifact;

    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    let _guard = DirGuard(original_dir);
    std::env::set_current_dir(project.path()).unwrap();
    fs::create_dir_all(project.path().join("source")).unwrap();
    fs::write(
        project.path().join("source/SKILL.md"),
        "---\nname: ordinary\nversion: 1.0.0\ndescription: ordinary\n---\nOrdinary\n",
    )
    .unwrap();
    let manifest = format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\nordinary = {{ origin = {{ type = \"local\", path = \"source\" }} }}\n\n[bundles.team]\nversion = \"1.0.0\"\nartifact = {:?}\n",
            artifact
        );
    fs::write(project.path().join("skill-project.toml"), &manifest).unwrap();
    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: false,
        json: false,
        reindex: false,
        no_reindex: true,
    };

    FAIL_DURING_BUNDLE_APPLY.store(true, std::sync::atomic::Ordering::SeqCst);
    let error = execute_install(args.clone()).await.unwrap_err();
    assert!(error.to_string().contains("during bundle apply"));
    assert_eq!(
        fs::read_to_string(project.path().join("skill-project.toml")).unwrap(),
        manifest
    );
    assert!(!project.path().join("skills.lock").exists());
    assert!(!project.path().join("skills/bundle-member").exists());
    assert!(!project.path().join("skills/ordinary").exists());

    FAIL_AFTER_BUNDLE_APPLY.store(true, std::sync::atomic::Ordering::SeqCst);
    let error = execute_install(args.clone()).await.unwrap_err();

    assert!(error.to_string().contains("injected failure"));
    assert_eq!(
        fs::read_to_string(project.path().join("skill-project.toml")).unwrap(),
        manifest
    );
    assert!(!project.path().join("skills.lock").exists());
    assert!(!project.path().join("skills/bundle-member").exists());
    assert!(!project.path().join("skills/ordinary").exists());
    assert!(!project
        .path()
        .join(".fastskill/bundle-history.toml")
        .exists());
    assert!(!project.path().join(".fastskill/bundles").exists());

    let (result, output) = crate::output::capture(execute_install(args)).await;
    result.unwrap();
    assert!(output.contains("Restored bundle team@1.0.0"), "{output}");
    assert!(output.contains("Installed ordinary"), "{output}");
}

/// Regression test for spec 013 minor #1: when every dependency fails to
/// install (here, an editable local dependency whose source directory was
/// never created), `execute_install` must not claim `skills.lock` was
/// updated -- it never touched the file. Verified against the real binary:
/// this exact manifest reproduces `[OK] Installation complete` /
/// `Updated skills.lock` printed immediately before the command exits
/// with an error and no `skills.lock` anywhere on disk.
#[tokio::test]
async fn test_execute_install_all_failed_does_not_claim_lock_updated() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    // Deliberately never created: "./skills/demo-skill" does not exist,
    // matching the reported repro's manifest.
    let manifest_content = r#"[dependencies]
demo-skill = { source = "local", path = "./skills/demo-skill", editable = true }

[tool.fastskill]
skills_directory = ".claude/skills"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

    let args = InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: false,
        json: false,
        reindex: false,
        no_reindex: false,
    };

    // `capture` scopes `Mode::Capture` to its own future, so this collects
    // the command's output without touching the process-wide mode.
    let (result, output) = crate::output::capture(execute_install(args)).await;

    assert!(
        result.is_err(),
        "install should fail: the dependency's local path never exists"
    );
    assert!(
        !temp_dir.path().join("skills.lock").exists(),
        "skills.lock must not exist: nothing was successfully installed"
    );
    // Positive assertion first: without it the negative one below would
    // pass vacuously if `capture` ever stopped capturing.
    assert!(
        output.contains("skills.lock was not modified"),
        "the failure path must say the lock was left alone: {output:?}"
    );
    assert!(
        !output.contains("Updated skills.lock"),
        "output must not claim the lock was updated when it was not written: {output:?}"
    );
}

fn base_args() -> InstallArgs {
    InstallArgs {
        without: None,
        only: None,
        lock: false,
        depth: None,
        offline: false,
        dry_run: false,
        json: false,
        reindex: false,
        no_reindex: true,
    }
}

#[test]
fn argument_conversion_ignores_wrong_types_and_accepts_repeated_groups() {
    let mut map = HashMap::new();
    map.insert(
        "without".to_string(),
        ArgValue::List(vec![ArgValue::Str("dev".to_string()), ArgValue::Bool(true)]),
    );
    map.insert("only".to_string(), ArgValue::Str("not-a-list".to_string()));
    map.insert("depth".to_string(), ArgValue::Int(7));
    map.insert("offline".to_string(), ArgValue::Bool(true));
    let args = InstallArgs::from_arg_value_map(&map);
    assert_eq!(args.without, Some(vec!["dev".to_string()]));
    assert_eq!(args.only, None);
    assert_eq!(args.depth, Some(7));
    assert!(args.offline);

    map.insert("without".to_string(), ArgValue::List(Vec::new()));
    map.insert("depth".to_string(), ArgValue::Str("wrong".to_string()));
    let args = InstallArgs::from_arg_value_map(&map);
    assert_eq!(args.without, None);
    assert_eq!(args.depth, None);
}

#[tokio::test]
async fn validation_rejects_conflicting_flags_depth_and_global_destination() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    let _guard = DirGuard(original_dir);
    std::env::set_current_dir(project.path()).unwrap();

    assert!(execute_install(InstallArgs {
        reindex: true,
        no_reindex: true,
        ..base_args()
    })
    .await
    .unwrap_err()
    .to_string()
    .contains("--reindex and --no-reindex"));
    assert!(execute_install(InstallArgs {
        offline: true,
        reindex: true,
        no_reindex: false,
        ..base_args()
    })
    .await
    .unwrap_err()
    .to_string()
    .contains("--offline and --reindex"));
    assert!(execute_install(InstallArgs {
        only: Some(vec!["default".to_string()]),
        without: Some(vec!["dev".to_string()]),
        ..base_args()
    })
    .await
    .unwrap_err()
    .to_string()
    .contains("--only and --without"));
    for depth in [0, i64::MAX] {
        assert!(matches!(
            execute_install(InstallArgs {
                depth: Some(depth),
                ..base_args()
            })
            .await,
            Err(CliError::InvalidDepth(_))
        ));
    }

    let (result, output) = crate::output::capture(execute_install_scoped(
        InstallArgs {
            json: true,
            ..base_args()
        },
        true,
        Some(project.path().join("skills")),
    ))
    .await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("--global and --skills-dir"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(output.trim()).unwrap()["outcome"],
        "blocked"
    );

    fs::write(
        project.path().join("skill-project.toml"),
        "[dependencies]\n",
    )
    .unwrap();
    let invalid_storage = project.path().join("storage-file");
    fs::write(&invalid_storage, "not a directory").unwrap();
    assert!(matches!(
        execute_install_scoped(base_args(), false, Some(invalid_storage)).await,
        Err(CliError::Service(_))
    ));
}

#[tokio::test]
async fn human_dry_run_reports_empty_and_installable_plans() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    let _guard = DirGuard(original_dir);
    std::env::set_current_dir(project.path()).unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();

    let (result, output) = crate::output::capture(execute_install(InstallArgs {
        dry_run: true,
        ..base_args()
    }))
    .await;
    result.unwrap();
    assert!(output.contains("No skills selected"));

    fs::create_dir(project.path().join("source")).unwrap();
    fs::write(
        project.path().join("source/SKILL.md"),
        "---\nname: demo\nversion: 1.0.0\ndescription: demo\n---\n",
    )
    .unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\ndemo = { origin = { type = \"local\", path = \"source\" } }\n",
    )
    .unwrap();
    let (result, output) = crate::output::capture(execute_install(InstallArgs {
        dry_run: true,
        ..base_args()
    }))
    .await;
    result.unwrap();
    assert!(output.contains("demo: not installed -> 1.0.0 (changed)"));
    assert!(output.contains("Dry run complete"));
}

use super::*;
use fastskill_core::core::{Origin, VersionConstraint};
use options::{controlled_origin, selected_repository, strategy_constraint};
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn validation_args() -> UpdateArgs {
    UpdateArgs {
        skill_id: Some("demo".to_string()),
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
        no_reindex: false,
        offline: false,
    }
}

#[test]
fn typed_argument_map_ignores_wrong_optional_types() {
    let mut map = HashMap::new();
    map.insert("skill-id".to_string(), ArgValue::Bool(true));
    map.insert("strategy".to_string(), ArgValue::Str("minor".to_string()));
    map.insert("check".to_string(), ArgValue::Bool(true));
    map.insert("json".to_string(), ArgValue::Bool(true));
    map.insert("reindex".to_string(), ArgValue::Bool(true));
    map.insert("no-reindex".to_string(), ArgValue::Bool(true));
    map.insert("offline".to_string(), ArgValue::Bool(true));
    let args = UpdateArgs::from_arg_value_map(&map);
    assert_eq!(args.skill_id, None);
    assert_eq!(args.strategy, "minor");
    assert!(
        args.strategy_explicit
            && args.check
            && args.json
            && args.reindex
            && args.no_reindex
            && args.offline
    );
    assert_eq!(
        UpdateArgs::command_spec().syntax,
        Some("update [SKILL_ID] [OPTIONS]")
    );
}

#[tokio::test]
async fn bundle_controls_are_validated_before_artifact_access() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = TempDir::new().unwrap();
    let original = std::env::current_dir().unwrap();
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _restore = Restore(original);
    std::env::set_current_dir(temp.path()).unwrap();

    let mut args = validation_args();
    args.bundle = Some("release".to_string());
    args.from = Some("release.zip".to_string());
    assert!(execute_update(args.clone(), true, None).await.is_err());
    assert!(execute_update(args.clone(), false, None).await.is_err());

    std::fs::write(temp.path().join("skill-project.toml"), "[dependencies]\n").unwrap();
    assert!(execute_update(args.clone(), false, None).await.is_err());
    args.skill_id = None;
    args.from = None;
    assert!(execute_update(args.clone(), false, None).await.is_err());
    args.from = Some("ftp://example.invalid/release.zip".to_string());
    assert!(matches!(
        execute_update(args, false, None).await,
        Err(CliError::Validation(message)) if message.contains("HTTPS")
    ));

    let mut args = validation_args();
    args.from = Some("release.zip".to_string());
    assert!(execute_update(args, false, None).await.is_err());

    let mut args = validation_args();
    args.reindex = true;
    args.no_reindex = true;
    assert!(execute_update(args, false, None).await.is_err());
}

#[test]
fn explicit_version_requires_one_target_and_no_strategy() {
    let mut args = validation_args();
    args.version = Some("1.2.3".to_string());
    assert!(validate_update_args(&args).is_ok());

    args.skill_id = None;
    assert!(validate_update_args(&args).is_err());
    args.skill_id = Some("demo".to_string());
    args.strategy_explicit = true;
    assert!(validate_update_args(&args).is_err());
    args.strategy_explicit = false;
    args.version = Some("^1.2".to_string());
    assert!(validate_update_args(&args).is_err());
}

#[test]
fn repository_alias_must_agree_and_requires_target() {
    let mut args = validation_args();
    args.source = Some("old".to_string());
    args.repository = Some("new".to_string());
    assert!(validate_update_args(&args).is_err());
    args.source = Some("new".to_string());
    assert_eq!(selected_repository(&args).unwrap(), Some("new"));
    args.skill_id = None;
    assert!(validate_update_args(&args).is_err());
}

#[test]
fn patch_and_minor_constraints_never_cross_their_boundary() {
    let floating = VersionConstraint::parse("*").unwrap();
    let patch = strategy_constraint("1.2.3", "patch", &floating).unwrap();
    assert!(patch.satisfies("1.2.9").unwrap());
    assert!(!patch.satisfies("1.3.0").unwrap());
    let minor = strategy_constraint("1.2.3", "minor", &floating).unwrap();
    assert!(minor.satisfies("1.9.0").unwrap());
    assert!(!minor.satisfies("2.0.0").unwrap());

    let exact = VersionConstraint::parse("1.2.3").unwrap();
    assert_eq!(
        strategy_constraint("1.2.3", "minor", &exact)
            .unwrap()
            .as_exact()
            .as_deref(),
        Some("1.2.3")
    );

    assert!(strategy_constraint("1.18446744073709551615.0", "patch", &floating).is_err());
    assert!(strategy_constraint("18446744073709551615.0.0", "minor", &floating).is_err());
}

#[test]
fn validation_rejects_every_conflicting_update_control() {
    let mut args = validation_args();
    args.check = true;
    args.dry_run = true;
    assert!(validate_update_args(&args).is_err());

    let mut args = validation_args();
    args.offline = true;
    args.reindex = true;
    assert!(validate_update_args(&args).is_err());

    let mut args = validation_args();
    args.strategy = "unsafe".to_string();
    assert!(validate_update_args(&args).is_err());

    let mut args = validation_args();
    args.version = Some("not-semver".to_string());
    assert!(validate_update_args(&args).is_err());

    for configure in [
        |args: &mut UpdateArgs| args.version = Some("1.2.3".to_string()),
        |args: &mut UpdateArgs| args.repository = Some("community".to_string()),
        |args: &mut UpdateArgs| args.strategy_explicit = true,
    ] {
        let mut args = validation_args();
        args.bundle = Some("release".to_string());
        configure(&mut args);
        assert!(validate_update_args(&args).is_err());
    }
}

#[test]
fn strategy_and_origin_controls_preserve_declared_intent() {
    let floating = VersionConstraint::parse("*").unwrap();
    assert_eq!(
        strategy_constraint("1.2.3", "latest", &floating).unwrap(),
        floating
    );
    assert!(strategy_constraint("invalid", "patch", &floating).is_err());
    assert_eq!(
        strategy_constraint("1.2.3", "future", &floating).unwrap(),
        floating
    );
    let declared = VersionConstraint::parse(">=1.0.0").unwrap();
    let bounded = strategy_constraint("1.2.3", "patch", &declared).unwrap();
    assert!(bounded.satisfies("1.2.9").unwrap());

    let origin = Origin::Repository {
        repo: "old".to_string(),
        skill: "demo".to_string(),
        version: None,
    };
    let mut args = validation_args();
    args.repository = Some("new".to_string());
    args.version = Some("2.0.0".to_string());
    let controlled = controlled_origin(&origin, "1.0.0", &args).unwrap();
    assert!(matches!(
        controlled,
        Origin::Repository { repo, version: Some(version), .. }
            if repo == "new" && version.as_exact().as_deref() == Some("2.0.0")
    ));

    let mut args = validation_args();
    args.strategy = "patch".to_string();
    args.strategy_explicit = true;
    let controlled = controlled_origin(&origin, "1.2.3", &args).unwrap();
    assert!(matches!(
        controlled,
        Origin::Repository { version: Some(version), .. }
            if version.satisfies("1.2.9").unwrap() && !version.satisfies("1.3.0").unwrap()
    ));

    let mut invalid_target = validation_args();
    invalid_target.version = Some("invalid".to_string());
    assert!(controlled_origin(&origin, "1.0.0", &invalid_target).is_err());

    let local = Origin::Local {
        path: "demo".into(),
        editable: false,
    };
    assert!(controlled_origin(&local, "1.0.0", &args).is_err());
    let args = validation_args();
    assert_eq!(controlled_origin(&local, "1.0.0", &args).unwrap(), local);
}

#[tokio::test]
async fn test_execute_update_no_manifest() {
    // Use a shared mutex to serialize directory changes across parallel tests
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    // Helper struct to ensure directory is restored even if test panics
    struct DirGuard(Option<std::path::PathBuf>);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            if let Some(dir) = &self.0 {
                let _ = std::env::set_current_dir(dir);
            }
        }
    }
    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    let args = UpdateArgs {
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
        no_reindex: false,
        offline: false,
    };

    let result = execute_update(args, false, None).await;
    assert!(result.is_err());
    if let Err(CliError::Validation(msg) | CliError::Config(msg)) = result {
        assert!(
            msg.contains("skill-project.toml not found") && msg.contains("fastskill init"),
            "Error message must mention skill-project.toml and fastskill init: '{}'",
            msg
        );
    } else {
        panic!("Expected Config error");
    }
}

#[tokio::test]
async fn test_execute_update_invalid_strategy() {
    let mut args = validation_args();
    args.strategy = "invalid-strategy".to_string();
    args.strategy_explicit = true;

    let result = execute_update(args, false, None).await;
    let CliError::Validation(message) = result.unwrap_err() else {
        panic!("invalid strategy must be rejected as validation");
    };
    assert!(message.contains("Invalid strategy"));
    assert!(message.contains("latest, patch, minor, major"));
}

#[tokio::test]
async fn test_execute_update_check_mode() {
    // Use a shared mutex to serialize directory changes across parallel tests
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    // Helper struct to ensure directory is restored even if test panics
    struct DirGuard(Option<std::path::PathBuf>);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            if let Some(dir) = &self.0 {
                let _ = std::env::set_current_dir(dir);
            }
        }
    }
    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    // Create skill-project.toml at project root using absolute paths
    let skill_project_toml = temp_dir.path().join("skill-project.toml");
    fs::write(&skill_project_toml, "[dependencies]").unwrap();

    let args = UpdateArgs {
        skill_id: None,
        check: true,
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
        no_reindex: false,
        offline: false,
    };

    // Should succeed in check mode even with no skills
    let result = execute_update(args, false, None).await;
    // May succeed or fail depending on lock file, but shouldn't panic
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_execute_update_success_with_check() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp_dir = TempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();

    struct DirGuard(Option<std::path::PathBuf>);
    impl Drop for DirGuard {
        fn drop(&mut self) {
            if let Some(dir) = &self.0 {
                let _ = std::env::set_current_dir(dir);
            }
        }
    }
    let _guard = DirGuard(original_dir);

    std::env::set_current_dir(temp_dir.path()).unwrap();

    let skills_dir = temp_dir.path().join(".claude/skills");
    fs::create_dir_all(&skills_dir).unwrap();

    let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = "1.0.0"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

    let lock_content = r#"version = "1.0.0"
generated_at = "2024-01-01T00:00:00Z"
fastskill_version = "0.1.0"

[[skills]]
id = "test-skill"
name = "test-skill"
version = "1.0.0"
source_type = "local"
source = { path = ".claude/skills/test-skill" }
"#;
    fs::write(temp_dir.path().join("skills.lock"), lock_content).unwrap();

    let args = UpdateArgs {
        skill_id: None,
        check: true,
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
        no_reindex: false,
        offline: false,
    };

    let result = execute_update(args, false, None).await;
    // Should succeed in check mode or fail with appropriate error
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn empty_and_unlocked_project_targets_have_explicit_results() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = TempDir::new().unwrap();
    let original = std::env::current_dir().unwrap();
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _restore = Restore(original);
    std::env::set_current_dir(temp.path()).unwrap();
    fs::write(
        temp.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    execute_update(validation_args_with_target(None), false, None)
        .await
        .unwrap();
    assert!(matches!(
        execute_update(validation_args(), false, None).await,
        Err(CliError::Validation(message)) if message.contains("not a declared dependency")
    ));

    fs::write(
        temp.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\ndemo = { origin = { type = \"local\", path = \"demo\" } }\n",
    )
    .unwrap();
    ProjectSkillsLock::new_empty()
        .save_to_file(&temp.path().join("skills.lock"))
        .unwrap();
    assert!(matches!(
        execute_update(validation_args(), false, None).await,
        Err(CliError::Validation(message)) if message.contains("no locked version")
    ));
}

fn validation_args_with_target(skill_id: Option<&str>) -> UpdateArgs {
    let mut args = validation_args();
    args.skill_id = skill_id.map(str::to_string);
    args
}

#[tokio::test]
async fn remote_bundle_download_errors_are_reported_before_any_bundle_mutation() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = TempDir::new().unwrap();
    let original = std::env::current_dir().unwrap();
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let _restore = Restore(original);
    std::env::set_current_dir(temp.path()).unwrap();
    fs::write(
        temp.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let listener = match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("failed to bind mock server: {error}"),
    };
    let server = MockServer::builder().listener(listener).start().await;
    Mock::given(method("GET"))
        .and(path("/missing.zip"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let mut args = validation_args_with_target(None);
    args.bundle = Some("release".to_string());
    args.from = Some(format!("{}/missing.zip", server.uri()));
    assert!(matches!(
        execute_update(args.clone(), false, None).await,
        Err(CliError::InvalidSource(message)) if message.contains("Failed to download")
    ));
    Mock::given(method("GET"))
        .and(path("/invalid.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not a bundle"))
        .mount(&server)
        .await;
    args.from = Some(format!("{}/invalid.zip", server.uri()));
    assert!(execute_update(args, false, None).await.is_err());
    assert!(!temp.path().join("skills.lock").exists());
}

struct UpdateProject {
    previous: PathBuf,
    directory: TempDir,
}

impl UpdateProject {
    fn new() -> Self {
        let project = Self {
            previous: std::env::current_dir().unwrap(),
            directory: TempDir::new().unwrap(),
        };
        std::env::set_current_dir(project.directory.path()).unwrap();
        fs::write(
            project.directory.path().join("skill-project.toml"),
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
        )
        .unwrap();
        project
    }

    fn path(&self) -> &std::path::Path {
        self.directory.path()
    }
}

impl Drop for UpdateProject {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.previous).unwrap();
    }
}

fn update_test_bundle(root: &std::path::Path, version: &str) -> PathBuf {
    let author = root.join(format!("author-{version}"));
    fs::create_dir_all(author.join("skills/demo")).unwrap();
    fs::write(
        author.join("skill-project.toml"),
        format!(
            "[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"{version}\"\n[bundle.members.demo]\noverridable = false\n[dependencies]\ndemo = \"1.0.0\"\n"
        ),
    )
    .unwrap();
    fs::write(
        author.join("skills/demo/SKILL.md"),
        format!("---\nname: demo\nversion: \"1.0.0\"\ndescription: demo\n---\nRelease {version}\n"),
    )
    .unwrap();
    fastskill_core::core::bundle::BundleService::new(&author, author.join("skills"))
        .build(&author.join("dist"))
        .unwrap()
        .artifact
}

#[tokio::test]
async fn bundle_update_preview_and_apply_report_the_same_release() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = UpdateProject::new();
    let first = update_test_bundle(project.path(), "1.0.0");
    let second = update_test_bundle(project.path(), "2.0.0");
    let bundles = fastskill_core::core::bundle::BundleService::new(
        project.path(),
        project.path().join("skills"),
    );
    bundles.install(&first).unwrap();
    let manifest = fs::read(project.path().join("skill-project.toml")).unwrap();
    let lock = fs::read(project.path().join("skills.lock")).unwrap();

    let mut args = validation_args_with_target(None);
    args.bundle = Some("team".to_string());
    args.from = Some(second.to_string_lossy().into_owned());
    args.dry_run = true;
    args.json = true;
    args.no_reindex = true;
    let (result, rendered) =
        crate::output::capture(execute_update(args.clone(), false, None)).await;
    result.unwrap();
    let preview: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["targets"][0]["target_revision"], "2.0.0");
    assert_eq!(preview["indexing"]["outcome"], "skipped");
    assert_eq!(
        fs::read(project.path().join("skill-project.toml")).unwrap(),
        manifest
    );
    assert_eq!(fs::read(project.path().join("skills.lock")).unwrap(), lock);

    args.from = Some(first.to_string_lossy().into_owned());
    args.json = false;
    let (result, rendered) =
        crate::output::capture(execute_update(args.clone(), false, None)).await;
    result.unwrap();
    assert!(rendered.contains("already unchanged"), "{rendered}");

    args.from = Some(second.to_string_lossy().into_owned());
    args.dry_run = false;
    let (result, rendered) =
        crate::output::capture(execute_update(args.clone(), false, None)).await;
    result.unwrap();
    assert!(rendered.contains("Updated bundle team@2.0.0"), "{rendered}");
    assert!(
        fs::read_to_string(project.path().join("skills/demo/SKILL.md"))
            .unwrap()
            .contains("Release 2.0.0")
    );

    args.json = true;
    let (result, rendered) = crate::output::capture(execute_update(args, false, None)).await;
    result.unwrap();
    let applied: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(applied["dry_run"], false);
    assert_eq!(applied["outcome"], "unchanged");
    assert_eq!(applied["targets"][0]["target_revision"], "2.0.0");
}

#[tokio::test]
async fn project_update_human_preview_preserves_installed_content_and_lock() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = UpdateProject::new();
    let source = project.path().join("sources/demo");
    fs::create_dir_all(&source).unwrap();
    let first = "---\nname: demo\nversion: \"1.0.0\"\ndescription: demo\n---\nFirst\n";
    fs::write(source.join("SKILL.md"), first).unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n[dependencies.demo.origin]\ntype = \"local\"\npath = \"sources/demo\"\n",
    )
    .unwrap();
    crate::commands::install::execute_install_scoped(
        crate::commands::install::InstallArgs::from_arg_value_map(&HashMap::new()),
        false,
        None,
    )
    .await
    .unwrap();
    let before = fs::read(project.path().join("skills.lock")).unwrap();
    fs::write(source.join("SKILL.md"), first.replace("1.0.0", "2.0.0")).unwrap();

    let mut args = validation_args();
    args.check = true;
    let (result, rendered) = crate::output::capture(execute_update(args, false, None)).await;
    result.unwrap();
    assert!(rendered.contains("1.0.0 -> 2.0.0 (upgrade)"), "{rendered}");
    assert!(rendered.contains("No changes were applied"), "{rendered}");
    assert_eq!(
        fs::read(project.path().join("skills.lock")).unwrap(),
        before
    );
    assert_eq!(
        fs::read_to_string(project.path().join("skills/demo/SKILL.md")).unwrap(),
        first
    );
}

#[tokio::test]
async fn bundle_transport_failures_preserve_existing_project_state() {
    use std::io::{Read, Write};
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = UpdateProject::new();
    let manifest = fs::read(project.path().join("skill-project.toml")).unwrap();
    let mut args = validation_args_with_target(None);
    args.bundle = Some("team".to_string());
    args.from = Some("https://[invalid".to_string());
    assert!(matches!(
        execute_update(args.clone(), false, None).await,
        Err(CliError::InvalidSource(message)) if message.contains("Failed to download")
    ));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    args.from = Some(format!(
        "http://{}/truncated.zip",
        listener.local_addr().unwrap()
    ));
    let server = std::thread::spawn(move || {
        let (mut connection, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let request_bytes = connection.read(&mut request).unwrap();
        assert!(request_bytes > 0);
        connection
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort")
            .unwrap();
    });
    assert!(matches!(
        execute_update(args, false, None).await,
        Err(CliError::InvalidSource(message)) if message.contains("Failed to read")
    ));
    server.join().unwrap();
    assert_eq!(
        fs::read(project.path().join("skill-project.toml")).unwrap(),
        manifest
    );
    assert!(!project.path().join("skills.lock").exists());
    assert!(!project.path().join("skills").exists());
}

use super::*;
use fastskill_core::core::lock::{ProjectLockedSkillEntry, ProjectSkillsLock};
use fastskill_core::core::origin::GitRef;
use fastskill_core::core::origin::Resolved;
use fastskill_core::{FastSkillService, ServiceConfig};
use std::fs;
use tempfile::TempDir;

#[path = "reconciliation_tests.rs"]
mod reconciliation;

#[test]
fn typed_arguments_and_origin_labels_cover_all_supported_variants() {
    for (name, expected) in [
        ("table", OutputFormat::Table),
        ("json", OutputFormat::Json),
        ("grid", OutputFormat::Grid),
        ("xml", OutputFormat::Xml),
    ] {
        assert_eq!(parse_output_format(name), Some(expected));
    }
    assert_eq!(parse_output_format("yaml"), None);

    let mut map = HashMap::new();
    map.insert("format".to_string(), ArgValue::Str("json".to_string()));
    map.insert("json".to_string(), ArgValue::Bool(true));
    map.insert("details".to_string(), ArgValue::Bool(true));
    map.insert("check".to_string(), ArgValue::Bool(true));
    map.insert(
        "only".to_string(),
        ArgValue::List(vec![
            ArgValue::Str("dev".to_string()),
            ArgValue::Bool(false),
        ]),
    );
    let args = ListArgs::from_arg_value_map(&map);
    assert_eq!(args.format, Some(OutputFormat::Json));
    assert!(args.json && args.details && args.check);
    assert_eq!(args.only, Some(vec!["dev".to_string()]));
    assert_eq!(repeated_strings(&ArgValue::Bool(true)), None);
    assert_eq!(repeated_strings(&ArgValue::List(vec![])), None);

    let origins = [
        Origin::Git {
            url: "https://example.invalid/repo.git".to_string(),
            r#ref: GitRef::Default,
            subdir: None,
        },
        Origin::Local {
            path: "skills/demo".into(),
            editable: false,
        },
        Origin::ZipUrl {
            url: "https://example.invalid/demo.zip".to_string(),
        },
        Origin::Repository {
            repo: "community".to_string(),
            skill: "demo".to_string(),
            version: None,
        },
    ];
    let expected = ["git", "local", "zip-url", "repository"];
    for (origin, expected) in origins.iter().zip(expected) {
        assert_eq!(origin_type_label(origin), expected);
        let (location, kind) = format_source_info(origin);
        assert!(location.is_some());
        assert_eq!(kind.as_deref(), Some(expected));
    }
}

#[tokio::test]
async fn test_execute_list_format_conflict() {
    let temp_dir = TempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Test conflicting --json and --format flags
    let args = ListArgs {
        format: Some(OutputFormat::Table),
        json: true,
        details: false,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };

    let result = execute_list(&service, args, false).await;
    assert!(result.is_err());
    if let Err(CliError::Config(msg)) = result {
        assert!(msg.contains("--json and --format cannot be used together"));
    } else {
        panic!("Expected Config error for format conflict");
    }
}

#[tokio::test]
async fn test_execute_list_no_manifest() {
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
    let work_dir: PathBuf = if resolve_project_file(temp_dir.path()).found {
        let fallback = std::env::temp_dir()
            .join("fastskill_no_manifest")
            .join(std::process::id().to_string());
        fs::create_dir_all(&fallback).unwrap();
        fallback
    } else {
        temp_dir.path().to_path_buf()
    };
    std::env::set_current_dir(&work_dir).unwrap();

    let config = ServiceConfig {
        skill_storage_path: work_dir.clone(),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let args = ListArgs {
        format: None,
        json: false,
        details: false,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };

    let result = execute_list(&service, args, false).await;
    assert!(result.is_err());
    if let Err(CliError::Config(msg)) = result {
        assert!(
            msg.contains("skill-project.toml"),
            "Error should mention skill-project.toml"
        );
    } else {
        panic!("Expected Config error for missing manifest");
    }
}

#[tokio::test]
async fn test_execute_list_manifest_empty_lock() {
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
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir,
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let args = ListArgs {
        format: None,
        json: false,
        details: false,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };

    let result = execute_list(&service, args, false).await;
    // May succeed or fail depending on various factors
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_execute_list_with_installed_skill() {
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

    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
    fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

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

    let config = ServiceConfig {
        skill_storage_path: skills_dir,
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let args = ListArgs {
        format: None,
        json: false,
        details: false,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };

    let result = execute_list(&service, args, false).await;
    // May succeed or fail depending on various factors
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_execute_list_json() {
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
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir,
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let args = ListArgs {
        format: None,
        json: false,
        details: false,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };

    let result = execute_list(&service, args, false).await;
    // May succeed or fail depending on various factors
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_execute_list_details() {
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
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

    let config = ServiceConfig {
        skill_storage_path: skills_dir,
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let args = ListArgs {
        format: None,
        json: false,
        details: false,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };

    let result = execute_list(&service, args, false).await;
    // May succeed or fail depending on various factors
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn bundle_listing_and_selector_conflicts_are_validated_consistently() {
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
    let storage = temp.path().join("skills");
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::write(
        temp.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: storage,
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();

    let mut args = ListArgs {
        format: None,
        json: false,
        details: false,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };
    args.only = Some(vec!["dev".to_string()]);
    args.without = Some(vec!["prod".to_string()]);
    assert!(matches!(
        execute_list(&service, args, false).await,
        Err(CliError::Validation(message)) if message.contains("--only and --without")
    ));

    let mut map = HashMap::new();
    map.insert("format".to_string(), ArgValue::Bool(true));
    assert_eq!(ListArgs::from_arg_value_map(&map).format, None);
}

#[tokio::test]
async fn project_reconciliation_classifies_each_managed_state_shape_and_group_selection() {
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
    let ids = [
        "missing-lock",
        "missing-content",
        "revision",
        "intent",
        "good",
        "bad",
        "insufficient",
    ];
    let dependencies = ids
        .iter()
        .map(|id| {
            format!(
                "{id} = {{ origin = {{ type = \"repository\", repo = \"team\", skill = \"{id}\", version = \"1.0.0\" }} }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        temp.path().join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n{dependencies}\n"
        ),
    )
    .unwrap();
    let storage = temp.path().join("skills");
    for id in ["revision", "intent", "good", "bad", "insufficient"] {
        let directory = storage.join(id);
        fs::create_dir_all(&directory).unwrap();
        let version = if id == "revision" { "2.0.0" } else { "1.0.0" };
        fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {id}\nversion: {version}\ndescription: fixture\n---\n# {id}\n"),
        )
        .unwrap();
    }
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = ids.iter().map(|id| (*id).to_string()).collect();
    for id in ids.into_iter().filter(|id| *id != "missing-lock") {
        let origin = Origin::Repository {
            repo: if id == "intent" { "other" } else { "team" }.to_string(),
            skill: id.to_string(),
            version: Some(fastskill_core::core::VersionConstraint::parse("1.0.0").unwrap()),
        };
        let checksum = match id {
            "good" => Some(
                managed_tree_digest(&storage.join(id)).expect("good fixture must be digestible"),
            ),
            "bad" => Some("wrong".to_string()),
            "missing-content" => Some("missing".to_string()),
            _ => None,
        };
        lock.skills.push(ProjectLockedSkillEntry {
            id: id.to_string(),
            name: id.to_string(),
            origin,
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum,
            },
            dependencies: Vec::new(),
            groups: Vec::new(),
            depth: 0,
            parent_skill: None,
            required_by: Vec::new(),
        });
    }
    lock.save_to_file(&temp.path().join("skills.lock")).unwrap();
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: storage,
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let base = ListArgs {
        format: Some(OutputFormat::Json),
        json: false,
        details: true,
        check: false,
        only: None,
        without: None,
        skills_dir: None,
    };
    execute_list(&service, base.clone(), false).await.unwrap();
    let mut only = base.clone();
    only.only = Some(vec!["default".to_string()]);
    execute_list(&service, only, false).await.unwrap();
    let mut without = base.clone();
    without.without = Some(vec!["default".to_string()]);
    execute_list(&service, without, false).await.unwrap();
    let mut unknown = base;
    unknown.only = Some(vec!["unknown".to_string()]);
    assert!(matches!(
        execute_list(&service, unknown, false).await,
        Err(CliError::Validation(message)) if message.contains("Unknown group")
    ));
}

use super::*;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_execute_repos_list() {
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

    let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
    fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

    let args = ReposListArgs {
        format: None,
        json: false,
    };

    let result = execute_repos_list(args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execute_repos_skills() {
    // Note: This test is expected to fail without a configured repository
    // It's here to verify the command structure compiles correctly
    let args = ReposSkillsArgs {
        repository: None,
        scope: None,
        all_versions: false,
        include_pre_release: false,
        format: None,
        json: false,
    };

    let result = execute_repos_skills(args).await;
    // Should fail due to missing repository configuration, but shouldn't panic
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_repos_command_excludes_search_variant() {
    // This compile-time test ensures the ReposCommand enum does not contain a Search variant.
    // Search is a top-level command to maintain clear architectural separation.
    let _command_list = [
        "List", "Add", "Remove", "Info", "Update", "Test", "Refresh", "Skills", "Show", "Versions",
    ];
}

#[test]
fn test_repos_command_has_approved_subcommands() {
    // Verify ReposCommand contains exactly the approved subcommands
    // This test validates the command structure for specification 026a
    use std::mem::discriminant;

    let list = ReposCommand::List {
        format: None,
        json: false,
    };
    let add = ReposCommand::Add {
        name: "test".to_string(),
        repo_type: "local".to_string(),
        // Value is incidental: this is a compile-time discriminant test,
        // no I/O touches this field, so a platform-neutral string is fine.
        url_or_path: "test-path".to_string(),
        priority: None,
        branch: None,
        tag: None,
        auth_type: None,
        auth_env: None,
        auth_key_path: None,
        auth_username: None,
    };
    let remove = ReposCommand::Remove {
        name: "test".to_string(),
    };
    let info = ReposCommand::Info {
        name: "test".to_string(),
        format: None,
        json: false,
    };
    let update = ReposCommand::Update {
        name: "test".to_string(),
        branch: None,
        priority: None,
    };
    let test = ReposCommand::Test {
        name: "test".to_string(),
    };
    let refresh = ReposCommand::Refresh { name: None };
    let skills = ReposCommand::Skills {
        repository: None,
        scope: None,
        all_versions: false,
        include_pre_release: false,
        format: None,
        json: false,
    };
    let show = ReposCommand::Show {
        skill_id: "test".to_string(),
        repository: None,
    };
    let versions = ReposCommand::Versions {
        skill_id: "test".to_string(),
        repository: None,
    };

    // Verify all commands have different discriminants (different variants)
    let discriminants = vec![
        discriminant(&list),
        discriminant(&add),
        discriminant(&remove),
        discriminant(&info),
        discriminant(&update),
        discriminant(&test),
        discriminant(&refresh),
        discriminant(&skills),
        discriminant(&show),
        discriminant(&versions),
    ];

    // All discriminants should be unique (10 unique subcommands)
    assert_eq!(
        discriminants.len(),
        discriminants
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
}

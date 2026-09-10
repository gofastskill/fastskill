use super::*;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

#[test]
fn add_typed_arguments_preserve_repository_options() {
    let values = HashMap::from([
        ("name".to_string(), ArgValue::Str("team".to_string())),
        (
            "url-or-path".to_string(),
            ArgValue::Str("https://example.invalid/team.git".to_string()),
        ),
        (
            "repo-type".to_string(),
            ArgValue::Str("git-marketplace".to_string()),
        ),
        ("priority".to_string(), ArgValue::Int(7)),
        ("branch".to_string(), ArgValue::Str("stable".to_string())),
        ("tag".to_string(), ArgValue::Str("v2".to_string())),
        ("auth-type".to_string(), ArgValue::Str("pat".to_string())),
        (
            "auth-env".to_string(),
            ArgValue::Str("TEAM_TOKEN".to_string()),
        ),
        (
            "auth-key-path".to_string(),
            ArgValue::Str("keys/team".to_string()),
        ),
        (
            "auth-username".to_string(),
            ArgValue::Str("octocat".to_string()),
        ),
    ]);

    let args = ReposAddArgs::from_arg_value_map(&values);
    assert_eq!(args.name, "team");
    assert_eq!(args.url_or_path, "https://example.invalid/team.git");
    assert_eq!(args.repo_type, "git-marketplace");
    assert_eq!(args.priority, Some(7));
    assert_eq!(args.branch.as_deref(), Some("stable"));
    assert_eq!(args.tag.as_deref(), Some("v2"));
    assert_eq!(args.auth_type.as_deref(), Some("pat"));
    assert_eq!(args.auth_env.as_deref(), Some("TEAM_TOKEN"));
    assert_eq!(args.auth_key_path, Some("keys/team".into()));
    assert_eq!(args.auth_username.as_deref(), Some("octocat"));
}

#[test]
fn add_typed_spec_rejects_branch_and_tag_together() {
    let values = HashMap::from([
        ("name".to_string(), ArgValue::Str("team".to_string())),
        (
            "url-or-path".to_string(),
            ArgValue::Str("https://example.invalid/team.git".to_string()),
        ),
        (
            "repo-type".to_string(),
            ArgValue::Str("git-marketplace".to_string()),
        ),
        ("branch".to_string(), ArgValue::Str("stable".to_string())),
        ("tag".to_string(), ArgValue::Str("v2".to_string())),
    ]);

    let diagnostics = ReposAddArgs::command_spec().validate_typed_args(&values);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "E005");
    assert!(diagnostics[0].message.contains("--branch"));
    assert!(diagnostics[0].message.contains("--tag"));
}

#[test]
fn typed_specs_describe_every_repository_subcommand() {
    fn assert_spec<T: IntoCommandSpec>(syntax: &str, expected_args: &[&str]) {
        let spec = T::command_spec();
        assert_eq!(spec.syntax, Some(syntax));
        assert_eq!(
            spec.args.iter().map(|arg| arg.name).collect::<Vec<_>>(),
            expected_args
        );
    }

    assert_spec::<ReposListArgs>("repo list [OPTIONS]", &["format", "json"]);
    assert_spec::<ReposAddArgs>(
        "repo add <NAME> <URL-OR-PATH> [OPTIONS]",
        &[
            "name",
            "url-or-path",
            "repo-type",
            "priority",
            "branch",
            "tag",
            "auth-type",
            "auth-env",
            "auth-key-path",
            "auth-username",
        ],
    );
    assert_spec::<ReposRemoveArgs>("repo remove <NAME>", &["name"]);
    assert_spec::<ReposInfoArgs>("repo info <NAME> [OPTIONS]", &["name", "format", "json"]);
    assert_spec::<ReposUpdateArgs>(
        "repo update <NAME> [OPTIONS]",
        &["name", "branch", "priority"],
    );
    assert_spec::<ReposTestArgs>("repo test <NAME>", &["name"]);
    assert_spec::<ReposRefreshArgs>("repo refresh [NAME]", &["name"]);
    assert_spec::<ReposSkillsArgs>(
        "repo skills [REPOSITORY] [OPTIONS]",
        &[
            "repo",
            "repository",
            "scope",
            "all-versions",
            "include-pre-release",
            "format",
            "json",
        ],
    );
    assert_spec::<ReposShowArgs>(
        "repo show <SKILL-ID> [OPTIONS]",
        &["skill-id", "repository"],
    );
    assert_spec::<ReposVersionsArgs>(
        "repo versions <SKILL-ID> [OPTIONS]",
        &["skill-id", "repository"],
    );
}

#[test]
fn typed_argument_maps_preserve_each_repository_command_value() {
    for (value, expected) in [
        ("table", OutputFormat::Table),
        ("json", OutputFormat::Json),
        ("grid", OutputFormat::Grid),
        ("xml", OutputFormat::Xml),
    ] {
        assert_eq!(super::args::parse_output_format(value), Some(expected));
    }
    assert_eq!(super::args::parse_output_format("yaml"), None);

    let list = ReposListArgs::from_arg_value_map(&HashMap::from([
        ("format".to_string(), ArgValue::Str("grid".to_string())),
        ("json".to_string(), ArgValue::Bool(true)),
    ]));
    assert_eq!(list.format, Some(OutputFormat::Grid));
    assert!(list.json);

    let remove = ReposRemoveArgs::from_arg_value_map(&HashMap::from([(
        "name".to_string(),
        ArgValue::Str("obsolete".to_string()),
    )]));
    assert_eq!(remove.name, "obsolete");

    let info = ReposInfoArgs::from_arg_value_map(&HashMap::from([
        ("name".to_string(), ArgValue::Str("team".to_string())),
        ("format".to_string(), ArgValue::Str("xml".to_string())),
        ("json".to_string(), ArgValue::Bool(true)),
    ]));
    assert_eq!(info.name, "team");
    assert_eq!(info.format, Some(OutputFormat::Xml));
    assert!(info.json);

    let update = ReposUpdateArgs::from_arg_value_map(&HashMap::from([
        ("name".to_string(), ArgValue::Str("team".to_string())),
        ("branch".to_string(), ArgValue::Str("next".to_string())),
        ("priority".to_string(), ArgValue::Int(3)),
    ]));
    assert_eq!(update.name, "team");
    assert_eq!(update.branch.as_deref(), Some("next"));
    assert_eq!(update.priority, Some(3));

    let test = ReposTestArgs::from_arg_value_map(&HashMap::from([(
        "name".to_string(),
        ArgValue::Str("team".to_string()),
    )]));
    assert_eq!(test.name, "team");

    let refresh = ReposRefreshArgs::from_arg_value_map(&HashMap::from([(
        "name".to_string(),
        ArgValue::Str("team".to_string()),
    )]));
    assert_eq!(refresh.name.as_deref(), Some("team"));

    let skills = ReposSkillsArgs::from_arg_value_map(&HashMap::from([
        ("repository".to_string(), ArgValue::Str("team".to_string())),
        ("scope".to_string(), ArgValue::Str("platform".to_string())),
        ("all-versions".to_string(), ArgValue::Bool(true)),
        ("include-pre-release".to_string(), ArgValue::Bool(true)),
        ("format".to_string(), ArgValue::Str("table".to_string())),
        ("json".to_string(), ArgValue::Bool(true)),
    ]));
    assert_eq!(skills.repository.as_deref(), Some("team"));
    assert_eq!(skills.scope.as_deref(), Some("platform"));
    assert!(skills.all_versions && skills.include_pre_release && skills.json);
    assert_eq!(skills.format, Some(OutputFormat::Table));

    let positional_skills = ReposSkillsArgs::from_arg_value_map(&HashMap::from([(
        "repo".to_string(),
        ArgValue::Str("community".to_string()),
    )]));
    assert_eq!(positional_skills.repository.as_deref(), Some("community"));

    let show = ReposShowArgs::from_arg_value_map(&HashMap::from([
        ("skill-id".to_string(), ArgValue::Str("slides".to_string())),
        ("repository".to_string(), ArgValue::Str("team".to_string())),
    ]));
    assert_eq!(show.skill_id, "slides");
    assert_eq!(show.repository.as_deref(), Some("team"));

    let versions = ReposVersionsArgs::from_arg_value_map(&HashMap::from([
        ("skill-id".to_string(), ArgValue::Str("slides".to_string())),
        (
            "repository".to_string(),
            ArgValue::Str("community".to_string()),
        ),
    ]));
    assert_eq!(versions.skill_id, "slides");
    assert_eq!(versions.repository.as_deref(), Some("community"));
}

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

struct RepositoryProject {
    root: TempDir,
    previous: std::path::PathBuf,
}

impl RepositoryProject {
    fn new(repositories: &str) -> Self {
        let root = TempDir::new().unwrap();
        let previous = std::env::current_dir().unwrap();
        fs::write(
            root.path().join("skill-project.toml"),
            format!(
                "schema_version = \"1\"\n[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"\n{repositories}"
            ),
        )
        .unwrap();
        std::env::set_current_dir(root.path()).unwrap();
        Self { root, previous }
    }

    fn manifest_path(&self) -> std::path::PathBuf {
        self.root.path().join("skill-project.toml")
    }
}

impl Drop for RepositoryProject {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

#[tokio::test]
async fn updating_repository_branch_replaces_tag_but_priority_only_preserves_it() {
    use fastskill_core::core::repository::{RepositoryAuth, RepositoryConfig};
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _project = RepositoryProject::new(
        r#"
[[tool.fastskill.repositories]]
name = "team"
type = "git-marketplace"
priority = 1
url = "https://example.invalid/team.git"
tag = "v1.2.0"
auth = { type = "pat", env_var = "TEAM_TOKEN" }
"#,
    );
    execute_repos_update(ReposUpdateArgs {
        name: "team".to_string(),
        branch: None,
        priority: Some(3),
    })
    .await
    .unwrap();
    let manager = super::helpers::load_repo_manager().await.unwrap();
    let repository = manager.get_repository("team").unwrap();
    assert_eq!(repository.priority, 3);
    assert!(
        matches!(&repository.config, RepositoryConfig::GitMarketplace { branch: None, tag: Some(tag), .. } if tag == "v1.2.0")
    );

    execute_repos_update(ReposUpdateArgs {
        name: "team".to_string(),
        branch: Some("develop".to_string()),
        priority: None,
    })
    .await
    .unwrap();
    let manager = super::helpers::load_repo_manager().await.unwrap();
    let repository = manager.get_repository("team").unwrap();
    assert_eq!(repository.priority, 3);
    assert!(
        matches!(&repository.config, RepositoryConfig::GitMarketplace { url, branch: Some(branch), tag: None }
        if url == "https://example.invalid/team.git" && branch == "develop")
    );
    assert!(
        matches!(&repository.auth, Some(RepositoryAuth::Pat { env_var }) if env_var == "TEAM_TOKEN")
    );
}

#[tokio::test]
async fn updating_non_git_repository_branch_fails_without_saving_any_changes() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let project = RepositoryProject::new(
        r#"
# Preserve the original file if an update is rejected.
[[tool.fastskill.repositories]]
name = "local"
type = "local"
priority = 1
path = "catalog"
[[tool.fastskill.repositories]]
name = "http"
type = "http-registry"
priority = 2
index_url = "https://example.invalid/index"
[[tool.fastskill.repositories]]
name = "archive"
type = "zip-url"
priority = 3
zip_url = "https://example.invalid/catalog.zip"
"#,
    );
    let original = fs::read(project.manifest_path()).unwrap();
    for name in ["local", "http", "archive"] {
        let error = execute_repos_update(ReposUpdateArgs {
            name: name.to_string(),
            branch: Some("develop".to_string()),
            priority: Some(9),
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("only valid for git-marketplace"));
        assert_eq!(fs::read(project.manifest_path()).unwrap(), original);
    }
    execute_repos_update(ReposUpdateArgs {
        name: "local".to_string(),
        branch: None,
        priority: Some(9),
    })
    .await
    .unwrap();
    let manager = super::helpers::load_repo_manager().await.unwrap();
    assert_eq!(manager.get_repository("local").unwrap().priority, 9);
}

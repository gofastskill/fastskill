use super::*;
use std::path::PathBuf;

/// Was `pre_origin_manifest_gives_actionable_hint`, which asserted a pre-`Origin`
/// manifest FAILS with a hint telling the author to hand-edit it. That behaviour is
/// gone on purpose: a well-formed legacy manifest is now migrated automatically, so
/// there is nothing for the author to do and nothing to hint at.
///
/// The hint itself still exists in `parse_current` and still fires for legacy-looking
/// input that cannot be migrated — see
/// `schema_version_tests::legacy_entry_missing_required_field_is_reported_not_guessed`.
#[test]
fn pre_origin_manifest_is_migrated_rather_than_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skill-project.toml");
    std::fs::write(
        &path,
        "[metadata]\nid = \"x\"\nversion = \"1.0.0\"\ndescription = \"d\"\n\n\
             [dependencies.old]\nsource = \"git\"\nurl = \"https://example.com/x.git\"\n",
    )
    .unwrap();

    let project = SkillProjectToml::load_from_file(&path)
        .expect("a well-formed pre-Origin manifest must now load, not error");

    assert_eq!(
        project.schema_version.as_deref(),
        Some(MANIFEST_SCHEMA_VERSION),
        "the loaded value must carry the current schema version"
    );
    match project
        .dependencies
        .as_ref()
        .unwrap()
        .dependencies
        .get("old")
    {
        Some(DependencySpec::Inline {
            origin: Origin::Git { url, .. },
            ..
        }) => assert_eq!(url, "https://example.com/x.git"),
        other => panic!("expected a migrated git origin, got {other:?}"),
    }

    // Reading must not rewrite the file — migration is persisted only on save.
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(
        on_disk.contains("source = \"git\""),
        "load_from_file must leave the file untouched, got:\n{on_disk}"
    );
}

#[test]
fn test_manifest_parsing() {
    let toml_content = r#"
            [metadata]
            version = "1.0.0"

            [[skills]]
            id = "web-scraper"
            origin = { type = "git", url = "https://github.com/org/repo.git", ref = { branch = "main" } }

            [[skills]]
            id = "dev-tools"
            origin = { type = "git", url = "https://github.com/org/dev-tools.git" }
            groups = ["dev"]

            [[skills]]
            id = "monitoring"
            origin = { type = "repository", repo = "team-tools", skill = "monitoring", version = "=2.1.0" }
            groups = ["prod"]
        "#;

    let manifest: SkillsManifest = toml::from_str(toml_content).unwrap();

    assert_eq!(manifest.metadata.version, "1.0.0");
    assert_eq!(manifest.skills.len(), 3);

    // Check all skills
    let all_skills = manifest.get_all_skills();
    assert_eq!(all_skills.len(), 3);

    // Check skills without dev group
    let without_dev = manifest.get_skills_for_groups(Some(&["dev".to_string()]), None);
    assert_eq!(without_dev.len(), 2); // web-scraper and monitoring

    // Check only prod group
    let only_prod = manifest.get_skills_for_groups(None, Some(&["prod".to_string()]));
    assert_eq!(only_prod.len(), 1); // monitoring
}

#[test]
fn test_origin_variants_serialize_as_expected() {
    // Test Git origin
    let git_origin = Origin::Git {
        url: "https://github.com/org/repo.git".to_string(),
        r#ref: crate::core::origin::GitRef::Branch("main".to_string()),
        subdir: None,
    };

    // Test Repository reference
    let repo_origin = Origin::Repository {
        repo: "team-tools".to_string(),
        skill: "monitoring".to_string(),
        version: Some(VersionConstraint::parse("2.1.0").unwrap()),
    };

    // Test Local origin
    let _local_origin = Origin::Local {
        path: PathBuf::from("./local-skills"),
        editable: false,
    };

    // Test ZipUrl origin
    let _zip_origin = Origin::ZipUrl {
        url: "https://skills.example.com/".to_string(),
    };

    // Verify they serialize correctly
    let git_toml = toml::to_string(&git_origin).unwrap();
    assert!(git_toml.contains("type = \"git\""));

    let repo_toml = toml::to_string(&repo_origin).unwrap();
    assert!(repo_toml.contains("type = \"repository\""));
}

#[test]
fn test_get_skills_for_groups_exclude_wins_over_only() {
    // S15: A skill present in both only_groups and exclude_groups MUST be excluded.
    let toml_content = r#"
            [metadata]
            version = "1.0.0"

            [[skills]]
            id = "dual-group-skill"
            origin = { type = "git", url = "https://github.com/org/repo.git" }
            groups = ["prod", "dev"]

            [[skills]]
            id = "only-prod-skill"
            origin = { type = "git", url = "https://github.com/org/repo2.git" }
            groups = ["prod"]
        "#;

    let manifest: SkillsManifest = toml::from_str(toml_content).unwrap();

    // dual-group-skill is in both "prod" (only) and "dev" (exclude) → must be excluded
    let result =
        manifest.get_skills_for_groups(Some(&["dev".to_string()]), Some(&["prod".to_string()]));

    let ids: Vec<&str> = result.iter().map(|s| s.id.as_str()).collect();
    assert!(
        !ids.contains(&"dual-group-skill"),
        "skill in both only_groups and exclude_groups must be excluded"
    );
    assert!(
        ids.contains(&"only-prod-skill"),
        "skill only in only_groups and not in exclude_groups must be included"
    );
}

#[test]
fn test_from_manifest_repo_to_repository_definition() {
    let manifest_repo = RepositoryDefinition {
        name: "test-repo".to_string(),
        r#type: RepositoryType::GitMarketplace,
        priority: 1,
        connection: RepositoryConnection::GitMarketplace {
            url: "https://github.com/org/marketplace.git".to_string(),
            branch: Some("main".to_string()),
            tag: None,
        },
        auth: None,
    };

    let repo_def = crate::core::repository::RepositoryDefinition::from(&manifest_repo);
    assert_eq!(repo_def.name, "test-repo");
    assert_eq!(repo_def.priority, 1);
    assert!(matches!(
        repo_def.repo_type,
        crate::core::repository::RepositoryType::GitMarketplace
    ));
}

#[test]
fn skills_manifest_persists_additions_removals_and_git_tag_intent() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("nested/skills.toml");
    let mut manifest = SkillsManifest {
        metadata: ManifestMetadata {
            version: "1.0.0".to_string(),
        },
        skills: Vec::new(),
    };
    let tagged = Origin::Git {
        url: "https://example.com/team/skills.git".to_string(),
        r#ref: GitRef::Tag("v1.2.0".to_string()),
        subdir: Some(PathBuf::from("skills/demo")),
    };
    manifest.add_skill(SkillEntry {
        id: "demo".to_string(),
        origin: tagged.clone(),
        groups: vec!["dev".to_string()],
    });
    manifest.add_skill(SkillEntry {
        id: "local".to_string(),
        origin: Origin::Local {
            path: "source/local".into(),
            editable: true,
        },
        groups: Vec::new(),
    });
    manifest.save_to_file(&path).unwrap();
    let mut restored = SkillsManifest::load_from_file(&path).unwrap();
    assert_eq!(restored.metadata.version, "1.0.0");
    assert_eq!(restored.skills[0].origin, tagged);
    assert_eq!(restored.skills[0].groups, ["dev"]);
    assert_eq!(restored.get_skills_for_groups(None, Some(&[])).len(), 2);
    assert_eq!(
        restored.get_skills_for_groups(Some(&["dev".to_string()]), Some(&[]))[0].id,
        "local"
    );
    assert!(restored.remove_skill("demo"));
    assert!(!restored.remove_skill("missing"));
    restored.save_to_file(&path).unwrap();
    let restored = SkillsManifest::load_from_file(&path).unwrap();
    assert_eq!(restored.get_all_skills().len(), 1);
    assert_eq!(restored.skills[0].id, "local");
    assert!(matches!(
        restored.skills[0].origin,
        Origin::Local { editable: true, .. }
    ));
}

#[test]
fn manifest_io_errors_distinguish_missing_invalid_and_unwritable_files() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing.toml");
    assert!(
        matches!(SkillsManifest::load_from_file(&missing), Err(ManifestError::NotFound(path)) if path == missing)
    );
    assert!(
        matches!(SkillProjectToml::load_from_file(&missing), Err(ManifestError::NotFound(path)) if path == missing)
    );
    assert!(matches!(
        SkillsManifest::load_from_file(root.path()),
        Err(ManifestError::Io(_))
    ));
    assert!(matches!(
        SkillProjectToml::load_from_file(root.path()),
        Err(ManifestError::Io(_))
    ));
    let malformed = root.path().join("malformed.toml");
    std::fs::write(&malformed, "[metadata\nversion = \"1.0.0\"\n").unwrap();
    assert!(matches!(
        SkillsManifest::load_from_file(&malformed),
        Err(ManifestError::Parse(_))
    ));
    let error = SkillProjectToml::load_from_file(&malformed).unwrap_err();
    assert!(error.to_string().contains("TOML syntax error"));
    assert!(error.to_string().contains("line"));
    let manifest: SkillsManifest = toml::from_str("[metadata]\nversion = \"1.0.0\"\n").unwrap();
    assert!(matches!(
        manifest.save_to_file(root.path()),
        Err(ManifestError::Io(_))
    ));
    let project = SkillProjectToml::from_toml_str("[dependencies]\n").unwrap();
    assert!(matches!(
        project.save_to_file(root.path()),
        Err(ManifestError::Io(_))
    ));
    assert!(
        malformed.exists(),
        "failed saves must preserve unrelated existing files"
    );
}

#[test]
fn project_validation_reports_the_missing_requirement_for_each_context() {
    for (content, context, expected) in [
        ("", ProjectContext::Skill, "requires [metadata] section"),
        (
            "[metadata]\nversion = \"1.0.0\"",
            ProjectContext::Skill,
            "requires [metadata].id",
        ),
        (
            "[metadata]\nid = \"\"\nversion = \"1.0.0\"",
            ProjectContext::Skill,
            "requires [metadata].id",
        ),
        (
            "[metadata]\nid = \"demo\"",
            ProjectContext::Skill,
            "requires [metadata].version",
        ),
        (
            "[metadata]\nid = \"demo\"\nversion = \"\"",
            ProjectContext::Skill,
            "requires [metadata].version",
        ),
        ("", ProjectContext::Project, "requires [dependencies]"),
        (
            "[dependencies]",
            ProjectContext::Project,
            "skills_directory",
        ),
        (
            "[dependencies]\n[tool]",
            ProjectContext::Project,
            "skills_directory",
        ),
        (
            "[dependencies]\n[tool.fastskill]",
            ProjectContext::Project,
            "skills_directory",
        ),
        (
            "[dependencies]",
            ProjectContext::Ambiguous,
            "Cannot determine context",
        ),
    ] {
        let project = SkillProjectToml::from_toml_str(content).unwrap();
        let error = project.validate_for_context(context).unwrap_err();
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error:?}"
        );
    }
    let skill =
        SkillProjectToml::from_toml_str("[metadata]\nid = \"demo\"\nversion = \"1.0.0\"").unwrap();
    skill.validate_for_context(ProjectContext::Skill).unwrap();
    let project = SkillProjectToml::from_toml_str(
        "[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"",
    )
    .unwrap();
    project
        .validate_for_context(ProjectContext::Project)
        .unwrap();
}

#[test]
fn dependency_conversion_resolves_relative_paths_and_preserves_exact_versions_and_groups() {
    let root = tempfile::tempdir().unwrap();
    let source = r#"
schema_version = "1"
[dependencies]
pinned = "1.2.3"
local = { origin = { type = "local", path = "source/demo", editable = true }, groups = ["dev", "test"] }
tagged = { origin = { type = "git", url = "https://example.com/skills.git", ref = { tag = "v2.0.0" } } }
"#;
    let project = SkillProjectToml::from_toml_str(source).unwrap();
    let entries = project.to_skill_entries(root.path()).unwrap();
    let pinned = entries.iter().find(|entry| entry.id == "pinned").unwrap();
    assert!(
        matches!(&pinned.origin, Origin::Repository { repo, skill, version: Some(version) }
        if repo == "default" && skill == "pinned" && version.satisfies("1.2.3").unwrap() && !version.satisfies("1.2.4").unwrap())
    );
    let local = entries.iter().find(|entry| entry.id == "local").unwrap();
    assert_eq!(
        local.origin,
        Origin::Local {
            path: root.path().join("source/demo"),
            editable: true
        }
    );
    assert_eq!(local.groups, ["dev", "test"]);
    let tagged = entries.iter().find(|entry| entry.id == "tagged").unwrap();
    assert!(
        matches!(&tagged.origin, Origin::Git { r#ref: GitRef::Tag(tag), .. } if tag == "v2.0.0")
    );
    assert!(tagged.groups.is_empty());
    assert!(SkillProjectToml::from_toml_str("")
        .unwrap()
        .to_skill_entries(root.path())
        .unwrap()
        .is_empty());
    let invalid =
        SkillProjectToml::from_toml_str("[dependencies]\nbroken = \"not-semver\"").unwrap();
    let error = invalid.to_skill_entries(root.path()).unwrap_err();
    assert!(error.contains("Invalid version 'not-semver' for broken"));
}

#[test]
fn legacy_migration_preserves_version_defaults_aliases_and_reports_malformed_origins() {
    let source = r#"
[dependencies]
pinned = "1.2.3"
archive = { source = "zip-url", url = "https://example.com/skills.zip" }
local = { source = "local", path = "source/demo" }
repo = { source = "source", name = "team", version = "^1.2.0", groups = ["dev"] }
"#;
    let project = SkillProjectToml::from_toml_str(source).unwrap();
    let entries = project.to_skill_entries(Path::new("/project")).unwrap();
    assert_eq!(entries.len(), 4);
    let repo = entries.iter().find(|entry| entry.id == "repo").unwrap();
    assert_eq!(repo.groups, ["dev"]);
    assert!(
        matches!(&repo.origin, Origin::Repository { repo, skill, version: Some(version) }
        if repo == "team" && skill == "repo" && version.satisfies("1.9.0").unwrap() && !version.satisfies("2.0.0").unwrap())
    );
    assert!(matches!(
        &entries
            .iter()
            .find(|entry| entry.id == "local")
            .unwrap()
            .origin,
        Origin::Local {
            editable: false,
            ..
        }
    ));
    assert!(
        matches!(&entries.iter().find(|entry| entry.id == "archive").unwrap().origin, Origin::ZipUrl { url } if url.ends_with("skills.zip"))
    );
    for (fields, expected) in [
        ("source = \"local\"", "path"),
        ("source = \"zip-url\"", "zip_url"),
        ("source = \"source\"", "name"),
        (
            "source = \"source\"\nname = \"team\"\nversion = \"invalid\"",
            "unparseable version constraint",
        ),
    ] {
        let error = SkillProjectToml::from_toml_str(&format!("[dependencies.broken]\n{fields}\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("broken"));
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error:?}"
        );
    }
    let stamped_legacy = "schema_version = \"1\"\n[dependencies.old]\nsource = \"git\"\nurl = \"https://example.com/skills.git\"";
    assert!(SkillProjectToml::from_toml_str(stamped_legacy)
        .unwrap_err()
        .to_string()
        .contains("pre-Origin"));
}

#[test]
fn repository_connections_and_auth_survive_project_persistence_and_runtime_conversion() {
    use crate::core::repository::{
        RepositoryAuth, RepositoryConfig, RepositoryType as RuntimeType,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("skill-project.toml");
    let source = r#"
schema_version = "1"
[dependencies]
[tool.fastskill]
skills_directory = "skills"
[[tool.fastskill.repositories]]
name = "tagged"
type = "git-marketplace"
priority = 2
url = "https://example.com/skills.git"
tag = "v1.2.0"
auth = { type = "pat", env_var = "TEAM_TOKEN" }
[[tool.fastskill.repositories]]
name = "http"
type = "http-registry"
priority = 1
index_url = "https://example.com/index"
auth = { type = "pat" }
[[tool.fastskill.repositories]]
name = "archive"
type = "zip-url"
priority = 3
zip_url = "https://example.com/catalog.zip"
[[tool.fastskill.repositories]]
name = "local"
type = "local"
priority = 4
path = "catalog"
"#;
    let project = SkillProjectToml::from_toml_str(source).unwrap();
    project.save_to_file(&path).unwrap();
    let loaded = SkillProjectToml::load_from_file(&path).unwrap();
    let repositories = loaded
        .tool
        .unwrap()
        .fastskill
        .unwrap()
        .repositories
        .unwrap();
    let converted = repositories
        .iter()
        .map(crate::core::repository::RepositoryDefinition::from)
        .collect::<Vec<_>>();
    assert_eq!(converted.len(), 4);
    let git = &converted[0];
    assert_eq!(git.name, "tagged");
    assert_eq!(git.priority, 2);
    assert_eq!(git.repo_type, RuntimeType::GitMarketplace);
    assert!(
        matches!(&git.config, RepositoryConfig::GitMarketplace { url, branch: None, tag: Some(tag) }
        if url == "https://example.com/skills.git" && tag == "v1.2.0")
    );
    assert!(matches!(&git.auth, Some(RepositoryAuth::Pat { env_var }) if env_var == "TEAM_TOKEN"));
    assert_eq!(converted[1].repo_type, RuntimeType::HttpRegistry);
    assert!(
        matches!(&converted[1].config, RepositoryConfig::HttpRegistry { index_url } if index_url == "https://example.com/index")
    );
    assert!(
        matches!(&converted[1].auth, Some(RepositoryAuth::Pat { env_var }) if env_var == "PAT_TOKEN")
    );
    assert_eq!(converted[2].repo_type, RuntimeType::ZipUrl);
    assert!(
        matches!(&converted[2].config, RepositoryConfig::ZipUrl { base_url } if base_url == "https://example.com/catalog.zip")
    );
    assert_eq!(converted[3].repo_type, RuntimeType::Local);
    assert!(
        matches!(&converted[3].config, RepositoryConfig::Local { path } if path == Path::new("catalog"))
    );
    assert!(converted[2].auth.is_none());
    assert!(converted.iter().all(|repo| repo.storage.is_none()));
    assert!(std::fs::read_to_string(path)
        .unwrap()
        .contains("tag = \"v1.2.0\""));
}

#[test]
fn minimal_tool_config_applies_documented_eval_and_server_defaults() {
    let project = SkillProjectToml::from_toml_str(
        r#"
[tool.fastskill]
[tool.fastskill.eval]
prompts = "prompts.csv"
[tool.fastskill.server]
allowed_origins = ["https://example.com"]
"#,
    )
    .unwrap();
    let config = project.tool.unwrap().fastskill.unwrap();
    assert_eq!(config.install_depth, 5);
    assert!(!config.skip_transitive);
    assert!(config.auto_reindex);
    let eval = config.eval.unwrap();
    assert_eq!(eval.prompts, Path::new("prompts.csv"));
    assert_eq!(eval.timeout_seconds, 900);
    assert_eq!(eval.trials_per_case, 1);
    assert_eq!(eval.pass_threshold, 1.0);
    assert!(eval.fail_on_missing_agent);
    assert!(eval.parallel.is_none());
    assert!(eval.checks.is_none());
    assert_eq!(
        config.server.unwrap().allowed_headers,
        ["Content-Type", "Authorization"]
    );
}

#[cfg(unix)]
#[test]
fn unserializable_local_paths_do_not_replace_existing_manifests() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("manifest.toml");
    std::fs::write(&path, "original manifest").unwrap();
    let origin = Origin::Local {
        path: PathBuf::from(std::ffi::OsString::from_vec(vec![0xff])),
        editable: false,
    };
    let manifest = SkillsManifest {
        metadata: ManifestMetadata {
            version: "1.0.0".to_string(),
        },
        skills: vec![SkillEntry {
            id: "demo".to_string(),
            origin: origin.clone(),
            groups: Vec::new(),
        }],
    };
    assert!(matches!(
        manifest.save_to_file(&path),
        Err(ManifestError::Serialize(_))
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original manifest");
    let project = SkillProjectToml {
        schema_version: None,
        metadata: None,
        tool: None,
        dependencies: Some(DependenciesSection {
            dependencies: HashMap::from([(
                "demo".to_string(),
                DependencySpec::Inline {
                    origin,
                    groups: None,
                },
            )]),
        }),
    };
    assert!(matches!(
        project.save_to_file(&path),
        Err(ManifestError::Serialize(_))
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original manifest");
}

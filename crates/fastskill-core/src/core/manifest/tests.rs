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

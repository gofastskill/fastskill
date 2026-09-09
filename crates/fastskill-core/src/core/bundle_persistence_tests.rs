use super::*;
use crate::core::lock::{ProjectLockedBundleEntry, ProjectLockedBundleMember};

fn bundle_manifest(format: &str, version: &str, members: &str, dependencies: &str) -> String {
    format!(
            "[bundle]\nformat = {format:?}\nid = \"team\"\nversion = {version:?}\n{members}\n{dependencies}"
        )
}

fn override_fixture() -> (TempDir, BundleService, PathBuf) {
    let root = TempDir::new().unwrap();
    let skills = root.path().join("skills");
    let packaged = skills.join("demo");
    let source = root.path().join("personal/demo");
    fs::create_dir_all(&packaged).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(
        packaged.join("SKILL.md"),
        "---\nname: demo\nversion: 1.0.0\ndescription: packaged\n---\npackaged\n",
    )
    .unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\nversion: 2.0.0\ndescription: personal\n---\npersonal\n",
    )
    .unwrap();
    fs::write(root.path().join("skill-project.toml"), "").unwrap();
    let packaged_digest = digest_directory(&packaged).unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    lock.bundles.push(ProjectLockedBundleEntry {
        id: "team".to_string(),
        version: "1.0.0".to_string(),
        artifact: ".fastskill/bundles/team-1.0.0.zip".to_string(),
        digest: "release".to_string(),
        members: vec![ProjectLockedBundleMember {
            id: "demo".to_string(),
            digest: packaged_digest,
            overridable: true,
        }],
    });
    lock.save_to_file(&root.path().join("skills.lock")).unwrap();
    let service = BundleService::new(root.path(), skills);
    (root, service, source)
}

#[test]
fn bundle_project_parser_reports_each_invalid_manifest_contract() {
    let non_utf8 = parse_bundle_project(&[0xff]).unwrap_err().to_string();
    assert!(non_utf8.contains("not UTF-8"));
    assert!(parse_bundle_project(b"invalid = [")
        .unwrap_err()
        .to_string()
        .contains("Invalid bundle skill-project.toml"));
    assert!(parse_bundle_project(b"[dependencies]\n")
        .unwrap_err()
        .to_string()
        .contains("no [bundle] declaration"));
    assert!(parse_bundle_project(
        bundle_manifest(
            "wrong",
            "1.0.0",
            "[bundle.members.demo]",
            "demo = \"1.0.0\""
        )
        .as_bytes()
    )
    .unwrap_err()
    .to_string()
    .contains("supported FastSkill bundle"));
    assert!(parse_bundle_project(
        bundle_manifest(
            BUNDLE_FORMAT,
            "not-semver",
            "[bundle.members.demo]",
            "demo = \"1.0.0\""
        )
        .as_bytes()
    )
    .unwrap_err()
    .to_string()
    .contains("not valid SemVer"));
    assert!(parse_bundle_project(
        bundle_manifest(BUNDLE_FORMAT, "1.0.0", "", "demo = \"1.0.0\"").as_bytes()
    )
    .unwrap_err()
    .to_string()
    .contains("at least one member"));
    assert!(parse_bundle_project(
        bundle_manifest(BUNDLE_FORMAT, "1.0.0", "[bundle.members.demo]", "").as_bytes()
    )
    .unwrap_err()
    .to_string()
    .contains("not declared in [dependencies]"));
}

#[test]
fn member_preparation_reports_missing_content_and_invalid_dependency_metadata() {
    let root = TempDir::new().unwrap();
    let descriptor = BundleDescriptor {
        format_marker: BUNDLE_FORMAT.to_string(),
        id: "team".to_string(),
        version: "1.0.0".to_string(),
        members: BTreeMap::from([("demo".to_string(), BundleMemberPolicy::default())]),
    };
    let dependencies = BTreeMap::from([(
        "demo".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    )]);
    assert!(prepare_members(root.path(), &descriptor, &dependencies)
        .unwrap_err()
        .to_string()
        .contains("missing skills/demo/SKILL.md"));

    let demo = root.path().join("demo");
    fs::create_dir_all(&demo).unwrap();
    fs::write(
        demo.join("SKILL.md"),
        "---\nname: demo\nversion: 1.0.0\ndescription: demo\n---\n",
    )
    .unwrap();
    fs::write(demo.join("skill-project.toml"), "invalid = [").unwrap();
    assert!(prepare_members(root.path(), &descriptor, &dependencies)
        .unwrap_err()
        .to_string()
        .contains("Invalid dependency manifest"));
    fs::remove_file(demo.join("skill-project.toml")).unwrap();

    let invalid_constraint = BTreeMap::from([(
        "demo".to_string(),
        DependencySpec::Version("not-a-version".to_string()),
    )]);
    assert!(
        prepare_members(root.path(), &descriptor, &invalid_constraint)
            .unwrap_err()
            .to_string()
            .contains("invalid version constraint")
    );
    fs::write(demo.join("SKILL.md"), "no frontmatter").unwrap();
    assert!(prepare_members(root.path(), &descriptor, &dependencies)
        .unwrap_err()
        .to_string()
        .contains("invalid SKILL.md frontmatter"));
}

#[test]
fn member_preparation_expands_transitive_and_inline_origins_and_checks_versions() {
    let root = TempDir::new().unwrap();
    for id in ["root", "child", "local"] {
        let skill = root.path().join(id);
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {id}\nversion: 1.0.0\ndescription: {id}\n---\n"),
        )
        .unwrap();
    }
    fs::write(
        root.path().join("root/skill-project.toml"),
        "[dependencies.child]\norigin = { type = \"repository\", repo = \"main\", skill = \"child\", version = \"1.0.0\" }\n",
    )
    .unwrap();
    let descriptor = BundleDescriptor {
        format_marker: BUNDLE_FORMAT.to_string(),
        id: "team".to_string(),
        version: "1.0.0".to_string(),
        members: BTreeMap::from([
            ("root".to_string(), BundleMemberPolicy::default()),
            ("local".to_string(), BundleMemberPolicy::default()),
        ]),
    };
    let dependencies = BTreeMap::from([
        (
            "root".to_string(),
            DependencySpec::Inline {
                origin: Origin::Repository {
                    repo: "main".to_string(),
                    skill: "root".to_string(),
                    version: Some(VersionConstraint::parse("1.0.0").unwrap()),
                },
                groups: None,
            },
        ),
        (
            "local".to_string(),
            DependencySpec::Inline {
                origin: Origin::Local {
                    path: root.path().join("local"),
                    editable: false,
                },
                groups: None,
            },
        ),
    ]);

    let members = prepare_members(root.path(), &descriptor, &dependencies).unwrap();
    assert_eq!(members.len(), 3);
    assert!(members.contains_key("child"));

    fs::write(
        root.path().join("root/SKILL.md"),
        "---\nname: root\nversion: invalid\ndescription: root\n---\n",
    )
    .unwrap();
    assert!(prepare_members(root.path(), &descriptor, &dependencies)
        .unwrap_err()
        .to_string()
        .contains("invalid version"));
    fs::write(
        root.path().join("root/SKILL.md"),
        "---\nname: root\nversion: 2.0.0\ndescription: root\n---\n",
    )
    .unwrap();
    assert!(prepare_members(root.path(), &descriptor, &dependencies)
        .unwrap_err()
        .to_string()
        .contains("does not satisfy"));
}

#[test]
fn release_digest_changes_for_policy_identity_version_and_content() {
    let descriptor = BundleDescriptor {
        format_marker: BUNDLE_FORMAT.to_string(),
        id: "team".to_string(),
        version: "1.0.0".to_string(),
        members: BTreeMap::new(),
    };
    let required = BTreeMap::from([(
        "demo".to_string(),
        BundleArchiveLockMember {
            digest: "content".to_string(),
            overridable: false,
        },
    )]);
    let mut overridable = required.clone();
    overridable.get_mut("demo").unwrap().overridable = true;
    assert_ne!(
        digest_release(&descriptor, &required),
        digest_release(&descriptor, &overridable)
    );
    let mut next = descriptor.clone();
    next.version = "2.0.0".to_string();
    assert_ne!(
        digest_release(&descriptor, &required),
        digest_release(&next, &required)
    );
}

#[test]
fn transaction_rollback_restores_existing_and_removes_new_state() {
    let root = TempDir::new().unwrap();
    let skills = root.path().join("skills");
    let existing = skills.join("existing");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("SKILL.md"), "before").unwrap();
    let state = root.path().join("state.toml");
    fs::write(&state, "before").unwrap();
    let new_state = root.path().join("new-state.toml");
    let ids = vec!["existing".to_string(), "new".to_string()];
    let mut transaction =
        BundleTransaction::capture(&skills, &ids, &[state.clone(), new_state.clone()]).unwrap();
    fs::write(existing.join("SKILL.md"), "after").unwrap();
    fs::create_dir_all(skills.join("new")).unwrap();
    fs::write(skills.join("new/SKILL.md"), "new").unwrap();
    fs::write(&state, "after").unwrap();
    fs::write(&new_state, "new").unwrap();

    transaction.rollback().unwrap();

    assert_eq!(
        fs::read_to_string(existing.join("SKILL.md")).unwrap(),
        "before"
    );
    assert!(!skills.join("new").exists());
    assert_eq!(fs::read_to_string(state).unwrap(), "before");
    assert!(!new_state.exists());
}

#[test]
fn declaration_writer_adds_and_removes_only_owned_tables() {
    let root = TempDir::new().unwrap();
    let manifest = root.path().join("skill-project.toml");
    fs::write(
        &manifest,
        "[dependencies]\nkeep = \"1.0.0\"\n[extension]\nflag = true\n",
    )
    .unwrap();
    let bundles = BTreeMap::from([(
        "team".to_string(),
        BundleDependency {
            version: "1.0.0".to_string(),
            artifact: "team.zip".to_string(),
        },
    )]);
    let overrides = BTreeMap::from([(
        "demo".to_string(),
        BundleOverrideDeclaration {
            origin: "personal/demo".to_string(),
        },
    )]);

    save_bundle_declarations(&manifest, &bundles, &overrides).unwrap();
    let with_tables = fs::read_to_string(&manifest).unwrap();
    assert!(with_tables.contains("[bundles.team]"));
    assert!(with_tables.contains("[overrides.demo]"));
    assert!(with_tables.contains("[extension]"));

    save_bundle_declarations(&manifest, &BTreeMap::new(), &BTreeMap::new()).unwrap();
    let without_tables = fs::read_to_string(manifest).unwrap();
    assert!(!without_tables.contains("[bundles"));
    assert!(!without_tables.contains("[overrides"));
    assert!(without_tables.contains("[extension]"));
}

#[test]
fn directory_helpers_stage_replacements_and_refuse_non_directories() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("skills/demo");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "replacement").unwrap();
    replace_skill_directory(&destination, &source).unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("SKILL.md")).unwrap(),
        "replacement"
    );
    remove_skill_directory(&destination).unwrap();
    remove_skill_directory(&destination).unwrap();
    fs::write(&destination, "not a directory").unwrap();
    assert!(matches!(
        remove_skill_directory(&destination),
        Err(ServiceError::Validation(_))
    ));
    assert!(replace_skill_directory(Path::new("/"), &source).is_err());
}

#[cfg(unix)]
#[test]
fn digest_and_copy_reject_links_inside_managed_content() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "demo").unwrap();
    std::os::unix::fs::symlink(source.join("SKILL.md"), source.join("linked")).unwrap();
    assert!(matches!(
        digest_directory(&source),
        Err(ServiceError::Validation(_))
    ));
    assert!(matches!(
        copy_directory(&source, &root.path().join("copy")),
        Err(ServiceError::Validation(_))
    ));
    assert!(digest_directory(&root.path().join("missing")).is_err());
}

#[test]
fn personal_override_preview_apply_and_restore_preserve_consistent_state() {
    let (root, service, source) = override_fixture();
    let preview = preview_personal_override(&service, "demo", &source).unwrap();
    assert!(preview.changed);
    apply_personal_override(&service, "demo", &source).unwrap();
    apply_personal_override(&service, "demo", &source).unwrap();
    let unchanged = preview_personal_override(&service, "demo", &source).unwrap();
    assert!(!unchanged.changed);
    assert!(unchanged.changes.is_empty());

    fs::remove_dir_all(service.skills_directory.join("demo")).unwrap();
    restore_personal_overrides(&service).unwrap();
    assert!(
        fs::read_to_string(service.skills_directory.join("demo/SKILL.md"))
            .unwrap()
            .contains("personal")
    );

    let mut lock = ProjectSkillsLock::load_from_file(&root.path().join("skills.lock")).unwrap();
    lock.overrides[0].origin = root.path().join("different").display().to_string();
    lock.save_to_file(&root.path().join("skills.lock")).unwrap();
    let mismatch = restore_personal_overrides(&service).unwrap_err();
    assert!(mismatch
        .to_string()
        .contains("different Manifest and Lock origins"));
    assert!(!root.path().join(".fastskill/recovery-required").exists());

    lock.overrides.clear();
    lock.save_to_file(&root.path().join("skills.lock")).unwrap();
    let missing = restore_personal_overrides(&service).unwrap_err();
    assert!(missing.to_string().contains("missing from skills.lock"));
    assert!(!root.path().join(".fastskill/recovery-required").exists());
}

#[test]
fn guarded_override_restore_uses_the_callers_writer_lease() {
    let (_root, service, source) = override_fixture();
    apply_personal_override(&service, "demo", &source).unwrap();
    fs::remove_dir_all(service.skills_directory.join("demo")).unwrap();
    let guard = StateMutationGuard::acquire_for(
        &service.project_root,
        Some(&service.skills_directory),
        "test restore",
    )
    .unwrap();

    restore_personal_overrides_with_guard(&service, &guard).unwrap();
    guard.commit().unwrap();

    assert!(service.skills_directory.join("demo/SKILL.md").is_file());
}

#[test]
fn personal_override_validation_reports_owner_and_declaration_conflicts() {
    let (root, service, source) = override_fixture();
    let lock_path = root.path().join("skills.lock");
    let manifest_path = root.path().join("skill-project.toml");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();

    lock.bundles[0].members[0].overridable = false;
    lock.save_to_file(&lock_path).unwrap();
    assert!(preview_personal_override(&service, "demo", &source)
        .unwrap_err()
        .to_string()
        .contains("must permit"));

    lock.bundles[0].members[0].overridable = true;
    let mut second = lock.bundles[0].clone();
    second.id = "other".to_string();
    second.members[0].digest = "different".to_string();
    lock.bundles.push(second);
    lock.save_to_file(&lock_path).unwrap();
    assert!(preview_personal_override(&service, "demo", &source)
        .unwrap_err()
        .to_string()
        .contains("disagree"));

    lock.bundles.clear();
    lock.save_to_file(&lock_path).unwrap();
    assert!(preview_personal_override(&service, "demo", &source)
        .unwrap_err()
        .to_string()
        .contains("not owned"));

    fs::write(&manifest_path, "invalid = [").unwrap();
    assert!(preview_personal_override(&service, "demo", &source).is_err());
}

#[test]
fn override_validation_rejects_missing_sources_and_inconsistent_state() {
    let (root, service, source) = override_fixture();
    assert!(preview_personal_override(&service, "../escape", &source).is_err());
    assert!(preview_personal_override(&service, "demo", &root.path().join("missing")).is_err());

    fs::write(
        root.path().join("skill-project.toml"),
        format!(
            "[overrides.demo]\norigin = {:?}\n",
            source.display().to_string()
        ),
    )
    .unwrap();
    let mismatch = preview_personal_override(&service, "demo", &source).unwrap_err();
    assert!(mismatch.to_string().contains("inconsistent"));

    fs::write(root.path().join("skill-project.toml"), "invalid = [").unwrap();
    assert!(restore_personal_overrides(&service).is_err());
}

#[test]
fn override_apply_revalidates_every_input_after_preview_and_writer_acquisition() {
    assert!(!take_override_test_change(99));
    for mode in 1..=7 {
        let (root, service, source) = override_fixture();
        OVERRIDE_TEST_CHANGE.with(|change| change.set(mode));
        let error = apply_personal_override(&service, "demo", &source).unwrap_err();
        assert!(!error.to_string().is_empty(), "mode {mode}");
        assert!(!root.path().join(".fastskill/recovery-required").exists());
    }
}

#[test]
fn override_restore_revalidates_manifest_lock_and_ownership_under_lease() {
    for mode in 8..=10 {
        let (root, service, source) = override_fixture();
        apply_personal_override(&service, "demo", &source).unwrap();
        OVERRIDE_TEST_CHANGE.with(|change| change.set(mode));
        let error = restore_personal_overrides(&service).unwrap_err();
        assert!(!error.to_string().is_empty(), "mode {mode}");
        assert!(!root.path().join(".fastskill/recovery-required").exists());
    }
}

#[test]
fn descriptor_and_declaration_parsers_report_typed_and_editing_errors() {
    assert!(parse_bundle_descriptor(
        b"[bundle]\nformat = \"fastskill-bundle-v1\"\nid = 4\nversion = \"1.0.0\""
    )
    .is_err());
    let root = TempDir::new().unwrap();
    let manifest = root.path().join("skill-project.toml");
    fs::write(&manifest, "invalid = [").unwrap();
    assert!(save_bundle_declarations(&manifest, &BTreeMap::new(), &BTreeMap::new()).is_err());
}

use super::*;
use crate::core::manifest::SkillProjectToml;

fn fixture() -> (TempDir, BundleService) {
    let root = TempDir::new().unwrap();
    SkillProjectToml {
        schema_version: None,
        metadata: None,
        dependencies: None,
        tool: None,
    }
    .save_to_file(&root.path().join("skill-project.toml"))
    .unwrap();
    ProjectSkillsLock::new_empty()
        .save_to_file(&root.path().join("skills.lock"))
        .unwrap();
    let service = BundleService::new(root.path(), root.path().join("skills"));
    (root, service)
}

#[test]
fn state_loaders_report_missing_or_malformed_authoritative_files() {
    let (root, service) = fixture();
    fs::remove_file(root.path().join("skill-project.toml")).unwrap();
    assert!(service.load_project_state().is_err());

    SkillProjectToml {
        schema_version: None,
        metadata: None,
        dependencies: None,
        tool: None,
    }
    .save_to_file(&root.path().join("skill-project.toml"))
    .unwrap();
    fs::write(root.path().join("skills.lock"), "invalid = [").unwrap();
    assert!(service.load_project_state().is_err());

    ProjectSkillsLock::new_empty()
        .save_to_file(&root.path().join("skills.lock"))
        .unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[bundles.team]\nversion = 1\n",
    )
    .unwrap();
    assert!(service.load_project_state().is_err());

    fs::create_dir_all(root.path().join(".fastskill")).unwrap();
    fs::write(
        root.path().join(BUNDLE_HISTORY_FILE),
        "releases = { team = 1 }",
    )
    .unwrap();
    assert!(service.load_history().is_err());
    fs::remove_file(root.path().join(BUNDLE_HISTORY_FILE)).unwrap();
    assert!(service.load_history().unwrap().releases.is_empty());
}

#[test]
fn removal_and_persistence_revalidate_identity_and_project_state() {
    let (root, service) = fixture();
    assert!(service.prepare_remove("../escape").is_err());
    assert!(service
        .prepare_remove("missing")
        .err()
        .expect("missing bundle must fail")
        .to_string()
        .contains("not installed"));

    let (baseline_manifest, baseline_lock) = service.load_project_state().unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\nconcurrent = \"1.0.0\"\n",
    )
    .unwrap();
    let error = service
        .persist_transaction(
            &[],
            &[],
            &baseline_manifest,
            &baseline_lock,
            &baseline_manifest,
            &baseline_lock,
            None,
            None,
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("state changed"));
    assert!(!root.path().join(".fastskill/recovery-required").exists());
}

#[test]
fn project_state_comparison_includes_bundle_and_individual_ownership() {
    let (_root, service) = fixture();
    let (manifest, lock) = service.load_project_state().unwrap();
    assert!(same_project_state(&manifest, &lock, &manifest, &lock));

    let mut changed = lock.clone();
    changed.covered_roots.push("root".to_string());
    assert!(!same_project_state(&manifest, &changed, &manifest, &lock));
    changed = lock.clone();
    changed
        .overrides
        .push(crate::core::lock::ProjectLockedPersonalOverride {
            id: "demo".to_string(),
            origin: "personal/demo".to_string(),
            digest: "digest".to_string(),
        });
    assert!(!same_project_state(&manifest, &changed, &manifest, &lock));
}

fn built_artifact(root: &Path) -> PathBuf {
    let skills = root.join("skills");
    fs::create_dir_all(skills.join("demo")).unwrap();
    fs::write(
        skills.join("demo/SKILL.md"),
        "---\nname: demo\nversion: 1.0.0\ndescription: demo\n---\n",
    )
    .unwrap();
    fs::write(
        root.join("skill-project.toml"),
        "[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"1.0.0\"\n[bundle.members.demo]\n[dependencies]\ndemo = \"1.0.0\"\n",
    )
    .unwrap();
    BundleService::new(root, skills)
        .build(&root.join("dist"))
        .unwrap()
        .artifact
}

#[test]
fn apply_modes_reject_missing_wrong_identity_and_changed_known_release() {
    let author = TempDir::new().unwrap();
    let artifact = built_artifact(author.path());
    assert!(BundleService::is_bundle_artifact(&artifact).unwrap());

    let (recipient, service) = fixture();
    assert!(service
        .update("team", &artifact)
        .unwrap_err()
        .to_string()
        .contains("not installed"));
    assert!(service
        .apply(&artifact, ApplyMode::Restore { id: "other" }, None)
        .unwrap_err()
        .to_string()
        .contains("does not match artifact identity"));

    fs::create_dir_all(recipient.path().join(".fastskill")).unwrap();
    fs::write(
        recipient.path().join(BUNDLE_HISTORY_FILE),
        "[releases]\n\"team@1.0.0\" = \"different\"\n",
    )
    .unwrap();
    assert!(service
        .install(&artifact)
        .unwrap_err()
        .to_string()
        .contains("previously known digest"));
}

#[test]
fn guarded_declared_entrypoints_share_the_callers_lease() {
    let (root, service) = fixture();
    let guard = StateMutationGuard::acquire_for(
        root.path(),
        Some(&service.skills_directory),
        "combined test",
    )
    .unwrap();
    assert!(service
        .install_declared_with_guard(&guard)
        .unwrap()
        .is_empty());
    assert!(service
        .install_declared_locked_with_guard(&guard)
        .unwrap()
        .is_empty());
    guard.commit().unwrap();
}

#[test]
fn persistence_recovers_when_apply_fails_and_clears_pre_mutation_errors() {
    let (root, service) = fixture();
    let (manifest, lock) = service.load_project_state().unwrap();
    let missing = root.path().join("missing-source");
    let error = service
        .persist_transaction(
            &[],
            &[BundleReplacement {
                id: "demo".to_string(),
                source: missing,
            }],
            &manifest,
            &lock,
            &manifest,
            &lock,
            None,
            None,
            None,
        )
        .unwrap_err();
    assert!(matches!(error, ServiceError::Io(_)));
    assert!(!root.path().join(".fastskill/recovery-required").exists());

    fs::write(root.path().join("skill-project.toml"), "invalid = [").unwrap();
    let error = service
        .persist_transaction(
            &[],
            &[],
            &manifest,
            &lock,
            &manifest,
            &lock,
            None,
            None,
            None,
        )
        .unwrap_err();
    assert!(error.to_string().contains("Failed to load"));
    assert!(!root.path().join(".fastskill/recovery-required").exists());
}

#[test]
fn replacing_a_skill_copies_nested_content() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(source.join("references")).unwrap();
    fs::write(source.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
    fs::write(source.join("references/guide.md"), "guide").unwrap();
    let destination = ContainedPath::skill(&root.path().join("skills"), "demo").unwrap();

    replace_skill_directory(&destination, &source).unwrap();

    assert_eq!(
        fs::read_to_string(destination.as_path().join("references/guide.md")).unwrap(),
        "guide"
    );
}

#[test]
fn artifact_and_installed_content_validation_report_invalid_versions_and_local_edits() {
    let (root, service) = fixture();
    let descriptor = BundleDescriptor {
        format_marker: BUNDLE_FORMAT.to_string(),
        id: "team".to_string(),
        version: "not-semver".to_string(),
        members: Default::default(),
    };
    assert!(service.artifact_relative(&descriptor).is_err());
    assert!(service.ensure_unmodified("missing", "digest").is_ok());

    let installed = root.path().join("skills/demo");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join("SKILL.md"), "local edit").unwrap();
    assert!(service
        .ensure_unmodified("demo", "different")
        .unwrap_err()
        .to_string()
        .contains("locally modified"));
}

fn author_single_member_bundle() -> (TempDir, PathBuf) {
    let author = TempDir::new().unwrap();
    fs::write(
        author.path().join("skill-project.toml"),
        format!(
            "[bundle]\nformat = \"{BUNDLE_FORMAT}\"\nid = \"team\"\nversion = \"1.0.0\"\n\n\
             [bundle.members.demo]\noverridable = false\n\n[dependencies]\ndemo = \"1.0.0\"\n"
        ),
    )
    .unwrap();
    let skill = author.path().join("skills/demo");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: demo\nversion: 1.0.0\ndescription: demo\n---\ndemo\n",
    )
    .unwrap();
    let artifact = BundleService::new(author.path(), author.path().join("skills"))
        .build(author.path())
        .unwrap()
        .artifact;
    (author, artifact)
}

/// Rewrite `artifact` as a release built before ADR-0017: every digest in the legacy form.
fn legacy_artifact(artifact: &Path, destination: &Path) -> PreparedBundle {
    let prepared = PreparedBundle::load(artifact).unwrap();
    let extract = TempDir::new().unwrap();
    ZipHandler::new()
        .unwrap()
        .extract_to_dir(artifact, extract.path())
        .unwrap();
    let current = fs::read_to_string(extract.path().join("skills.lock")).unwrap();
    let member = &prepared.members["demo"];
    let legacy = current
        .replace(&member.digest.current, &member.digest.legacy)
        .replace(
            &prepared.release_digest.current,
            &prepared.release_digest.legacy,
        );
    assert_ne!(legacy, current);
    let legacy_lock: BundleArchiveLock = toml::from_str(&legacy).unwrap();
    let manifest = fs::read(extract.path().join("skill-project.toml")).unwrap();
    write_bundle_archive(destination, &manifest, &legacy_lock, &prepared.members).unwrap();
    prepared
}

fn record_legacy_digests(root: &Path, prepared: &PreparedBundle) {
    let lock_path = root.join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.bundles[0].digest = prepared.release_digest.legacy.clone();
    lock.bundles[0].members[0].digest = prepared.members["demo"].digest.legacy.clone();
    lock.save_to_file(&lock_path).unwrap();
    fs::write(
        root.join(BUNDLE_HISTORY_FILE),
        format!(
            "[releases]\n\"team@1.0.0\" = {:?}\n",
            prepared.release_digest.legacy
        ),
    )
    .unwrap();
}

#[test]
fn legacy_bundle_artifacts_and_records_keep_working_and_upgrade_on_rewrite() {
    let (author, artifact) = author_single_member_bundle();
    let old_artifact = author.path().join("team-1.0.0-legacy.zip");
    let prepared = legacy_artifact(&artifact, &old_artifact);
    let current_release = prepared.release_digest.current.clone();
    let current_member = prepared.members["demo"].digest.current.clone();
    assert!(current_member.starts_with(crate::core::content_digest::CONTENT_DIGEST_PREFIX));
    let (root, service) = fixture();
    let lock_path = root.path().join("skills.lock");

    // An artifact built by an older release installs, and the Lock records the current form.
    assert!(!service.install(&old_artifact).unwrap().unchanged);
    let lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(lock.bundles[0].digest, current_release);
    assert_eq!(lock.bundles[0].members[0].digest, current_member);

    // A Lock and history written by an older release still identify the same release.
    record_legacy_digests(root.path(), &prepared);
    assert!(service.install(&artifact).unwrap().unchanged);
    assert!(service.plan_install(&artifact).unwrap().changes.is_empty());
    let untouched = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(untouched.bundles[0].digest, prepared.release_digest.legacy);

    // Restoring rewrites the records, and the rewrite upgrades them.
    fs::remove_dir_all(root.path().join("skills/demo")).unwrap();
    service.install_declared_locked().unwrap();
    let upgraded = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(upgraded.bundles[0].digest, current_release);
    assert_eq!(upgraded.bundles[0].members[0].digest, current_member);
    assert_eq!(
        service.load_history().unwrap().releases["team@1.0.0"],
        current_release
    );

    // Edit protection still honours a legacy record, and still catches an edit.
    record_legacy_digests(root.path(), &prepared);
    fs::write(root.path().join("skills/demo/extra.md"), "edit").unwrap();
    assert!(service
        .remove("team")
        .unwrap_err()
        .to_string()
        .contains("locally modified"));
    fs::remove_file(root.path().join("skills/demo/extra.md")).unwrap();
    service.remove("team").unwrap();
    assert!(!root.path().join("skills/demo").exists());
}

#[test]
fn a_legacy_locked_release_that_names_other_contents_is_refused() {
    let (_author, artifact) = author_single_member_bundle();
    let (root, service) = fixture();
    service.install(&artifact).unwrap();
    let lock_path = root.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.bundles[0].members[0].digest = "0".repeat(64);
    lock.save_to_file(&lock_path).unwrap();
    fs::remove_dir_all(root.path().join("skills/demo")).unwrap();
    assert!(service.install_declared_locked().is_err());
}

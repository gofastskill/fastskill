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
    assert!(error.to_string().contains("No such file") || error.to_string().contains("I/O"));
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

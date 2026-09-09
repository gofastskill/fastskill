#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use fastskill_core::core::bundle::{BundleService, BUNDLE_FORMAT};
use fastskill_core::core::lock::{ProjectLockedSkillEntry, ProjectSkillsLock};
use fastskill_core::core::manifest::DependencySpec;
use fastskill_core::core::origin::{Origin, Resolved};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_skill(root: &Path, id: &str, marker: &str) {
    let skill = root.join("skills").join(id);
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {id}\nversion: \"1.0.0\"\ndescription: {marker}\n---\n{marker}\n"),
    )
    .unwrap();
    fs::write(
        skill.join("references/guide.md"),
        format!("guide: {marker}\n"),
    )
    .unwrap();
}

fn write_bundle_project(root: &Path, id: &str, version: &str, members: &[(&str, bool)]) {
    let members = members
        .iter()
        .map(|(member, overridable)| {
            format!("[bundle.members.{member}]\noverridable = {overridable}\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let dependencies = members
        .lines()
        .filter_map(|line| line.strip_prefix("[bundle.members."))
        .filter_map(|line| line.strip_suffix("]"))
        .map(|member| format!("{member} = \"1.0.0\""))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        root.join("skill-project.toml"),
        format!(
            "[bundle]\nformat = \"{BUNDLE_FORMAT}\"\nid = \"{id}\"\nversion = \"{version}\"\n\n{members}\n[dependencies]\n{dependencies}\n"
        ),
    )
    .unwrap();
}

fn author_bundle(id: &str, version: &str, members: &[(&str, bool)]) -> (TempDir, PathBuf) {
    let author = TempDir::new().unwrap();
    write_bundle_project(author.path(), id, version, members);
    for (member, _) in members {
        write_skill(author.path(), member, &format!("member-{member}"));
    }
    fs::write(author.path().join("unrelated.txt"), "do not package\n").unwrap();
    let output = TempDir::new().unwrap();
    let service = BundleService::new(author.path(), author.path().join("skills"));
    let built = service.build(output.path()).unwrap();
    let artifact = output.path().join(format!("{id}-{version}.zip"));
    assert_eq!(built.artifact, artifact);
    // Keep the archive alive after this helper returns.
    let persistent = author.path().join(format!("{id}-{version}.zip"));
    fs::copy(&artifact, &persistent).unwrap();
    (author, persistent)
}

fn recipient() -> (TempDir, BundleService) {
    let project = TempDir::new().unwrap();
    fs::create_dir_all(project.path().join(".claude/skills")).unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let service = BundleService::new(project.path(), project.path().join(".claude/skills"));
    (project, service)
}

#[test]
fn build_creates_a_versioned_self_contained_archive() {
    let (author, artifact) = author_bundle(
        "payments-team",
        "1.2.0",
        &[("code-review", false), ("payments-debugging", true)],
    );

    let extract = TempDir::new().unwrap();
    fastskill_core::storage::zip::ZipHandler::new()
        .unwrap()
        .extract_to_dir(&artifact, extract.path())
        .unwrap();

    assert!(extract.path().join("skill-project.toml").is_file());
    assert!(extract.path().join("skills/code-review/SKILL.md").is_file());
    assert!(extract
        .path()
        .join("skills/payments-debugging/references/guide.md")
        .is_file());
    assert!(!extract.path().join("unrelated.txt").exists());
    assert!(author.path().join("unrelated.txt").is_file());
}

#[test]
fn build_rejects_a_member_version_outside_the_declared_constraint() {
    let author = TempDir::new().unwrap();
    write_bundle_project(
        author.path(),
        "payments-team",
        "1.0.0",
        &[("code-review", false)],
    );
    write_skill(author.path(), "code-review", "review");
    fs::write(
        author.path().join("skills/code-review/SKILL.md"),
        "---\nname: code-review\nversion: \"9.0.0\"\ndescription: wrong version\n---\nreview\n",
    )
    .unwrap();

    let error = BundleService::new(author.path(), author.path().join("skills"))
        .build(author.path())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("does not satisfy declared dependency"),
        "{error}"
    );
    assert!(!author.path().join("payments-team-1.0.0.zip").exists());
}

#[test]
fn renamed_artifact_installs_members_and_records_the_bundle() {
    let (_author, artifact) = author_bundle("payments-team", "1.2.0", &[("code-review", false)]);
    let (project, service) = recipient();
    let renamed = project.path().join("download.zip");
    fs::copy(artifact, &renamed).unwrap();

    let installed = service.install(&renamed).unwrap();

    assert_eq!(installed.id, "payments-team");
    assert_eq!(installed.version, "1.2.0");
    assert_eq!(
        fs::read_to_string(
            project
                .path()
                .join(".claude/skills/code-review/SKILL.md"),
        )
        .unwrap(),
        "---\nname: code-review\nversion: \"1.0.0\"\ndescription: member-code-review\n---\nmember-code-review\n"
    );
    assert_eq!(service.list().unwrap().len(), 1);
    assert!(project.path().join("skills.lock").is_file());
}

#[test]
fn bundle_add_preview_uses_install_validation_and_never_mutates_state() {
    let (author, artifact) = author_bundle("payments-team", "1.2.0", &[("code-review", false)]);
    let (project, service) = recipient();
    let manifest_path = project.path().join("skill-project.toml");
    let before_manifest = fs::read(&manifest_path).unwrap();

    let preview = service.plan_install(&artifact).unwrap();
    assert_eq!(preview.id, "payments-team");
    assert_eq!(preview.current_revision, None);
    assert_eq!(preview.target_revision, "1.2.0");
    assert_eq!(preview.changes, vec!["add code-review", "lock", "manifest"]);
    assert!(preview.retained.is_empty());
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    assert!(!project.path().join("skills.lock").exists());
    assert!(!project.path().join(".claude/skills/code-review").exists());

    let (identical_project, identical_service) = recipient();
    let source = author.path().join("skills/code-review");
    let destination = identical_project.path().join(".claude/skills/code-review");
    fs::create_dir_all(destination.join("references")).unwrap();
    fs::copy(source.join("SKILL.md"), destination.join("SKILL.md")).unwrap();
    fs::copy(
        source.join("references/guide.md"),
        destination.join("references/guide.md"),
    )
    .unwrap();
    let identical = identical_service.plan_install(&artifact).unwrap();
    assert_eq!(identical.changes, vec!["lock", "manifest"]);
    assert_eq!(identical.retained, vec!["code-review"]);
    assert!(!identical_project.path().join("skills.lock").exists());

    service.install(&artifact).unwrap();
    let before_lock = fs::read(project.path().join("skills.lock")).unwrap();
    let unchanged = service.plan_install(&artifact).unwrap();
    assert_eq!(unchanged.current_revision.as_deref(), Some("1.2.0"));
    assert!(unchanged.changes.is_empty());
    assert_eq!(unchanged.retained, vec!["code-review"]);
    assert_eq!(
        fs::read(project.path().join("skills.lock")).unwrap(),
        before_lock
    );
}

#[test]
fn bundle_add_preview_rejects_the_same_conflicts_as_install_without_mutation() {
    let (_author, artifact) = author_bundle("payments-team", "1.2.0", &[("code-review", false)]);
    let (project, service) = recipient();
    let destination = project.path().join(".claude/skills/code-review");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("SKILL.md"), "unmanaged contents\n").unwrap();
    let manifest_path = project.path().join("skill-project.toml");
    let before_manifest = fs::read(&manifest_path).unwrap();

    let preview_error = service.plan_install(&artifact).unwrap_err().to_string();
    let install_error = service.install(&artifact).unwrap_err().to_string();

    assert!(preview_error.contains("already exists with different contents"));
    assert!(install_error.contains("already exists with different contents"));
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    assert!(!project.path().join("skills.lock").exists());
    assert_eq!(
        fs::read_to_string(destination.join("SKILL.md")).unwrap(),
        "unmanaged contents\n"
    );
}

#[test]
fn bundle_add_preview_enforces_installed_and_historical_release_identity() {
    let (_author, installed_artifact) =
        author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (_next_author, next_artifact) =
        author_bundle("payments-team", "2.0.0", &[("code-review", false)]);
    let (changed_author, _original_changed_artifact) =
        author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    fs::write(
        changed_author.path().join("skills/code-review/SKILL.md"),
        "---\nname: code-review\nversion: 1.0.0\ndescription: changed\n---\nchanged\n",
    )
    .unwrap();
    let changed_artifact =
        BundleService::new(changed_author.path(), changed_author.path().join("skills"))
            .build(changed_author.path())
            .unwrap()
            .artifact;
    let (_project, service) = recipient();
    service.install(&installed_artifact).unwrap();

    let version_error = service
        .plan_install(&next_artifact)
        .unwrap_err()
        .to_string();
    assert!(version_error.contains("already installed at 1.0.0"));
    let installed_digest_error = service
        .plan_install(&changed_artifact)
        .unwrap_err()
        .to_string();
    assert!(installed_digest_error.contains("installed digest"));

    service.remove("payments-team").unwrap();
    let historical_digest_error = service
        .plan_install(&changed_artifact)
        .unwrap_err()
        .to_string();
    assert!(historical_digest_error.contains("previously known digest"));
}

#[test]
fn identical_members_are_preserved_until_the_last_bundle_is_removed() {
    let (_first_author, first) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (_second_author, second) =
        author_bundle("security-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();

    service.install(&first).unwrap();
    service.install(&second).unwrap();
    service.remove("payments-team").unwrap();
    assert!(project
        .path()
        .join(".claude/skills/code-review/SKILL.md")
        .exists());

    service.remove("security-team").unwrap();
    assert!(!project.path().join(".claude/skills/code-review").exists());
}

#[test]
fn changed_contents_for_the_same_release_are_rejected_before_mutation() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let installed_before =
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap();

    let different = TempDir::new().unwrap();
    write_bundle_project(
        different.path(),
        "payments-team",
        "1.0.0",
        &[("code-review", false)],
    );
    write_skill(different.path(), "code-review", "changed");
    let builder = BundleService::new(different.path(), different.path().join("skills"));
    let changed = builder.build(different.path()).unwrap().artifact;

    let error = service.install(&changed).unwrap_err().to_string();
    assert!(error.contains("immutable"), "unexpected error: {error}");
    assert_eq!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap(),
        installed_before
    );
}

#[test]
fn local_edits_block_removal_without_changing_state() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let skill = project.path().join(".claude/skills/code-review/SKILL.md");
    fs::write(&skill, "local edit\n").unwrap();

    let error = service.remove("payments-team").unwrap_err().to_string();

    assert!(
        error.contains("locally modified"),
        "unexpected error: {error}"
    );
    assert_eq!(fs::read_to_string(skill).unwrap(), "local edit\n");
    assert_eq!(service.list().unwrap().len(), 1);
}

#[test]
fn explicit_update_replaces_changed_members_and_preserves_unrelated_skills() {
    let (_author, first) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let updated_author = TempDir::new().unwrap();
    write_bundle_project(
        updated_author.path(),
        "payments-team",
        "1.1.0",
        &[("code-review", false), ("payments-debugging", false)],
    );
    write_skill(updated_author.path(), "code-review", "review-v2");
    write_skill(updated_author.path(), "payments-debugging", "debug-v1");
    let updated = BundleService::new(updated_author.path(), updated_author.path().join("skills"))
        .build(updated_author.path())
        .unwrap()
        .artifact;
    let (project, service) = recipient();
    fs::create_dir_all(project.path().join(".claude/skills/personal")).unwrap();
    fs::write(
        project.path().join(".claude/skills/personal/SKILL.md"),
        "personal skill\n",
    )
    .unwrap();
    service.install(&first).unwrap();

    let preview = service.preview_update("payments-team", &updated).unwrap();
    assert_eq!(
        preview,
        vec!["add payments-debugging", "replace code-review"]
    );
    service.update("payments-team", &updated).unwrap();

    assert!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md"))
            .unwrap()
            .contains("review-v2")
    );
    assert!(project
        .path()
        .join(".claude/skills/payments-debugging/SKILL.md")
        .exists());
    assert!(project
        .path()
        .join(".claude/skills/personal/SKILL.md")
        .exists());
}

#[test]
fn declared_artifact_can_be_restored_after_the_skill_directory_is_removed() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    fs::remove_dir_all(project.path().join(".claude/skills")).unwrap();
    fs::create_dir_all(project.path().join(".claude/skills")).unwrap();

    let restored = service.install_declared().unwrap();

    assert_eq!(restored.len(), 1);
    assert!(!restored[0].unchanged);
    assert!(project
        .path()
        .join(".claude/skills/code-review/SKILL.md")
        .exists());
}

#[test]
fn locked_restore_uses_the_pinned_bundle_release() {
    let (_initial_author, initial) =
        author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let updated_author = TempDir::new().unwrap();
    write_bundle_project(
        updated_author.path(),
        "payments-team",
        "1.1.0",
        &[("code-review", false)],
    );
    write_skill(updated_author.path(), "code-review", "review-v2");
    let updated = BundleService::new(updated_author.path(), updated_author.path().join("skills"))
        .build(updated_author.path())
        .unwrap()
        .artifact;
    let (project, service) = recipient();
    service.install(&initial).unwrap();
    let bundle_dir = project.path().join(".fastskill/bundles");
    fs::copy(&updated, bundle_dir.join("payments-team-1.1.0.zip")).unwrap();
    let manifest = project.path().join("skill-project.toml");
    let drifted = fs::read_to_string(&manifest)
        .unwrap()
        .replace("version = \"1.0.0\"", "version = \"1.1.0\"")
        .replace("payments-team-1.0.0.zip", "payments-team-1.1.0.zip");
    fs::write(&manifest, drifted).unwrap();
    fs::remove_dir_all(project.path().join(".claude/skills")).unwrap();
    fs::create_dir_all(project.path().join(".claude/skills")).unwrap();

    service.install_declared_locked().unwrap();

    let installed =
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap();
    assert!(installed.contains("member-code-review"), "{installed}");
    assert!(!installed.contains("review-v2"), "{installed}");
    let repaired_manifest = fs::read_to_string(manifest).unwrap();
    assert!(repaired_manifest.contains("version = \"1.0.0\""));
    assert!(repaired_manifest.contains("payments-team-1.0.0.zip"));
}

#[test]
fn build_includes_transitive_member_dependencies() {
    let author = TempDir::new().unwrap();
    write_bundle_project(
        author.path(),
        "payments-team",
        "1.0.0",
        &[("code-review", false)],
    );
    write_skill(author.path(), "code-review", "review");
    fs::write(
        author.path().join("skills/code-review/skill-project.toml"),
        "[dependencies]\nshared-guidance = \"1.0.0\"\n",
    )
    .unwrap();
    write_skill(author.path(), "shared-guidance", "shared");

    let built = BundleService::new(author.path(), author.path().join("skills"))
        .build(author.path())
        .unwrap();
    let extract = TempDir::new().unwrap();
    fastskill_core::storage::zip::ZipHandler::new()
        .unwrap()
        .extract_to_dir(&built.artifact, extract.path())
        .unwrap();

    assert!(extract.path().join("skills/code-review/SKILL.md").is_file());
    assert!(extract
        .path()
        .join("skills/shared-guidance/SKILL.md")
        .is_file());
}

#[test]
fn conflicting_shared_member_is_rejected_without_mutating_the_target() {
    let (_first_author, first) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let different = TempDir::new().unwrap();
    write_bundle_project(
        different.path(),
        "security-team",
        "1.0.0",
        &[("code-review", false)],
    );
    write_skill(different.path(), "code-review", "different-content");
    let second = BundleService::new(different.path(), different.path().join("skills"))
        .build(different.path())
        .unwrap()
        .artifact;
    let (project, service) = recipient();
    service.install(&first).unwrap();
    let before =
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap();

    let error = service.install(&second).unwrap_err().to_string();

    assert!(
        error.contains("conflicting contents"),
        "unexpected error: {error}"
    );
    assert_eq!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap(),
        before
    );
    assert_eq!(service.list().unwrap().len(), 1);
}

#[test]
fn permitted_override_survives_restore_and_blocks_policy_tightening() {
    let (_author, initial) = author_bundle("payments-team", "1.0.0", &[("code-review", true)]);
    let updated_author = TempDir::new().unwrap();
    write_bundle_project(
        updated_author.path(),
        "payments-team",
        "1.1.0",
        &[("code-review", false)],
    );
    write_skill(updated_author.path(), "code-review", "review-v2");
    let update = BundleService::new(updated_author.path(), updated_author.path().join("skills"))
        .build(updated_author.path())
        .unwrap()
        .artifact;
    let override_source = TempDir::new().unwrap();
    let override_skill = override_source.path().join("code-review");
    fs::create_dir_all(&override_skill).unwrap();
    fs::write(
        override_skill.join("SKILL.md"),
        "personal review instructions\n",
    )
    .unwrap();
    let (project, service) = recipient();
    service.install(&initial).unwrap();

    service
        .override_member("code-review", &override_skill)
        .unwrap();
    fs::remove_dir_all(project.path().join(".claude/skills")).unwrap();
    fs::create_dir_all(project.path().join(".claude/skills")).unwrap();
    service.install_declared().unwrap();

    assert_eq!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap(),
        "personal review instructions\n"
    );
    let error = service
        .update("payments-team", &update)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("does not permit the personal override"),
        "unexpected error: {error}"
    );
}

#[test]
fn malicious_archive_path_is_rejected_without_writing_outside_the_project() {
    let (project, service) = recipient();
    let artifact = project.path().join("malicious.zip");
    let file = fs::File::create(&artifact).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("../outside.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"must not escape\n").unwrap();
    writer.finish().unwrap();

    let error = service.install(&artifact).unwrap_err().to_string();

    assert!(
        error.contains("unsafe") || error.contains("traversal"),
        "{error}"
    );
    assert!(!project
        .path()
        .parent()
        .unwrap()
        .join("outside.txt")
        .exists());
}

#[test]
fn bundle_update_rejects_contents_selected_by_a_direct_root() {
    let (_author, initial) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let changed_author = TempDir::new().unwrap();
    write_bundle_project(
        changed_author.path(),
        "payments-team",
        "2.0.0",
        &[("code-review", false)],
    );
    write_skill(changed_author.path(), "code-review", "changed");
    let changed = BundleService::new(changed_author.path(), changed_author.path().join("skills"))
        .build(changed_author.path())
        .unwrap()
        .artifact;
    let (project, service) = recipient();
    service.install(&initial).unwrap();
    let manifest_path = project.path().join("skill-project.toml");
    let mut manifest =
        fastskill_core::core::manifest::SkillProjectToml::load_from_file(&manifest_path).unwrap();
    manifest.dependencies.as_mut().unwrap().dependencies.insert(
        "code-review".to_string(),
        DependencySpec::Inline {
            origin: Origin::Local {
                path: PathBuf::from("direct-origin"),
                editable: false,
            },
            groups: None,
        },
    );
    fastskill_core::core::project_state::save_project_preserving(&manifest_path, &manifest)
        .unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    let digest = fastskill_core::core::project_removal::managed_tree_digest(
        &project.path().join(".claude/skills/code-review"),
    )
    .unwrap();
    lock.covered_roots.push("code-review".to_string());
    lock.skills.push(ProjectLockedSkillEntry {
        id: "code-review".to_string(),
        name: "code-review".to_string(),
        origin: Origin::Local {
            path: PathBuf::from("direct-origin"),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(digest),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        depth: 0,
        parent_skill: None,
        required_by: Vec::new(),
    });
    lock.save_to_file(&lock_path).unwrap();

    let error = service
        .update("payments-team", &changed)
        .unwrap_err()
        .to_string();

    assert!(error.contains("individual root(s): code-review"), "{error}");
    assert!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md"))
            .unwrap()
            .contains("member-code-review")
    );
}

#[test]
fn override_reset_rejects_local_edits_and_conflicting_retained_owners() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", true)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let personal = project.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal\n").unwrap();
    service.override_member("code-review", &personal).unwrap();
    let installed = project.path().join(".claude/skills/code-review/SKILL.md");
    fs::write(&installed, "local edit after override\n").unwrap();

    let edited_error = service
        .reset_override("code-review")
        .unwrap_err()
        .to_string();
    assert!(edited_error.contains("locally modified"), "{edited_error}");
    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        "local edit after override\n"
    );

    fs::write(&installed, "personal\n").unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    let mut conflicting = lock.bundles[0].clone();
    conflicting.id = "security-team".to_string();
    conflicting.members[0].digest = "different-selection".to_string();
    lock.bundles.push(conflicting);
    lock.save_to_file(&lock_path).unwrap();

    let conflict = service
        .reset_override("code-review")
        .unwrap_err()
        .to_string();
    assert!(conflict.contains("conflicting contents"), "{conflict}");
    assert_eq!(fs::read_to_string(installed).unwrap(), "personal\n");
    assert!(ProjectSkillsLock::load_from_file(&lock_path)
        .unwrap()
        .overrides
        .iter()
        .any(|entry| entry.id == "code-review"));
}

#[test]
fn override_reset_rejects_inconsistent_state_without_changing_personal_bytes() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", true)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let personal = project.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal\n").unwrap();
    service.override_member("code-review", &personal).unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.overrides.clear();
    lock.save_to_file(&lock_path).unwrap();

    let error = service
        .reset_override("code-review")
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("inconsistent between the Manifest and Lock"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap(),
        "personal\n"
    );
}

#[test]
fn override_reset_requires_a_retained_bundle_owner() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", true)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let personal = project.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal\n").unwrap();
    service.override_member("code-review", &personal).unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.bundles.clear();
    lock.save_to_file(&lock_path).unwrap();

    let error = service
        .reset_override("code-review")
        .unwrap_err()
        .to_string();

    assert!(error.contains("no retained bundle owner"), "{error}");
    assert_eq!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md")).unwrap(),
        "personal\n"
    );
}

#[test]
fn personal_override_validates_identity_source_and_owner_policy_before_mutation() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let missing = project.path().join("missing");
    let error = service
        .override_member("code-review", &missing)
        .unwrap_err()
        .to_string();
    assert!(error.contains("must contain SKILL.md"), "{error}");
    let personal = project.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal\n").unwrap();
    let invalid = service
        .override_member("../escape", &personal)
        .unwrap_err()
        .to_string();
    assert!(invalid.contains("Skill ID must be"), "{invalid}");
    let absent = service
        .override_member("not-owned", &personal)
        .unwrap_err()
        .to_string();
    assert!(
        absent.contains("not owned by an installed bundle"),
        "{absent}"
    );
    let policy = service
        .override_member("code-review", &personal)
        .unwrap_err()
        .to_string();
    assert!(
        policy.contains("must permit a personal override"),
        "{policy}"
    );
    assert!(
        fs::read_to_string(project.path().join(".claude/skills/code-review/SKILL.md"))
            .unwrap()
            .contains("member-code-review")
    );
}

#[test]
fn override_preview_protects_local_edits_and_reports_the_reset_selection() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", true)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let installed = project.path().join(".claude/skills/code-review/SKILL.md");
    let packaged = fs::read_to_string(&installed).unwrap();
    fs::write(&installed, "untracked local edit\n").unwrap();
    let personal = project.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal\n").unwrap();

    let error = service
        .preview_override("code-review", &personal)
        .unwrap_err()
        .to_string();
    assert!(error.contains("locally modified"), "{error}");
    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        "untracked local edit\n"
    );

    fs::write(&installed, packaged).unwrap();
    let set = service.preview_override("code-review", &personal).unwrap();
    assert!(set.changed);
    assert_eq!(set.changes, vec!["content", "manifest", "lock"]);
    service.override_member("code-review", &personal).unwrap();
    assert!(service.install_declared().unwrap()[0].unchanged);
    assert!(service.install_declared_locked().unwrap()[0].unchanged);
    let reset = service.preview_reset_override("code-review").unwrap();
    assert!(reset.changed);
    assert_ne!(reset.current_revision, reset.target_revision);
    service.reset_override("code-review").unwrap();
    let unchanged = service.preview_reset_override("code-review").unwrap();
    assert!(!unchanged.changed);
}

#[test]
fn override_restore_rejects_a_missing_or_changed_personal_source() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", true)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();
    let personal = project.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal\n").unwrap();
    service.override_member("code-review", &personal).unwrap();
    fs::write(personal.join("SKILL.md"), "changed source\n").unwrap();
    let changed = service.install_declared().unwrap_err().to_string();
    assert!(
        changed.contains("no longer matches its locked digest"),
        "{changed}"
    );
    fs::remove_file(personal.join("SKILL.md")).unwrap();
    let missing = service.install_declared().unwrap_err().to_string();
    assert!(missing.contains("source is unavailable"), "{missing}");
}

#[test]
fn two_bundles_and_a_direct_root_release_shared_contents_independently() {
    let (_first_author, first) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (_second_author, second) =
        author_bundle("security-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    service.install(&first).unwrap();
    service.install(&second).unwrap();
    let manifest_path = project.path().join("skill-project.toml");
    let mut manifest =
        fastskill_core::core::manifest::SkillProjectToml::load_from_file(&manifest_path).unwrap();
    manifest.dependencies.as_mut().unwrap().dependencies.insert(
        "code-review".to_string(),
        DependencySpec::Inline {
            origin: Origin::Local {
                path: PathBuf::from("direct-origin"),
                editable: false,
            },
            groups: None,
        },
    );
    fastskill_core::core::project_state::save_project_preserving(&manifest_path, &manifest)
        .unwrap();
    let installed_directory = project.path().join(".claude/skills/code-review");
    let digest =
        fastskill_core::core::project_removal::managed_tree_digest(&installed_directory).unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.covered_roots.push("code-review".to_string());
    lock.skills.push(ProjectLockedSkillEntry {
        id: "code-review".to_string(),
        name: "code-review".to_string(),
        origin: Origin::Local {
            path: PathBuf::from("direct-origin"),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(digest),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        depth: 0,
        parent_skill: None,
        required_by: Vec::new(),
    });
    lock.save_to_file(&lock_path).unwrap();

    let direct_plan = fastskill_core::core::project_removal::ProjectRemovalService::new(
        project.path(),
        project.path().join(".claude/skills"),
    )
    .remove(&["code-review".to_string()])
    .unwrap();
    assert_eq!(direct_plan.retained_files, vec!["code-review"]);
    assert!(installed_directory.is_dir());
    service.remove("payments-team").unwrap();
    assert!(installed_directory.is_dir());
    service.remove("security-team").unwrap();
    assert!(!installed_directory.exists());
}

#[test]
fn resetting_an_absent_override_is_unchanged() {
    let (_project, service) = recipient();
    assert!(!service.reset_override("code-review").unwrap());
}

#[test]
fn bundle_detection_and_lifecycle_noops_report_precise_results() {
    let scratch = TempDir::new().unwrap();
    assert!(!BundleService::is_bundle_artifact(&scratch.path().join("plain.txt")).unwrap());
    for (name, manifest) in [
        ("missing.zip", None),
        ("ordinary.zip", Some("[dependencies]\n")),
        ("invalid.zip", Some("[bundle\n")),
    ] {
        let artifact = scratch.path().join(name);
        let file = fs::File::create(&artifact).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        if let Some(manifest) = manifest {
            zip.start_file(
                "skill-project.toml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    assert!(!BundleService::is_bundle_artifact(&scratch.path().join("missing.zip")).unwrap());
    assert!(!BundleService::is_bundle_artifact(&scratch.path().join("ordinary.zip")).unwrap());
    assert!(BundleService::is_bundle_artifact(&scratch.path().join("invalid.zip")).is_err());

    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (_next_author, next) = author_bundle("payments-team", "2.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    let first = service.install(&artifact).unwrap();
    assert!(!first.unchanged);
    assert!(service.install(&artifact).unwrap().unchanged);
    assert!(service
        .install(&next)
        .unwrap_err()
        .to_string()
        .contains("use 'fastskill update"));
    assert!(service
        .update("other-team", &next)
        .unwrap_err()
        .to_string()
        .contains("not requested"));
    assert!(service
        .update("missing-team", &artifact)
        .unwrap_err()
        .to_string()
        .contains("not requested"));
    assert!(
        service
            .update("payments-team", &artifact)
            .unwrap()
            .unchanged
    );
    assert!(service.install_declared().unwrap()[0].unchanged);
    assert!(service.install_declared_locked().unwrap()[0].unchanged);
    assert!(service.ensure_individual_removal_allowed("unowned").is_ok());
    assert!(service
        .ensure_individual_removal_allowed("code-review")
        .unwrap_err()
        .to_string()
        .contains("managed by installed bundle"));
    assert!(service.preview_remove("../escape").is_err());
    assert!(service.preview_remove("missing-team").is_err());
    assert!(project.path().join(".claude/skills/code-review").exists());
}

#[test]
fn malformed_bundle_history_is_rejected_before_installation() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    fs::create_dir_all(project.path().join(".fastskill")).unwrap();
    fs::write(
        project.path().join(".fastskill/bundle-history.toml"),
        "releases = [",
    )
    .unwrap();

    let error = service.install(&artifact).unwrap_err().to_string();

    assert!(error.contains("Failed to load bundle history"), "{error}");
    assert!(!project.path().join(".claude/skills/code-review").exists());
}

#[test]
fn declared_bundle_validation_covers_manifest_and_locked_restore_plans() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (_project, service) = recipient();
    service.install(&artifact).unwrap();

    let manifest_members = service.validated_declared_members(false).unwrap();
    let locked_members = service.validated_declared_members(true).unwrap();
    assert_eq!(manifest_members.len(), 1);
    assert_eq!(manifest_members, locked_members);
    assert_eq!(manifest_members[0].bundle_id, "payments-team");
    assert_eq!(manifest_members[0].id, "code-review");
}

#[test]
fn combined_declared_bundle_validation_rejects_conflicting_owners() {
    let (_first_author, first) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let different = TempDir::new().unwrap();
    write_bundle_project(
        different.path(),
        "security-team",
        "1.0.0",
        &[("code-review", false)],
    );
    write_skill(different.path(), "code-review", "different");
    let second = BundleService::new(different.path(), different.path().join("skills"))
        .build(different.path())
        .unwrap()
        .artifact;
    let (project, service) = recipient();
    fs::copy(first, project.path().join("payments.zip")).unwrap();
    fs::copy(second, project.path().join("security.zip")).unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n\n[bundles.payments-team]\nversion = \"1.0.0\"\nartifact = \"payments.zip\"\n\n[bundles.security-team]\nversion = \"1.0.0\"\nartifact = \"security.zip\"\n",
    )
    .unwrap();

    let error = service.validate_declared(false).unwrap_err().to_string();

    assert!(error.contains("conflicting contents"), "{error}");
    assert!(service.list().unwrap().is_empty());
    assert!(!project.path().join(".claude/skills/code-review").exists());
}

#[test]
fn declared_bundle_validation_rejects_an_artifact_with_another_identity() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    fs::copy(artifact, project.path().join("bundle.zip")).unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n\n[bundles.security-team]\nversion = \"1.0.0\"\nartifact = \"bundle.zip\"\n",
    )
    .unwrap();

    let error = service.validate_declared(false).unwrap_err().to_string();

    assert!(
        error.contains("does not match artifact identity"),
        "{error}"
    );
}

#[test]
fn declared_bundle_validation_reports_an_invalid_project_manifest() {
    let (project, service) = recipient();
    fs::write(project.path().join("skill-project.toml"), "[invalid\n").unwrap();

    let error = service.validate_declared(false).unwrap_err().to_string();

    assert!(
        error.contains("Failed to load bundle declarations"),
        "{error}"
    );
}

#[test]
fn bundle_build_validates_identity_version_format_and_member_declarations() {
    let cases = [
        (
            "[bundle]\nformat = \"other\"\nid = \"team\"\nversion = \"1.0.0\"\n[bundle.members.demo]\n[dependencies]\ndemo = \"1.0.0\"\n",
            "supported FastSkill bundle",
        ),
        (
            "[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"../team\"\nversion = \"1.0.0\"\n[bundle.members.demo]\n[dependencies]\ndemo = \"1.0.0\"\n",
            "Skill ID must be",
        ),
        (
            "[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"not-semver\"\n[bundle.members.demo]\n[dependencies]\ndemo = \"1.0.0\"\n",
            "not valid SemVer",
        ),
        (
            "[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"1.0.0\"\n",
            "at least one member",
        ),
        (
            "[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"1.0.0\"\n[bundle.members.demo]\n[dependencies]\n",
            "not declared in [dependencies]",
        ),
    ];
    for (manifest, expected) in cases {
        let author = TempDir::new().unwrap();
        fs::write(author.path().join("skill-project.toml"), manifest).unwrap();
        let error = BundleService::new(author.path(), author.path().join("skills"))
            .build(author.path())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(expected),
            "expected {expected:?}, got {error}"
        );
    }
}

#[test]
fn bundle_install_rejects_archives_missing_required_root_files() {
    let (project, service) = recipient();
    let missing_manifest = project.path().join("missing-manifest.zip");
    let writer = fs::File::create(&missing_manifest).unwrap();
    zip::ZipWriter::new(writer).finish().unwrap();
    let error = service.install(&missing_manifest).unwrap_err().to_string();
    assert!(error.contains("missing root skill-project.toml"), "{error}");

    let missing_lock = project.path().join("missing-lock.zip");
    let writer = fs::File::create(&missing_lock).unwrap();
    let mut archive = zip::ZipWriter::new(writer);
    archive
        .start_file(
            "skill-project.toml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive
        .write_all(
            b"[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"1.0.0\"\n[bundle.members.demo]\n[dependencies]\ndemo = \"1.0.0\"\n",
        )
        .unwrap();
    archive
        .start_file(
            "skills/demo/SKILL.md",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive
        .write_all(b"---\nname: demo\nversion: 1.0.0\n---\ndemo\n")
        .unwrap();
    archive.finish().unwrap();
    let error = service.install(&missing_lock).unwrap_err().to_string();
    assert!(error.contains("missing root skills.lock"), "{error}");
}

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use fastskill_core::core::bundle::{BundleService, BUNDLE_FORMAT};
use fastskill_core::core::manifest::SkillProjectToml;
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
fn normal_manifest_save_preserves_bundle_declarations() {
    let (_author, artifact) = author_bundle("payments-team", "1.0.0", &[("code-review", false)]);
    let (project, service) = recipient();
    service.install(&artifact).unwrap();

    let manifest_path = project.path().join("skill-project.toml");
    let manifest = SkillProjectToml::load_from_file(&manifest_path).unwrap();
    manifest.save_to_file(&manifest_path).unwrap();

    let content = fs::read_to_string(manifest_path).unwrap();
    assert!(content.contains("[bundles.payments-team]"), "{content}");
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

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const BUNDLE_FORMAT: &str = "fastskill-bundle-v1";

fn write_project(root: &Path, id: &str, version: &str, marker: &str) {
    write_project_with_policy(root, id, version, marker, false);
}

fn write_project_with_policy(
    root: &Path,
    id: &str,
    version: &str,
    marker: &str,
    overridable: bool,
) {
    fs::create_dir_all(root.join("skills/code-review")).unwrap();
    fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[bundle]\nformat = \"{BUNDLE_FORMAT}\"\nid = \"{id}\"\nversion = \"{version}\"\n\n[bundle.members.code-review]\noverridable = {overridable}\n\n[dependencies]\ncode-review = \"1.0.0\"\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("skills/code-review/SKILL.md"),
        format!("---\nname: code-review\nversion: \"1.0.0\"\n---\n{marker}\n"),
    )
    .unwrap();
}

fn build_bundle(
    root: &Path,
    id: &str,
    version: &str,
    marker: &str,
    overridable: bool,
) -> std::path::PathBuf {
    let dist = root.join("dist");
    write_project_with_policy(root, id, version, marker, overridable);
    assert_success(run(
        root,
        &["bundle", "build", "--output", dist.to_str().unwrap()],
    ));
    dist.join(format!("{id}-{version}.zip"))
}

fn write_recipient(root: &Path) -> std::path::PathBuf {
    let storage = root.join(".claude/skills");
    fs::create_dir_all(&storage).unwrap();
    fs::write(
        root.join("skill-project.toml"),
        "custom_root = \"keep\"\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[tool.fastskill.extension]\nflag = true\n\n[dependencies]\n",
    )
    .unwrap();
    storage
}

fn write_skill_zip(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("demo/SKILL.md", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(b"---\nname: demo\nversion: 1.0.0\n---\n# Demo\n")
        .unwrap();
    writer.finish().unwrap();
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn assert_success(output: std::process::Output) -> String {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn bundle_build_explains_the_required_bundle_declaration() {
    let project = TempDir::new().unwrap();
    fs::write(
        project.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\ncode-review = \"1.0.0\"\n",
    )
    .unwrap();

    let output = run(project.path(), &["bundle", "build"]);

    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("has no [bundle] declaration"), "{error}");
    assert!(error.contains("fastskill bundle build --help"), "{error}");
}

#[test]
fn bundle_build_help_shows_a_complete_bundle_declaration() {
    let project = TempDir::new().unwrap();

    let output = run(project.path(), &["bundle", "build", "--help"]);

    let help = assert_success(output);
    assert!(
        help.contains("Add this to skill-project.toml before building:"),
        "{help}"
    );
    assert!(help.contains("[bundle]"), "{help}");
    assert!(help.contains("format = \"fastskill-bundle-v1\""), "{help}");
    assert!(help.contains("id = \"payments-team\""), "{help}");
    assert!(help.contains("version = \"1.2.0\""), "{help}");
    assert!(help.contains("[bundle.members.code-review]"), "{help}");
    assert!(help.contains("code-review = \"1.0.0\""), "{help}");
}

#[test]
fn bundle_add_dry_run_json_is_one_nonmutating_validated_plan() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), "payments-team", "1.0.0", "review-v1", false);
    let recipient = TempDir::new().unwrap();
    let storage = write_recipient(recipient.path());
    let manifest_path = recipient.path().join("skill-project.toml");
    let manifest_before = fs::read(&manifest_path).unwrap();

    let output = run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "add",
            artifact.to_str().unwrap(),
            "--dry-run",
            "--json",
        ],
    );
    let stdout = assert_success(output);
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["scope"], "project");
    assert_eq!(value["outcome"], "changed");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["targets"][0]["id"], "payments-team");
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest_before);
    assert!(!recipient.path().join("skills.lock").exists());
    assert!(!storage.join("code-review").exists());
    assert!(!recipient.path().join(".fastskill").exists());
}

#[test]
fn artifact_kind_mismatches_fail_before_service_initialization() {
    let author = TempDir::new().unwrap();
    let bundle = build_bundle(author.path(), "payments-team", "1.0.0", "review-v1", false);
    let recipient = TempDir::new().unwrap();

    let skill_add = run(
        recipient.path(),
        &["skill", "add", bundle.to_str().unwrap(), "--no-reindex"],
    );
    assert!(!skill_add.status.success());
    assert!(
        String::from_utf8_lossy(&skill_add.stderr).contains("fastskill bundle add <ARTIFACT>"),
        "{}",
        String::from_utf8_lossy(&skill_add.stderr)
    );

    let skill_zip = recipient.path().join("demo.zip");
    write_skill_zip(&skill_zip);
    let bundle_add = run(
        recipient.path(),
        &["bundle", "add", skill_zip.to_str().unwrap()],
    );
    assert!(!bundle_add.status.success());
    assert!(
        String::from_utf8_lossy(&bundle_add.stderr).contains("fastskill skill add <SOURCE>"),
        "{}",
        String::from_utf8_lossy(&bundle_add.stderr)
    );

    for path in ["skill-project.toml", "skills.lock", ".claude", ".fastskill"] {
        assert!(
            !recipient.path().join(path).exists(),
            "artifact rejection created {path}"
        );
    }
}

#[test]
fn bundle_add_modes_share_validation_preview_and_structured_apply_results() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), "payments-team", "1.0.0", "review-v1", false);
    let recipient = TempDir::new().unwrap();
    let storage = write_recipient(recipient.path());
    let storage_arg = storage.to_str().unwrap();
    let artifact_arg = artifact.to_str().unwrap();

    for extra in ["--force", "--group"] {
        let args = if extra == "--group" {
            vec![
                "--skills-dir",
                storage_arg,
                "bundle",
                "add",
                artifact_arg,
                extra,
                "dev",
            ]
        } else {
            vec![
                "--skills-dir",
                storage_arg,
                "bundle",
                "add",
                artifact_arg,
                extra,
            ]
        };
        let output = run(recipient.path(), &args);
        assert!(!output.status.success());
        assert!(!recipient.path().join("skills.lock").exists());
    }

    let preview = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "add",
            artifact_arg,
            "--dry-run",
        ],
    ));
    assert!(preview.contains("Would install bundle"), "{preview}");

    let applied = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "add",
            artifact_arg,
            "--json",
        ],
    ));
    let applied: serde_json::Value = serde_json::from_str(&applied).unwrap();
    assert_eq!(applied["outcome"], "changed");
    assert_eq!(applied["dry_run"], false);
    assert!(storage.join("code-review/SKILL.md").exists());

    let unchanged = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "add",
            artifact_arg,
            "--dry-run",
        ],
    ));
    assert!(unchanged.contains("already installed"), "{unchanged}");

    let updated_author = TempDir::new().unwrap();
    let updated = build_bundle(
        updated_author.path(),
        "payments-team",
        "1.1.0",
        "review-v2",
        false,
    );
    let updated_arg = updated.to_str().unwrap();
    let update = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "update",
            "payments-team",
            "--from",
            updated_arg,
            "--json",
        ],
    ));
    let update: serde_json::Value = serde_json::from_str(&update).unwrap();
    assert_eq!(update["outcome"], "changed");
    assert_eq!(update["targets"][0]["target_revision"], "1.1.0");

    let unchanged_update = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "update",
            "payments-team",
            "--from",
            updated_arg,
            "--dry-run",
        ],
    ));
    assert!(
        unchanged_update.contains("already unchanged"),
        "{unchanged_update}"
    );
    let unchanged_apply = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "update",
            "payments-team",
            "--from",
            updated_arg,
        ],
    ));
    assert!(
        unchanged_apply.contains("already unchanged"),
        "{unchanged_apply}"
    );
}

#[test]
fn bundle_commands_build_install_list_update_and_remove() {
    let author = TempDir::new().unwrap();
    let dist = author.path().join("dist");
    write_project(author.path(), "payments-team", "1.0.0", "review-v1");
    assert_success(run(
        author.path(),
        &["bundle", "build", "--output", dist.to_str().unwrap()],
    ));
    let first = dist.join("payments-team-1.0.0.zip");
    assert!(first.is_file());

    let recipient = TempDir::new().unwrap();
    let storage = recipient.path().join(".claude/skills");
    fs::create_dir_all(&storage).unwrap();
    fs::write(
        recipient.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let storage_arg = storage.to_str().unwrap();
    let first_arg = first.to_str().unwrap();
    let added = assert_success(run(
        recipient.path(),
        &["--skills-dir", storage_arg, "bundle", "add", first_arg],
    ));
    assert!(
        added.contains("Installed bundle payments-team@1.0.0"),
        "{added}"
    );
    assert!(storage.join("code-review/SKILL.md").is_file());

    let member_remove = run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "skill",
            "remove",
            "code-review",
            "--force",
        ],
    );
    assert!(!member_remove.status.success());
    let member_error = String::from_utf8(member_remove.stderr).unwrap();
    assert!(
        member_error.contains("managed by installed bundle"),
        "{member_error}"
    );
    assert!(storage.join("code-review/SKILL.md").is_file());

    let listed = assert_success(run(
        recipient.path(),
        &["--skills-dir", storage_arg, "bundle", "list"],
    ));
    assert!(listed.contains("payments-team 1.0.0"), "{listed}");

    let updated_author = TempDir::new().unwrap();
    let updated_dist = updated_author.path().join("dist");
    write_project(updated_author.path(), "payments-team", "1.1.0", "review-v2");
    assert_success(run(
        updated_author.path(),
        &[
            "bundle",
            "build",
            "--output",
            updated_dist.to_str().unwrap(),
        ],
    ));
    let second = updated_dist.join("payments-team-1.1.0.zip");
    let updated = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "update",
            "payments-team",
            "--from",
            second.to_str().unwrap(),
        ],
    ));
    assert!(
        updated.contains("Updated bundle payments-team@1.1.0"),
        "{updated}"
    );
    assert!(fs::read_to_string(storage.join("code-review/SKILL.md"))
        .unwrap()
        .contains("review-v2"));

    let removed = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage_arg,
            "bundle",
            "remove",
            "payments-team",
            "--force",
        ],
    ));
    assert!(
        removed.contains("Removed bundle: payments-team"),
        "{removed}"
    );
    assert!(!storage.join("code-review").exists());
}

#[test]
fn adding_and_removing_an_unrelated_skill_preserves_bundle_and_extension_tables() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), "payments-team", "1.0.0", "packaged", false);
    let recipient = TempDir::new().unwrap();
    let storage = write_recipient(recipient.path());
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "add",
            artifact.to_str().unwrap(),
        ],
    ));

    let local = recipient.path().join("local-skill");
    fs::create_dir_all(&local).unwrap();
    fs::write(
        local.join("SKILL.md"),
        "---\nname: local-skill\ndescription: Local test skill\nversion: 1.0.0\n---\nlocal\n",
    )
    .unwrap();
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "skill",
            "add",
            local.to_str().unwrap(),
        ],
    ));
    let lock_path = recipient.path().join("skills.lock");
    let mut lock =
        fastskill_core::core::lock::ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    if let Some(entry) = lock
        .skills
        .iter_mut()
        .find(|entry| entry.id == "local-skill")
    {
        entry.resolved.checksum = Some(
            fastskill_core::core::project_removal::managed_tree_digest(
                &storage.join("local-skill"),
            )
            .unwrap(),
        );
    }
    lock.save_to_file(&lock_path).unwrap();
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "skill",
            "remove",
            "local-skill",
            "--force",
        ],
    ));

    let manifest = fs::read_to_string(recipient.path().join("skill-project.toml")).unwrap();
    assert!(manifest.contains("[bundles.payments-team]"), "{manifest}");
    assert!(manifest.contains("custom_root = \"keep\""), "{manifest}");
    assert!(
        manifest.contains("[tool.fastskill.extension]"),
        "{manifest}"
    );
    assert!(manifest.contains("flag = true"), "{manifest}");
}

#[test]
fn bundle_override_reset_restores_packaged_contents_and_clears_records() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), "payments-team", "1.0.0", "packaged", true);
    let recipient = TempDir::new().unwrap();
    let storage = write_recipient(recipient.path());
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "add",
            artifact.to_str().unwrap(),
        ],
    ));

    let personal = recipient.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal\n").unwrap();
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "override",
            "code-review",
            "--from",
            personal.to_str().unwrap(),
        ],
    ));
    assert!(fs::read_to_string(storage.join("code-review/SKILL.md"))
        .unwrap()
        .contains("personal"));

    for install_args in [
        vec!["project", "install"],
        vec!["project", "install", "--lock"],
    ] {
        assert_success(run(recipient.path(), &install_args));
        assert!(fs::read_to_string(storage.join("code-review/SKILL.md"))
            .unwrap()
            .contains("personal"));
        let manifest = fs::read_to_string(recipient.path().join("skill-project.toml")).unwrap();
        let lock = fs::read_to_string(recipient.path().join("skills.lock")).unwrap();
        assert!(manifest.contains("[overrides.code-review]"), "{manifest}");
        assert!(lock.contains("[[overrides]]"), "{lock}");
    }

    let reset = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "override",
            "code-review",
            "--reset",
        ],
    ));
    assert!(
        reset.contains("Reset personal override for code-review"),
        "{reset}"
    );
    assert!(fs::read_to_string(storage.join("code-review/SKILL.md"))
        .unwrap()
        .contains("packaged"));
    let manifest = fs::read_to_string(recipient.path().join("skill-project.toml")).unwrap();
    assert!(!manifest.contains("[overrides.code-review]"), "{manifest}");
    let lock = fs::read_to_string(recipient.path().join("skills.lock")).unwrap();
    assert!(!lock.contains("[[overrides]]"), "{lock}");
}

#[test]
fn bundle_override_requires_exactly_one_of_from_or_reset() {
    let project = TempDir::new().unwrap();
    write_recipient(project.path());
    let both = run(
        project.path(),
        &[
            "bundle",
            "override",
            "code-review",
            "--from",
            project.path().to_str().unwrap(),
            "--reset",
        ],
    );
    assert!(!both.status.success());
    assert!(
        String::from_utf8_lossy(&both.stderr).contains("Choose either --from DIRECTORY or --reset"),
        "{}",
        String::from_utf8_lossy(&both.stderr)
    );
}

#[test]
fn bundle_operations_reject_global_scope_before_mutation() {
    let project = TempDir::new().unwrap();
    write_recipient(project.path());
    for args in [
        vec!["--global", "bundle", "build"],
        vec!["--global", "bundle", "add", "missing.zip"],
        vec!["--global", "bundle", "list"],
        vec![
            "--global",
            "bundle",
            "update",
            "team",
            "--from",
            "missing.zip",
        ],
        vec!["--global", "bundle", "remove", "team", "--force"],
        vec!["--global", "bundle", "override", "code-review", "--reset"],
    ] {
        let output = run(project.path(), &args);
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("do not support --global"), "{error}");
    }
    assert!(!project.path().join("skills.lock").exists());
}

#[test]
fn removing_final_bundle_owner_promotes_personal_override_to_direct_requirement() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), "payments-team", "1.0.0", "packaged", true);
    let recipient = TempDir::new().unwrap();
    let storage = write_recipient(recipient.path());
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "add",
            artifact.to_str().unwrap(),
        ],
    ));
    let shared = recipient.path().join("shared-guidance");
    fs::create_dir_all(&shared).unwrap();
    fs::write(
        shared.join("SKILL.md"),
        "---\nname: shared-guidance\nversion: 1.0.0\ndescription: shared guidance\n---\nshared\n",
    )
    .unwrap();
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "skill",
            "add",
            shared.to_str().unwrap(),
            "--no-reindex",
        ],
    ));
    let personal = recipient.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(
        personal.join("SKILL.md"),
        "---\nname: personal-review\nversion: 2.0.0\n---\npersonal retained\n",
    )
    .unwrap();
    fs::write(
        personal.join("skill-project.toml"),
        "[dependencies]\nshared-guidance = { origin = { type = \"local\", path = \"../shared-guidance\" } }\n",
    )
    .unwrap();
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "override",
            "code-review",
            "--from",
            personal.to_str().unwrap(),
            "--no-reindex",
        ],
    ));

    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "remove",
            "payments-team",
            "--force",
            "--no-reindex",
        ],
    ));

    let manifest_path = recipient.path().join("skill-project.toml");
    let manifest_text = fs::read_to_string(&manifest_path).unwrap();
    assert!(
        !manifest_text.contains("[overrides.code-review]"),
        "{manifest_text}"
    );
    let manifest =
        fastskill_core::core::manifest::SkillProjectToml::load_from_file(&manifest_path).unwrap();
    let dependency = manifest
        .dependencies
        .unwrap()
        .dependencies
        .remove("code-review")
        .unwrap();
    assert!(matches!(
        dependency,
        fastskill_core::core::manifest::DependencySpec::Inline {
            origin: fastskill_core::core::origin::Origin::Local { path, editable: false },
            ..
        } if path.as_path() == Path::new("personal")
    ));
    let lock = fastskill_core::core::lock::ProjectSkillsLock::load_from_file(
        &recipient.path().join("skills.lock"),
    )
    .unwrap();
    assert!(lock.bundles.is_empty());
    assert!(lock.overrides.is_empty());
    let promoted = lock
        .skills
        .iter()
        .find(|entry| entry.id == "code-review")
        .unwrap();
    assert_eq!(promoted.resolved.version, "2.0.0");
    assert_eq!(promoted.depth, 0);
    assert_eq!(promoted.dependencies, vec!["shared-guidance"]);
    assert!(lock
        .skills
        .iter()
        .any(|entry| entry.id == "shared-guidance"));
    assert!(fs::read_to_string(storage.join("code-review/SKILL.md"))
        .unwrap()
        .contains("personal retained"));
}

#[test]
fn removing_final_bundle_owner_blocks_dangling_override_dependencies() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), "payments-team", "1.0.0", "packaged", true);
    let recipient = TempDir::new().unwrap();
    let storage = write_recipient(recipient.path());
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "add",
            artifact.to_str().unwrap(),
        ],
    ));
    let personal = recipient.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal retained\n").unwrap();
    fs::write(
        personal.join("skill-project.toml"),
        "[dependencies]\nmissing-child = \"1.0.0\"\n",
    )
    .unwrap();
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "override",
            "code-review",
            "--from",
            personal.to_str().unwrap(),
            "--no-reindex",
        ],
    ));
    let manifest_before = fs::read(recipient.path().join("skill-project.toml")).unwrap();
    let lock_before = fs::read(recipient.path().join("skills.lock")).unwrap();

    let output = run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "remove",
            "payments-team",
            "--force",
            "--no-reindex",
        ],
    );

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing-child"));
    assert_eq!(
        fs::read(recipient.path().join("skill-project.toml")).unwrap(),
        manifest_before
    );
    assert_eq!(
        fs::read(recipient.path().join("skills.lock")).unwrap(),
        lock_before
    );
    assert!(fs::read_to_string(storage.join("code-review/SKILL.md"))
        .unwrap()
        .contains("personal retained"));
}

#[test]
fn install_lock_restores_the_pinned_bundle_release() {
    let initial_author = TempDir::new().unwrap();
    let initial_dist = initial_author.path().join("dist");
    write_project(initial_author.path(), "payments-team", "1.0.0", "review-v1");
    assert_success(run(
        initial_author.path(),
        &[
            "bundle",
            "build",
            "--output",
            initial_dist.to_str().unwrap(),
        ],
    ));
    let initial = initial_dist.join("payments-team-1.0.0.zip");

    let updated_author = TempDir::new().unwrap();
    let updated_dist = updated_author.path().join("dist");
    write_project(updated_author.path(), "payments-team", "1.1.0", "review-v2");
    assert_success(run(
        updated_author.path(),
        &[
            "bundle",
            "build",
            "--output",
            updated_dist.to_str().unwrap(),
        ],
    ));
    let updated = updated_dist.join("payments-team-1.1.0.zip");

    let recipient = TempDir::new().unwrap();
    let storage = recipient.path().join(".claude/skills");
    fs::create_dir_all(&storage).unwrap();
    fs::write(
        recipient.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "add",
            initial.to_str().unwrap(),
        ],
    ));

    let bundle_dir = recipient.path().join(".fastskill/bundles");
    fs::copy(&updated, bundle_dir.join("payments-team-1.1.0.zip")).unwrap();
    let manifest = recipient.path().join("skill-project.toml");
    let drifted = fs::read_to_string(&manifest)
        .unwrap()
        .replace("version = \"1.0.0\"", "version = \"1.1.0\"")
        .replace("payments-team-1.0.0.zip", "payments-team-1.1.0.zip");
    fs::write(&manifest, drifted).unwrap();
    fs::remove_dir_all(&storage).unwrap();
    fs::create_dir_all(&storage).unwrap();

    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "project",
            "install",
            "--lock",
        ],
    ));

    let installed = fs::read_to_string(storage.join("code-review/SKILL.md")).unwrap();
    assert!(installed.contains("review-v1"), "{installed}");
    assert!(!installed.contains("review-v2"), "{installed}");
    let repaired_manifest = fs::read_to_string(manifest).unwrap();
    assert!(repaired_manifest.contains("version = \"1.0.0\""));
    assert!(repaired_manifest.contains("payments-team-1.0.0.zip"));
}

#[test]
fn bundle_previews_emit_json_and_leave_managed_state_unchanged() {
    let initial_author = TempDir::new().unwrap();
    let initial = build_bundle(
        initial_author.path(),
        "payments-team",
        "1.0.0",
        "review-v1",
        true,
    );
    let updated_author = TempDir::new().unwrap();
    let updated = build_bundle(
        updated_author.path(),
        "payments-team",
        "1.1.0",
        "review-v2",
        true,
    );
    let recipient = TempDir::new().unwrap();
    let storage = write_recipient(recipient.path());
    assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "add",
            initial.to_str().unwrap(),
        ],
    ));
    let manifest_path = recipient.path().join("skill-project.toml");
    let lock_path = recipient.path().join("skills.lock");
    let before_manifest = fs::read(&manifest_path).unwrap();
    let before_lock = fs::read(&lock_path).unwrap();

    let update = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "update",
            "payments-team",
            "--from",
            updated.to_str().unwrap(),
            "--dry-run",
            "--json",
        ],
    ));
    let update: serde_json::Value = serde_json::from_str(&update).unwrap();
    assert_eq!(update["targets"][0]["current_revision"], "1.0.0");
    assert_eq!(update["targets"][0]["target_revision"], "1.1.0");
    assert!(fs::read_to_string(storage.join("code-review/SKILL.md"))
        .unwrap()
        .contains("review-v1"));

    let personal = recipient.path().join("personal");
    fs::create_dir_all(&personal).unwrap();
    fs::write(personal.join("SKILL.md"), "personal").unwrap();
    let override_output = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "override",
            "code-review",
            "--from",
            personal.to_str().unwrap(),
            "--dry-run",
            "--json",
        ],
    ));
    let override_output: serde_json::Value = serde_json::from_str(&override_output).unwrap();
    assert_eq!(override_output["targets"][0]["id"], "code-review");
    assert_eq!(override_output["outcome"], "changed");

    let remove = assert_success(run(
        recipient.path(),
        &[
            "--skills-dir",
            storage.to_str().unwrap(),
            "bundle",
            "remove",
            "payments-team",
            "--dry-run",
            "--json",
        ],
    ));
    let remove: serde_json::Value = serde_json::from_str(&remove).unwrap();
    assert_eq!(remove["targets"][0]["id"], "payments-team");
    assert_eq!(remove["targets"][0]["current_revision"], "1.0.0");
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    assert_eq!(fs::read(&lock_path).unwrap(), before_lock);
}

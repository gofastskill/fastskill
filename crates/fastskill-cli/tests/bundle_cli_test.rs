#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const BUNDLE_FORMAT: &str = "fastskill-bundle-v1";

fn write_project(root: &Path, id: &str, version: &str, marker: &str) {
    fs::create_dir_all(root.join("skills/code-review")).unwrap();
    fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[bundle]\nformat = \"{BUNDLE_FORMAT}\"\nid = \"{id}\"\nversion = \"{version}\"\n\n[bundle.members.code-review]\n\n[dependencies]\ncode-review = \"1.0.0\"\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("skills/code-review/SKILL.md"),
        format!("---\nname: code-review\nversion: \"1.0.0\"\n---\n{marker}\n"),
    )
    .unwrap();
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
        &["--skills-dir", storage_arg, "add", first_arg],
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
        &["--skills-dir", storage_arg, "list", "--bundles"],
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
            "update",
            "--bundle",
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
            "remove",
            "--bundle",
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

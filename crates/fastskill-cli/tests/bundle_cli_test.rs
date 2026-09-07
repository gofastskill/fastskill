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

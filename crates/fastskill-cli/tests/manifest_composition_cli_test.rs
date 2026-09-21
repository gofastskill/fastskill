#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn project_install_applies_dependencies_from_a_composed_manifest() {
    let root = TempDir::new().unwrap();
    let shared = root.path().join("shared");
    let source = shared.join("sources/review");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: review\nversion: 1.0.0\ndescription: shared review\n---\n# Review\n",
    )
    .unwrap();
    fs::write(
        shared.join("skill-project.toml"),
        "[dependencies.review]\norigin = { type = \"local\", path = \"sources/review\" }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "schema_version = \"2\"\n[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"\n[tool.fastskill.manifests]\nteam = \"shared/skill-project.toml\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root.path())
        .args(["project", "install", "--no-reindex"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.path().join("skills/review/SKILL.md").is_file());
    let lock = fs::read_to_string(root.path().join("skills.lock")).unwrap();
    assert!(lock.contains("id = \"review\""), "{lock}");
}

#[test]
fn project_install_rejects_manifest_cycles_before_creating_a_lock() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"\n[tool.fastskill.manifests]\nother = \"other.toml\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("other.toml"),
        "[dependencies]\n[tool.fastskill.manifests]\nroot = \"skill-project.toml\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root.path())
        .args(["project", "install", "--no-reindex"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Manifest composition cycle"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.path().join("skills.lock").exists());
}

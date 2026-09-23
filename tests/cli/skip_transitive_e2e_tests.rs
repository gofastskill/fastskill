//! `[tool.fastskill] skip_transitive` applies to every lifecycle command that
//! plans an installation: `project install` (fresh and from the lock),
//! `skill add` and `skill update`.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::run_fastskill_command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_skill(path: &Path, id: &str, dependency: Option<&Path>) {
    fs::create_dir_all(path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!(
            "---\nname: {id}\ndescription: skip_transitive fixture\nversion: 1.0.0\n---\n# {id}\n"
        ),
    )
    .unwrap();
    if let Some(dependency) = dependency {
        fs::write(
            path.join("skill-project.toml"),
            format!(
                "[dependencies]\nchild = {{ origin = {{ type = \"local\", path = {:?} }} }}\n",
                dependency
            ),
        )
        .unwrap();
    }
}

struct Fixture {
    root: TempDir,
    parent: PathBuf,
    solo: PathBuf,
}

fn fixture() -> Fixture {
    let root = TempDir::new().unwrap();
    let child = root.path().join("source/child");
    let parent = root.path().join("source/parent");
    let solo = root.path().join("source/solo");
    write_skill(&child, "child", None);
    write_skill(&parent, "parent", Some(&child));
    write_skill(&solo, "solo", None);
    write_manifest(root.path());
    Fixture { root, parent, solo }
}

fn write_manifest(project: &Path) {
    fs::write(
        project.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".skills\"\nskip_transitive = false\n\n[dependencies]\n",
    )
    .unwrap();
}

/// Flip only the flag, leaving whatever `skill add` wrote untouched.
fn enable_skip_transitive(project: &Path) {
    let manifest = project.join("skill-project.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    assert!(text.contains("skip_transitive = false"), "{text}");
    fs::write(
        manifest,
        text.replace("skip_transitive = false", "skip_transitive = true"),
    )
    .unwrap();
}

fn add(project: &Path, source: &Path) -> super::snapshot_helpers::CommandResult {
    run_fastskill_command(
        &["skill", "add", source.to_str().unwrap(), "--no-reindex"],
        Some(project),
    )
}

fn assert_refused(result: &super::snapshot_helpers::CommandResult) {
    assert!(
        !result.success,
        "expected refusal: {}{}",
        result.stdout, result.stderr
    );
    let output = format!("{}{}", result.stdout, result.stderr);
    assert!(
        output.contains("parent has required dependencies; skip_transitive"),
        "{output}"
    );
}

#[test]
fn skill_add_refuses_a_skill_with_dependencies() {
    let fixture = fixture();
    enable_skip_transitive(fixture.root.path());
    let manifest = fs::read(fixture.root.path().join("skill-project.toml")).unwrap();

    assert_refused(&add(fixture.root.path(), &fixture.parent));

    assert_eq!(
        fs::read(fixture.root.path().join("skill-project.toml")).unwrap(),
        manifest
    );
    assert!(!fixture.root.path().join(".skills/parent").exists());
    assert!(!fixture.root.path().join(".skills/child").exists());
}

#[test]
fn skill_add_accepts_a_skill_without_dependencies() {
    let fixture = fixture();
    enable_skip_transitive(fixture.root.path());

    let result = add(fixture.root.path(), &fixture.solo);

    assert!(result.success, "{}{}", result.stdout, result.stderr);
    assert!(fixture.root.path().join(".skills/solo/SKILL.md").exists());
}

#[test]
fn update_and_locked_install_refuse_once_skip_transitive_is_set() {
    let fixture = fixture();
    let added = add(fixture.root.path(), &fixture.parent);
    assert!(added.success, "{}{}", added.stdout, added.stderr);
    enable_skip_transitive(fixture.root.path());
    let lock = fs::read(fixture.root.path().join("skills.lock")).unwrap();

    assert_refused(&run_fastskill_command(
        &["skill", "update", "--no-reindex"],
        Some(fixture.root.path()),
    ));
    assert_refused(&run_fastskill_command(
        &["project", "install"],
        Some(fixture.root.path()),
    ));

    assert_eq!(
        fs::read(fixture.root.path().join("skills.lock")).unwrap(),
        lock
    );
}

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

/// A shared Manifest whose repository origin names a catalog it declares
/// itself, next to a consuming project that composes it.
fn shared_manifest_with_its_own_catalog(root: &std::path::Path) {
    let catalog = root.join("shared/catalog/review");
    fs::create_dir_all(&catalog).unwrap();
    fs::write(
        catalog.join("SKILL.md"),
        "---\nname: review\nversion: 1.0.0\ndescription: shared catalog\n---\n# Review\n",
    )
    .unwrap();
    fs::write(
        root.join("shared/skill-project.toml"),
        "[dependencies]\n\
         review = { origin = { type = \"repository\", repo = \"team\", skill = \"review\" } }\n\
         [[tool.fastskill.repositories]]\n\
         name = \"team\"\n\
         type = \"local\"\n\
         priority = 1\n\
         path = \"catalog\"\n",
    )
    .unwrap();
}

fn install(root: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root)
        .args(["project", "install", "--no-reindex"])
        .output()
        .unwrap()
}

#[test]
fn a_composed_repository_origin_resolves_through_the_repositories_its_manifest_declares() {
    let root = TempDir::new().unwrap();
    shared_manifest_with_its_own_catalog(root.path());
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"\n\
         [tool.fastskill.manifests]\nteam = \"shared\"\n",
    )
    .unwrap();

    let output = install(root.path());

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let installed = fs::read_to_string(root.path().join("skills/review/SKILL.md")).unwrap();
    assert!(installed.contains("shared catalog"), "{installed}");
}

#[test]
fn a_repository_name_defined_differently_by_composed_manifests_is_refused() {
    let root = TempDir::new().unwrap();
    shared_manifest_with_its_own_catalog(root.path());
    let other = root.path().join("catalog/review");
    fs::create_dir_all(&other).unwrap();
    fs::write(
        other.join("SKILL.md"),
        "---\nname: review\nversion: 1.0.0\ndescription: consumer catalog\n---\n# Review\n",
    )
    .unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"\n\
         [tool.fastskill.manifests]\nteam = \"shared\"\n\
         [[tool.fastskill.repositories]]\n\
         name = \"team\"\n\
         type = \"local\"\n\
         priority = 1\n\
         path = \"catalog\"\n",
    )
    .unwrap();

    let output = install(root.path());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("Repository 'team'"), "{stderr}");
    assert!(!root.path().join("skills/review").exists());
    assert!(!root.path().join("skills.lock").exists());
}

#[test]
fn the_same_repository_declared_by_two_composed_manifests_is_one_repository() {
    let root = TempDir::new().unwrap();
    shared_manifest_with_its_own_catalog(root.path());
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"\n\
         [tool.fastskill.manifests]\nteam = \"shared\"\n\
         [[tool.fastskill.repositories]]\n\
         name = \"team\"\n\
         type = \"local\"\n\
         priority = 1\n\
         path = \"shared/catalog\"\n",
    )
    .unwrap();

    let output = install(root.path());

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.path().join("skills/review/SKILL.md").is_file());
}

#[test]
fn adding_a_repository_never_copies_composed_ones_into_the_project() {
    let root = TempDir::new().unwrap();
    shared_manifest_with_its_own_catalog(root.path());
    fs::create_dir_all(root.path().join("mine")).unwrap();
    fs::write(
        root.path().join("skill-project.toml"),
        "[dependencies]\n[tool.fastskill]\nskills_directory = \"skills\"\n\
         [tool.fastskill.manifests]\nteam = \"shared\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root.path())
        .args(["repo", "add", "mine", "mine", "--repo-type", "local"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let project = fs::read_to_string(root.path().join("skill-project.toml")).unwrap();
    assert!(project.contains("name = \"mine\""), "{project}");
    assert!(!project.contains("name = \"team\""), "{project}");
}

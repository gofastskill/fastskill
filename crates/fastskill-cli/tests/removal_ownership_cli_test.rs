#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use chrono::Utc;
use fastskill_core::core::lock::{
    GlobalLockedSkillEntry, GlobalSkillsLock, ProjectLockedSkillEntry, ProjectSkillsLock,
};
use fastskill_core::core::origin::{Origin, Resolved};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn entry(id: &str, depth: u32, dependencies: &[&str], origin: Origin) -> ProjectLockedSkillEntry {
    ProjectLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin,
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: None,
        },
        dependencies: dependencies.iter().map(|value| value.to_string()).collect(),
        groups: Vec::new(),
        depth,
        parent_skill: None,
        required_by: Vec::new(),
    }
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn run_global(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .arg("--global")
        .args(args)
        .output()
        .unwrap()
}

fn global_entry(
    root: &Path,
    id: &str,
    dependencies: &[&str],
    editable: bool,
) -> GlobalLockedSkillEntry {
    let installed = root.join("config/fastskill/skills").join(id);
    let origin = if editable {
        Origin::Local {
            path: root.join("sources").join(id),
            editable: true,
        }
    } else {
        Origin::Local {
            path: root.join("sources").join(id),
            editable: false,
        }
    };
    GlobalLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin,
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: (!editable)
                .then(|| fastskill_core::core::install::content_digest(&installed).unwrap()),
        },
        dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
        groups: Vec::new(),
        installed_at: Utc::now(),
        last_checked_at: None,
        last_updated_at: None,
    }
}

fn write_global_skill(root: &Path, id: &str, body: &str) {
    let directory = root.join("config/fastskill/skills").join(id);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {id}\nversion: 1.0.0\ndescription: test\n---\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn global_remove_detaches_roots_prunes_only_orphans_and_blocks_required_members() {
    let root = TempDir::new().unwrap();
    for id in ["alpha", "beta", "shared"] {
        write_global_skill(root.path(), id, id);
    }
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots = vec!["alpha".to_string(), "beta".to_string()];
    lock.skills = vec![
        global_entry(root.path(), "alpha", &["shared"], false),
        global_entry(root.path(), "beta", &["shared"], false),
        global_entry(root.path(), "shared", &[], false),
    ];
    let lock_path = root.path().join("config/fastskill/global-skills.lock");
    lock.save_to_file(&lock_path).unwrap();

    let required = run_global(
        root.path(),
        &["skill", "remove", "shared", "--force", "--no-reindex"],
    );
    assert!(!required.status.success());
    assert!(String::from_utf8_lossy(&required.stderr).contains("required by retained root(s)"));

    let removed = run_global(
        root.path(),
        &["skill", "remove", "alpha", "--force", "--no-reindex"],
    );
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!root.path().join("config/fastskill/skills/alpha").exists());
    assert!(root.path().join("config/fastskill/skills/shared").exists());
    let lock = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(lock.covered_roots, vec!["beta"]);
    assert!(lock.skills.iter().any(|entry| entry.id == "shared"));
}

#[test]
fn global_remove_blocks_local_edits_without_poisoning_the_next_attempt() {
    let root = TempDir::new().unwrap();
    write_global_skill(root.path(), "managed", "original");
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots = vec!["managed".to_string()];
    lock.skills = vec![global_entry(root.path(), "managed", &[], false)];
    let lock_path = root.path().join("config/fastskill/global-skills.lock");
    lock.save_to_file(&lock_path).unwrap();
    fs::write(
        root.path().join("config/fastskill/skills/managed/SKILL.md"),
        "edited",
    )
    .unwrap();

    let rejected = run_global(
        root.path(),
        &["skill", "remove", "managed", "--force", "--no-reindex"],
    );
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("locally modified"));
    assert!(!root
        .path()
        .join("config/fastskill/.fastskill/recovery-required")
        .exists());

    write_global_skill(root.path(), "managed", "original");
    let accepted = run_global(
        root.path(),
        &["skill", "remove", "managed", "--force", "--no-reindex"],
    );
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
}

#[cfg(unix)]
#[test]
fn global_remove_unlinks_an_editable_root_and_preserves_its_source() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("sources/editable");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "source sentinel").unwrap();
    let installed = root.path().join("config/fastskill/skills/editable");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&source, &installed).unwrap();
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots = vec!["editable".to_string()];
    lock.skills = vec![global_entry(root.path(), "editable", &[], true)];
    lock.save_to_file(&root.path().join("config/fastskill/global-skills.lock"))
        .unwrap();

    let output = run_global(
        root.path(),
        &["skill", "remove", "editable", "--force", "--no-reindex"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(source.join("SKILL.md").exists());
    assert!(fs::symlink_metadata(&installed).is_err());
}

fn write_project(root: &Path, dependencies: &str, entries: Vec<ProjectLockedSkillEntry>) {
    fs::create_dir_all(root.join("skills")).unwrap();
    fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n{dependencies}"
        ),
    )
    .unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = entries
        .iter()
        .filter(|entry| entry.depth == 0)
        .map(|entry| entry.id.clone())
        .collect();
    lock.skills = entries;
    lock.save_to_file(&root.join("skills.lock")).unwrap();
}

#[test]
fn declared_skill_with_missing_files_can_be_removed_from_a_nested_directory() {
    let project = TempDir::new().unwrap();
    let origin = Origin::Local {
        path: PathBuf::from("origin"),
        editable: false,
    };
    write_project(
        project.path(),
        "missing = { origin = { type = \"local\", path = \"origin\" } }\n",
        vec![entry("missing", 0, &[], origin)],
    );
    let nested = project.path().join("nested/child");
    fs::create_dir_all(&nested).unwrap();

    let output = run(&nested, &["skill", "remove", "missing", "--force"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = fs::read_to_string(project.path().join("skill-project.toml")).unwrap();
    assert!(!manifest.contains("missing ="), "{manifest}");
    let lock = ProjectSkillsLock::load_from_file(&project.path().join("skills.lock")).unwrap();
    assert!(lock.skills.is_empty());
}

#[test]
fn required_transitive_skill_cannot_be_removed_while_root_is_retained() {
    let project = TempDir::new().unwrap();
    let local = |path: &str| Origin::Local {
        path: PathBuf::from(path),
        editable: false,
    };
    write_project(
        project.path(),
        "app = { origin = { type = \"local\", path = \"app-origin\" } }\n",
        vec![
            entry("app", 0, &["shared"], local("app-origin")),
            entry("shared", 1, &[], local("shared-origin")),
        ],
    );
    for id in ["app", "shared"] {
        fs::create_dir_all(project.path().join("skills").join(id)).unwrap();
        fs::write(project.path().join("skills").join(id).join("SKILL.md"), id).unwrap();
    }

    let output = run(project.path(), &["skill", "remove", "shared", "--force"]);

    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("required by retained root(s): app"),
        "{error}"
    );
    assert!(project.path().join("skills/shared/SKILL.md").is_file());
}

#[cfg(unix)]
#[test]
fn removing_an_editable_skill_unlinks_only_the_installation() {
    let project = TempDir::new().unwrap();
    let origin = project.path().join("origin");
    fs::create_dir_all(&origin).unwrap();
    fs::write(origin.join("SKILL.md"), "source sentinel").unwrap();
    write_project(
        project.path(),
        &format!(
            "editable = {{ origin = {{ type = \"local\", path = \"{}\", editable = true }} }}\n",
            origin.display()
        ),
        vec![entry(
            "editable",
            0,
            &[],
            Origin::Local {
                path: origin.clone(),
                editable: true,
            },
        )],
    );
    std::os::unix::fs::symlink(&origin, project.path().join("skills/editable")).unwrap();

    let output = run(project.path(), &["skill", "remove", "editable", "--force"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.path().join("skills/editable").exists());
    assert_eq!(
        fs::read_to_string(origin.join("SKILL.md")).unwrap(),
        "source sentinel"
    );
}

#[test]
fn editable_skill_that_is_not_a_link_is_never_deleted() {
    let project = TempDir::new().unwrap();
    let origin = project.path().join("origin");
    fs::create_dir_all(&origin).unwrap();
    fs::write(origin.join("SKILL.md"), "source sentinel").unwrap();
    write_project(
        project.path(),
        &format!(
            "editable = {{ origin = {{ type = \"local\", path = \"{}\", editable = true }} }}\n",
            origin.display()
        ),
        vec![entry(
            "editable",
            0,
            &[],
            Origin::Local {
                path: origin.clone(),
                editable: true,
            },
        )],
    );
    let installed = project.path().join("skills/editable");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join("SKILL.md"), "installed sentinel").unwrap();

    let output = run(project.path(), &["skill", "remove", "editable", "--force"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not installed as a link"));
    assert!(installed.join("SKILL.md").is_file());
    assert!(origin.join("SKILL.md").is_file());
}

#[test]
fn modified_immutable_skill_is_preserved_and_failed_validation_does_not_poison_state() {
    let project = TempDir::new().unwrap();
    write_project(
        project.path(),
        "managed = { origin = { type = \"local\", path = \"origin\" } }\n",
        vec![entry(
            "managed",
            0,
            &[],
            Origin::Local {
                path: PathBuf::from("origin"),
                editable: false,
            },
        )],
    );
    let installed = project.path().join("skills/managed");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join("SKILL.md"), "original").unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.skills[0].resolved.checksum =
        Some(fastskill_core::core::project_removal::managed_tree_digest(&installed).unwrap());
    lock.save_to_file(&lock_path).unwrap();
    fs::write(installed.join("SKILL.md"), "locally edited").unwrap();

    let rejected = run(project.path(), &["skill", "remove", "managed", "--force"]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("locally modified"));
    assert_eq!(
        fs::read_to_string(installed.join("SKILL.md")).unwrap(),
        "locally edited"
    );
    assert!(!project.path().join(".fastskill/recovery-required").exists());

    fs::write(installed.join("SKILL.md"), "original").unwrap();
    let accepted = run(project.path(), &["skill", "remove", "managed", "--force"]);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert!(!installed.exists());
}

#[test]
fn project_remove_dry_run_json_uses_validated_plan_without_mutation() {
    let project = TempDir::new().unwrap();
    write_project(
        project.path(),
        "managed = { origin = { type = \"local\", path = \"origin\" } }\n",
        vec![entry(
            "managed",
            0,
            &[],
            Origin::Local {
                path: PathBuf::from("origin"),
                editable: false,
            },
        )],
    );
    let installed = project.path().join("skills/managed");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join("SKILL.md"), "managed").unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.skills[0].resolved.checksum =
        Some(fastskill_core::core::project_removal::managed_tree_digest(&installed).unwrap());
    lock.save_to_file(&lock_path).unwrap();
    let manifest_path = project.path().join("skill-project.toml");
    let before_manifest = fs::read(&manifest_path).unwrap();
    let before_lock = fs::read(&lock_path).unwrap();

    let output = run(
        project.path(),
        &["skill", "remove", "managed", "--dry-run", "--json"],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
    assert_eq!(result["scope"], "project");
    assert_eq!(result["outcome"], "changed");
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["targets"][0]["id"], "managed");
    assert!(installed.join("SKILL.md").is_file());
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    assert_eq!(fs::read(&lock_path).unwrap(), before_lock);
}

#[test]
fn project_remove_json_requires_force_and_emits_one_error_without_mutation() {
    let project = TempDir::new().unwrap();
    write_project(
        project.path(),
        "managed = { origin = { type = \"local\", path = \"origin\" } }\n",
        vec![entry(
            "managed",
            0,
            &[],
            Origin::Local {
                path: PathBuf::from("origin"),
                editable: false,
            },
        )],
    );
    let installed = project.path().join("skills/managed");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join("SKILL.md"), "managed").unwrap();
    let lock_path = project.path().join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.skills[0].resolved.checksum =
        Some(fastskill_core::core::project_removal::managed_tree_digest(&installed).unwrap());
    lock.save_to_file(&lock_path).unwrap();
    let manifest_path = project.path().join("skill-project.toml");
    let before_manifest = fs::read(&manifest_path).unwrap();
    let before_lock = fs::read(&lock_path).unwrap();
    let before_skill = fs::read(installed.join("SKILL.md")).unwrap();

    let output = run(project.path(), &["skill", "remove", "managed", "--json"]);

    assert!(!output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["outcome"], "failed");
    assert_eq!(result["dry_run"], false);
    assert!(result["diagnostics"][0]
        .as_str()
        .unwrap()
        .contains("requires --force"));
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(fs::read(&manifest_path).unwrap(), before_manifest);
    assert_eq!(fs::read(&lock_path).unwrap(), before_lock);
    assert_eq!(fs::read(installed.join("SKILL.md")).unwrap(), before_skill);
}

#[test]
fn global_remove_rejects_an_explicit_skills_directory_before_mutation() {
    let project = TempDir::new().unwrap();
    let destination = project.path().join("destination");
    fs::create_dir_all(&destination).unwrap();

    let output = run(
        project.path(),
        &[
            "--global",
            "--skills-dir",
            destination.to_str().unwrap(),
            "skill",
            "remove",
            "anything",
            "--force",
        ],
    );

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("--global and --skills-dir cannot be combined"));
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
}

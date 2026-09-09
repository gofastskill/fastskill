#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use chrono::Utc;
use fastskill_core::core::bundle::BundleService;
use fastskill_core::core::lock::{GlobalLockedSkillEntry, GlobalSkillsLock, ProjectSkillsLock};
use fastskill_core::core::origin::{Origin, Resolved};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn write_skill(path: &Path, id: &str, version: &str, dependencies: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        format!(
            "---\nname: {id}\nversion: \"{version}\"\ndescription: fixture\n---\nBody {version}\n"
        ),
    )
    .unwrap();
    if !dependencies.is_empty() {
        std::fs::write(
            path.join("skill-project.toml"),
            format!(
                "[metadata]\nid = \"{id}\"\nversion = \"{version}\"\n\n[dependencies]\n{dependencies}\n"
            ),
        )
        .unwrap();
    }
}

fn project() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".claude/skills")).unwrap();
    (temp, root)
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(args)
        .current_dir(root)
        .env("FASTSKILL_CACHE_DIR", root.join("cache"))
        .output()
        .unwrap()
}

fn run_with_config(root: &Path, config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(args)
        .current_dir(root)
        .env("XDG_CONFIG_HOME", config)
        .env("FASTSKILL_CACHE_DIR", config.join("cache"))
        .output()
        .unwrap()
}

fn output(result: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    )
}

fn assert_success(result: &Output) {
    assert!(result.status.success(), "{}", output(result));
}

fn write_manifest(root: &Path, dependencies: &str) {
    std::fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n{dependencies}\n"
        ),
    )
    .unwrap();
}

fn build_demo_bundle(author: &Path) -> PathBuf {
    write_skill(&author.join("skills/demo"), "demo", "1.0.0", "");
    std::fs::write(
        author.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[bundle]\nformat = \"fastskill-bundle-v1\"\nid = \"team\"\nversion = \"1.0.0\"\n\n[bundle.members.demo]\noverridable = true\n\n[dependencies]\ndemo = \"1.0.0\"\n",
    )
    .unwrap();
    BundleService::new(author, author.join("skills"))
        .build(author)
        .unwrap()
        .artifact
}

fn write_bundle_project(root: &Path, artifact: &Path, dependencies: &str) {
    std::fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n{dependencies}\n\n[bundles.team]\nversion = \"1.0.0\"\nartifact = {:?}\n",
            artifact
        ),
    )
    .unwrap();
}

#[test]
fn global_install_restores_the_complete_locked_set_without_a_project_manifest() {
    let workspace = TempDir::new().unwrap();
    let config = workspace.path().join("config");
    let source = workspace.path().join("source");
    write_skill(&source, "global-demo", "1.0.0", "");
    let fastskill_config = config.join("fastskill");
    std::fs::create_dir_all(&fastskill_config).unwrap();
    let mut lock = GlobalSkillsLock::new_empty();
    lock.skills.push(GlobalLockedSkillEntry {
        id: "global-demo".to_string(),
        name: "global-demo".to_string(),
        origin: Origin::Local {
            path: source,
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(
                fastskill_core::core::install::content_digest(&workspace.path().join("source"))
                    .unwrap(),
            ),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        installed_at: Utc::now(),
        last_checked_at: None,
        last_updated_at: None,
    });
    lock.save_to_file(&fastskill_config.join("global-skills.lock"))
        .unwrap();

    let result = run_with_config(workspace.path(), &config, &["--global", "install"]);
    assert_success(&result);
    assert!(config
        .join("fastskill/skills/global-demo/SKILL.md")
        .is_file());
    let unchanged = run_with_config(
        workspace.path(),
        &config,
        &["--global", "install", "--dry-run", "--json"],
    );
    assert_success(&unchanged);
    let value: serde_json::Value = serde_json::from_slice(&unchanged.stdout).unwrap();
    assert_eq!(value["scope"], "global");
    assert_eq!(value["outcome"], "unchanged");

    let installed = config.join("fastskill/skills/global-demo/SKILL.md");
    std::fs::write(&installed, "untracked local edit").unwrap();
    let protected = run_with_config(workspace.path(), &config, &["--global", "install"]);
    assert!(!protected.status.success());
    assert!(output(&protected).contains("was modified"));
    assert_eq!(
        std::fs::read_to_string(&installed).unwrap(),
        "untracked local edit"
    );

    let invalid = run_with_config(
        workspace.path(),
        &config,
        &["--global", "--skills-dir", "/tmp", "install"],
    );
    assert!(!invalid.status.success());
    assert!(output(&invalid).contains("cannot be used together"));
}

#[test]
fn cold_install_fetches_and_records_complete_dependency_closure() {
    let (_temp, root) = project();
    let root_skill = root.join("sources/root");
    write_skill(
        &root_skill,
        "root",
        "1.0.0",
        "child = { origin = { type = \"local\", path = \"child\" } }",
    );
    write_skill(&root_skill.join("child"), "child", "1.0.0", "");
    write_manifest(
        &root,
        "root = { origin = { type = \"local\", path = \"sources/root\" } }",
    );

    let result = run(&root, &["install"]);
    assert_success(&result);
    assert!(root.join(".claude/skills/root/SKILL.md").is_file());
    assert!(root.join(".claude/skills/child/SKILL.md").is_file());
    let lock = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert_eq!(lock.covered_roots, vec!["root"]);
    let root_entry = lock.skills.iter().find(|entry| entry.id == "root").unwrap();
    assert_eq!(root_entry.dependencies, vec!["child"]);
    let child = lock
        .skills
        .iter()
        .find(|entry| entry.id == "child")
        .unwrap();
    assert_eq!(child.required_by, vec!["root"]);
    assert!(matches!(
        &child.origin,
        Origin::Local { path, .. } if path == Path::new("sources/root/child")
    ));
    std::fs::remove_dir_all(root.join(".claude/skills/root")).unwrap();
    std::fs::remove_dir_all(root.join(".claude/skills/child")).unwrap();
    assert_success(&run(&root, &["install", "--lock"]));
}

#[test]
fn sibling_local_dependency_records_a_durable_origin_and_restores() {
    let (_temp, root) = project();
    write_skill(
        &root.join("sources/alpha"),
        "alpha",
        "1.0.0",
        "beta = { origin = { type = \"local\", path = \"../beta\" } }",
    );
    write_skill(&root.join("sources/beta"), "beta", "1.0.0", "");
    write_manifest(
        &root,
        "alpha = { origin = { type = \"local\", path = \"sources/alpha\" } }",
    );

    assert_success(&run(&root, &["install"]));
    let lock = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    let beta = lock.skills.iter().find(|entry| entry.id == "beta").unwrap();
    assert!(matches!(
        &beta.origin,
        Origin::Local { path, .. } if path == Path::new("sources/beta")
    ));
    std::fs::remove_dir_all(root.join(".claude/skills/alpha")).unwrap();
    std::fs::remove_dir_all(root.join(".claude/skills/beta")).unwrap();
    assert_success(&run(&root, &["install", "--lock"]));
}

#[test]
fn project_install_honors_the_scoped_skills_directory_override() {
    let (_temp, root) = project();
    write_skill(&root.join("source"), "demo", "1.0.0", "");
    write_manifest(
        &root,
        "demo = { origin = { type = \"local\", path = \"source\" } }",
    );
    let override_dir = root.join("custom-skills");
    std::fs::create_dir_all(&override_dir).unwrap();
    let override_arg = override_dir.to_string_lossy();

    let result = run(
        &root,
        &[
            "--skills-dir",
            override_arg.as_ref(),
            "install",
            "--no-reindex",
        ],
    );

    assert_success(&result);
    assert!(override_dir.join("demo/SKILL.md").is_file());
    assert!(!root.join(".claude/skills/demo").exists());
    assert!(root.join("skills.lock").is_file());
}

#[test]
fn bundle_member_is_managed_for_identical_direct_add_and_local_edits_are_protected() {
    let author = TempDir::new().unwrap();
    let artifact = build_demo_bundle(author.path());
    let source = author.path().join("skills/demo");

    let (_first, first_root) = project();
    write_bundle_project(&first_root, &artifact, "");
    assert_success(&run(&first_root, &["install", "--no-reindex"]));
    let source_arg = source.to_string_lossy();
    let add = run(&first_root, &["add", source_arg.as_ref(), "--no-reindex"]);
    assert_success(&add);
    let lock = ProjectSkillsLock::load_from_file(&first_root.join("skills.lock")).unwrap();
    assert!(lock.skills.iter().any(|entry| entry.id == "demo"));
    assert!(lock.bundles.iter().any(|bundle| bundle.id == "team"));

    let (_second, second_root) = project();
    write_bundle_project(&second_root, &artifact, "");
    assert_success(&run(&second_root, &["install", "--no-reindex"]));
    let installed = second_root.join(".claude/skills/demo/SKILL.md");
    std::fs::write(&installed, "personal edit").unwrap();
    let blocked = run(&second_root, &["add", source_arg.as_ref(), "--no-reindex"]);
    assert!(!blocked.status.success(), "{}", output(&blocked));
    assert!(output(&blocked).contains("was modified"));
    assert_eq!(std::fs::read_to_string(installed).unwrap(), "personal edit");
}

#[test]
fn repeated_install_reports_unchanged_with_a_valid_bundle_override() {
    let author = TempDir::new().unwrap();
    let artifact = build_demo_bundle(author.path());
    let (_project, root) = project();
    write_bundle_project(&root, &artifact, "");
    assert_success(&run(&root, &["install", "--no-reindex"]));

    let personal = root.join("personal-demo");
    write_skill(&personal, "demo", "2.0.0", "");
    let personal_arg = personal.to_string_lossy();
    let override_result = run(
        &root,
        &[
            "bundle",
            "override",
            "demo",
            "--from",
            personal_arg.as_ref(),
            "--no-reindex",
        ],
    );
    assert_success(&override_result);
    let installed = root.join(".claude/skills/demo/SKILL.md");
    let override_bytes = std::fs::read(&installed).unwrap();

    for args in [
        &["install", "--no-reindex", "--json"][..],
        &["install", "--lock", "--no-reindex", "--json"][..],
    ] {
        let repeated = run(&root, args);
        assert_success(&repeated);
        let value: serde_json::Value = serde_json::from_slice(&repeated.stdout).unwrap();
        assert_eq!(value["outcome"], "unchanged", "{}", output(&repeated));
        assert_eq!(std::fs::read(&installed).unwrap(), override_bytes);
    }
}

#[test]
fn dropping_an_ordinary_edge_retains_bundle_owned_content() {
    let author = TempDir::new().unwrap();
    let artifact = build_demo_bundle(author.path());
    let (_project, root) = project();
    let root_source = root.join("root-source");
    write_skill(
        &root_source,
        "root",
        "1.0.0",
        "demo = { origin = { type = \"local\", path = \"demo\" } }",
    );
    write_skill(&root_source.join("demo"), "demo", "1.0.0", "");
    write_bundle_project(
        &root,
        &artifact,
        "root = { origin = { type = \"local\", path = \"root-source\" } }",
    );
    assert_success(&run(&root, &["install", "--no-reindex"]));
    let first = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert!(first.skills.iter().any(|entry| entry.id == "demo"));

    write_skill(&root_source, "root", "2.0.0", "");
    std::fs::remove_file(root_source.join("skill-project.toml")).unwrap();
    let update = run(&root, &["update", "root", "--no-reindex"]);

    assert_success(&update);
    let final_lock = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert!(!final_lock.skills.iter().any(|entry| entry.id == "demo"));
    assert!(final_lock
        .bundles
        .iter()
        .any(|bundle| { bundle.members.iter().any(|member| member.id == "demo") }));
    assert!(root.join(".claude/skills/demo/SKILL.md").is_file());
}

#[test]
fn cold_offline_install_accepts_available_local_dependency_closure() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(
        &source,
        "root",
        "1.0.0",
        "child = { origin = { type = \"local\", path = \"child\" } }",
    );
    write_skill(&source.join("child"), "child", "1.0.0", "");
    write_manifest(
        &root,
        "root = { origin = { type = \"local\", path = \"source\" } }",
    );

    let result = run(&root, &["install", "--offline"]);

    assert_success(&result);
    assert!(root.join(".claude/skills/root/SKILL.md").is_file());
    assert!(root.join(".claude/skills/child/SKILL.md").is_file());
    let lock = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert_eq!(lock.covered_roots, vec!["root"]);
}

#[test]
fn strict_and_ordinary_restore_do_not_accept_changed_local_snapshot() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(&source, "demo", "1.0.0", "");
    write_manifest(
        &root,
        "demo = { origin = { type = \"local\", path = \"source\" } }",
    );
    assert_success(&run(&root, &["install"]));
    let lock_before = std::fs::read(root.join("skills.lock")).unwrap();
    write_skill(&source, "demo", "2.0.0", "");

    for args in [&["install", "--lock"][..], &["install"][..]] {
        let result = run(&root, args);
        assert!(!result.status.success(), "{}", output(&result));
        assert!(
            output(&result).contains("locked version") || output(&result).contains("checksum"),
            "{}",
            output(&result)
        );
        assert_eq!(
            std::fs::read(root.join("skills.lock")).unwrap(),
            lock_before
        );
        let installed = std::fs::read_to_string(root.join(".claude/skills/demo/SKILL.md")).unwrap();
        assert!(installed.contains("1.0.0"));
    }
}

#[test]
fn update_refuses_to_replace_untracked_installed_edits() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(&source, "demo", "1.0.0", "");
    write_manifest(
        &root,
        "demo = { origin = { type = \"local\", path = \"source\" } }",
    );
    assert_success(&run(&root, &["install"]));
    let installed = root.join(".claude/skills/demo/SKILL.md");
    std::fs::write(&installed, "personal untracked edit").unwrap();
    write_skill(&source, "demo", "2.0.0", "");
    let lock_before = std::fs::read(root.join("skills.lock")).unwrap();

    let result = run(&root, &["update", "demo"]);

    assert!(!result.status.success(), "{}", output(&result));
    assert!(
        output(&result).contains("was modified"),
        "{}",
        output(&result)
    );
    assert_eq!(
        std::fs::read_to_string(installed).unwrap(),
        "personal untracked edit"
    );
    assert_eq!(
        std::fs::read(root.join("skills.lock")).unwrap(),
        lock_before
    );
}

#[test]
fn group_limited_install_tracks_coverage_and_strict_requires_it() {
    let (_temp, root) = project();
    write_skill(&root.join("default"), "default-skill", "1.0.0", "");
    write_skill(&root.join("dev"), "dev-skill", "1.0.0", "");
    write_manifest(
        &root,
        "default-skill = { origin = { type = \"local\", path = \"default\" } }\n\
         dev-skill = { origin = { type = \"local\", path = \"dev\" }, groups = [\"dev\"] }",
    );

    assert_success(&run(&root, &["install", "--only", "dev"]));
    let first = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert_eq!(first.covered_roots, vec!["dev-skill"]);
    assert!(!run(&root, &["install", "--lock", "--only", "default"])
        .status
        .success());

    assert_success(&run(&root, &["install", "--only", "default"]));
    let second = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert_eq!(second.covered_roots, vec!["default-skill", "dev-skill"]);
    assert!(second.skills.iter().any(|entry| entry.id == "dev-skill"));
}

#[test]
fn invalid_depth_and_wrong_identity_fail_before_mutation() {
    let (_temp, root) = project();
    write_skill(&root.join("source"), "actual", "1.0.0", "");
    write_manifest(
        &root,
        "expected = { origin = { type = \"local\", path = \"source\" } }",
    );

    let negative = run(&root, &["install", "--depth=-1"]);
    assert!(!negative.status.success());
    assert!(!root.join("skills.lock").exists());
    let wrong_id = run(&root, &["install"]);
    assert!(!wrong_id.status.success());
    assert!(output(&wrong_id).contains("wrong identity"));
    assert!(!root.join(".claude/skills/actual").exists());
}

#[test]
fn depth_one_rejects_a_required_child_before_installing_root() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(
        &source,
        "root",
        "1.0.0",
        "child = { origin = { type = \"local\", path = \"child\" } }",
    );
    write_skill(&source.join("child"), "child", "1.0.0", "");
    write_manifest(
        &root,
        "root = { origin = { type = \"local\", path = \"source\" } }",
    );

    let result = run(&root, &["install", "--depth", "1"]);
    assert!(!result.status.success(), "{}", output(&result));
    assert!(output(&result).contains("root -> child"));
    assert!(!root.join(".claude/skills/root").exists());
    assert!(!root.join("skills.lock").exists());
}

#[test]
fn dependency_cycle_across_two_selected_roots_is_rejected_before_apply() {
    let (_temp, root) = project();
    write_skill(
        &root.join("sources/alpha"),
        "alpha",
        "1.0.0",
        "beta = { origin = { type = \"local\", path = \"../beta\" } }",
    );
    write_skill(
        &root.join("sources/beta"),
        "beta",
        "1.0.0",
        "alpha = { origin = { type = \"local\", path = \"../alpha\" } }",
    );
    write_manifest(
        &root,
        "alpha = { origin = { type = \"local\", path = \"sources/alpha\" } }\n\
         beta = { origin = { type = \"local\", path = \"sources/beta\" } }",
    );

    let result = run(&root, &["install"]);

    assert!(!result.status.success(), "{}", output(&result));
    assert!(output(&result).contains("circular dependency"));
    assert!(!root.join("skills.lock").exists());
    assert!(!root.join(".claude/skills/alpha").exists());
    assert!(!root.join(".claude/skills/beta").exists());
}

#[test]
fn add_dry_run_json_plans_complete_closure_without_mutation() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(
        &source,
        "root",
        "1.0.0",
        "child = { origin = { type = \"local\", path = \"child\" } }",
    );
    write_skill(&source.join("child"), "child", "1.0.0", "");
    write_manifest(&root, "");
    let manifest_before = std::fs::read(root.join("skill-project.toml")).unwrap();

    let preview = run(&root, &["add", "./source", "--dry-run", "--json"]);
    assert_success(&preview);
    let value: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["scope"], "project");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["targets"][0]["id"], "root");
    assert_eq!(
        std::fs::read(root.join("skill-project.toml")).unwrap(),
        manifest_before
    );
    assert!(!root.join("skills.lock").exists());
    assert!(!root.join(".claude/skills/root").exists());
    assert!(!root.join(".claude/skills/child").exists());

    assert_success(&run(&root, &["add", "./source"]));
    assert!(root.join(".claude/skills/root/SKILL.md").is_file());
    assert!(root.join(".claude/skills/child/SKILL.md").is_file());
    let lock = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert!(lock.skills.iter().any(|entry| entry.id == "root"));
    assert!(lock.skills.iter().any(|entry| entry.id == "child"));
}

#[test]
fn adding_a_root_merges_shared_ownership_and_rejects_incompatible_shared_content() {
    let (_temp, root) = project();
    write_skill(&root.join("sources/shared"), "shared", "1.0.0", "");
    write_skill(&root.join("sources/other-shared"), "shared", "2.0.0", "");
    for id in ["alpha", "beta"] {
        write_skill(
            &root.join("sources").join(id),
            id,
            "1.0.0",
            "shared = { origin = { type = \"local\", path = \"../shared\" } }",
        );
    }
    write_skill(
        &root.join("sources/gamma"),
        "gamma",
        "1.0.0",
        "shared = { origin = { type = \"local\", path = \"../other-shared\" } }",
    );
    write_manifest(&root, "");

    assert_success(&run(&root, &["add", "./sources/alpha"]));
    assert_success(&run(&root, &["add", "./sources/beta"]));
    let lock_path = root.join("skills.lock");
    let lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(
        lock.skills
            .iter()
            .find(|entry| entry.id == "shared")
            .unwrap()
            .required_by,
        vec!["alpha", "beta"]
    );
    let before = std::fs::read(&lock_path).unwrap();

    let rejected = run(&root, &["add", "./sources/gamma"]);

    assert!(!rejected.status.success(), "{}", output(&rejected));
    assert!(output(&rejected).contains("retained roots"));
    assert_eq!(std::fs::read(lock_path).unwrap(), before);
    assert!(!root.join(".claude/skills/gamma").exists());
    assert!(
        std::fs::read_to_string(root.join(".claude/skills/shared/SKILL.md"))
            .unwrap()
            .contains("1.0.0")
    );
}

#[test]
fn add_refuses_to_replace_an_unmanaged_destination() {
    let (_temp, root) = project();
    write_skill(&root.join("source"), "alpha", "2.0.0", "");
    write_manifest(&root, "");
    let destination = root.join(".claude/skills/alpha");
    write_skill(&destination, "alpha", "1.0.0", "");
    std::fs::write(destination.join("my-work.txt"), "keep me").unwrap();

    let result = run(&root, &["add", "./source"]);

    assert!(!result.status.success(), "{}", output(&result));
    assert!(output(&result).contains("unmanaged"));
    assert_eq!(
        std::fs::read_to_string(destination.join("my-work.txt")).unwrap(),
        "keep me"
    );
    assert!(!root.join("skills.lock").exists());
}

#[test]
fn update_replans_changed_dependency_closure_and_preview_is_non_mutating() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(
        &source,
        "root",
        "1.0.0",
        "old-child = { origin = { type = \"local\", path = \"old-child\" } }",
    );
    write_skill(&source.join("old-child"), "old-child", "1.0.0", "");
    write_manifest(
        &root,
        "root = { origin = { type = \"local\", path = \"source\" } }",
    );
    assert_success(&run(&root, &["install"]));
    let lock_before = std::fs::read(root.join("skills.lock")).unwrap();
    let installed_before = std::fs::read(root.join(".claude/skills/root/SKILL.md")).unwrap();

    write_skill(
        &source,
        "root",
        "2.0.0",
        "new-child = { origin = { type = \"local\", path = \"new-child\" } }",
    );
    write_skill(&source.join("new-child"), "new-child", "1.0.0", "");
    let preview = run(&root, &["update", "root", "--dry-run", "--json"]);
    assert_success(&preview);
    let value: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["targets"][0]["current_revision"], "1.0.0");
    assert_eq!(value["targets"][0]["target_revision"], "2.0.0");
    assert_eq!(
        std::fs::read(root.join("skills.lock")).unwrap(),
        lock_before
    );
    assert_eq!(
        std::fs::read(root.join(".claude/skills/root/SKILL.md")).unwrap(),
        installed_before
    );

    assert_success(&run(&root, &["update", "root"]));
    let lock = ProjectSkillsLock::load_from_file(&root.join("skills.lock")).unwrap();
    assert!(lock.skills.iter().any(|entry| entry.id == "root"));
    assert!(lock.skills.iter().any(|entry| entry.id == "new-child"));
    assert!(!lock.skills.iter().any(|entry| entry.id == "old-child"));
    assert!(root.join(".claude/skills/new-child/SKILL.md").is_file());
    assert!(!root.join(".claude/skills/old-child").exists());
}

#[test]
fn unchanged_update_does_not_rewrite_project_state_or_installed_content() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(&source, "demo", "1.0.0", "");
    write_manifest(
        &root,
        "demo = { origin = { type = \"local\", path = \"source\" } }",
    );
    assert_success(&run(&root, &["install"]));
    let manifest_before = std::fs::read(root.join("skill-project.toml")).unwrap();
    let lock_before = std::fs::read(root.join("skills.lock")).unwrap();
    let installed = root.join(".claude/skills/demo/SKILL.md");
    let modified_before = std::fs::metadata(&installed).unwrap().modified().unwrap();

    let result = run(&root, &["update", "demo", "--json"]);

    assert_success(&result);
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["outcome"], "unchanged");
    assert_eq!(
        std::fs::read(root.join("skill-project.toml")).unwrap(),
        manifest_before
    );
    assert_eq!(
        std::fs::read(root.join("skills.lock")).unwrap(),
        lock_before
    );
    assert_eq!(
        std::fs::metadata(installed).unwrap().modified().unwrap(),
        modified_before
    );
}

#[test]
fn strict_restore_rejects_missing_integrity_evidence_in_transitive_lock_entry() {
    let (_temp, root) = project();
    let source = root.join("source");
    write_skill(
        &source,
        "root",
        "1.0.0",
        "child = { origin = { type = \"local\", path = \"child\" } }",
    );
    write_skill(&source.join("child"), "child", "1.0.0", "");
    write_manifest(
        &root,
        "root = { origin = { type = \"local\", path = \"source\" } }",
    );
    assert_success(&run(&root, &["install"]));
    let lock_path = root.join("skills.lock");
    let mut lock = ProjectSkillsLock::load_from_file(&lock_path).unwrap();
    lock.skills
        .iter_mut()
        .find(|entry| entry.id == "child")
        .unwrap()
        .resolved
        .checksum = None;
    lock.save_to_file(&lock_path).unwrap();
    std::fs::remove_dir_all(root.join(".claude/skills/root")).unwrap();
    std::fs::remove_dir_all(root.join(".claude/skills/child")).unwrap();
    let before = std::fs::read(&lock_path).unwrap();

    let result = run(&root, &["install", "--lock"]);

    assert!(!result.status.success(), "{}", output(&result));
    assert!(
        output(&result).contains("no content digest"),
        "{}",
        output(&result)
    );
    assert_eq!(std::fs::read(lock_path).unwrap(), before);
    assert!(!root.join(".claude/skills/root").exists());
    assert!(!root.join(".claude/skills/child").exists());
}

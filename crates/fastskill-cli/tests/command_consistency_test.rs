#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use chrono::Utc;
use fastskill_core::core::lock::{
    GlobalLockedSkillEntry, GlobalSkillsLock, ProjectLockedSkillEntry, ProjectSkillsLock,
};
use fastskill_core::core::origin::{Origin, Resolved};
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn run(root: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(args)
        .current_dir(cwd)
        .env("FASTSKILL_CACHE_DIR", root.join("cache"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root)
        .output()
        .unwrap()
}

fn write_project(root: &Path, dependencies: &str) {
    std::fs::create_dir_all(root.join(".claude/skills")).unwrap();
    std::fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n{dependencies}\n"
        ),
    )
    .unwrap();
}

fn write_skill(root: &Path, id: &str, version: &str) {
    let directory = root.join(".claude/skills").join(id);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {id}\nversion: \"{version}\"\ndescription: fixture\n---\n# {id}\n"),
    )
    .unwrap();
}

fn write_skill_directory(directory: &Path, id: &str, version: &str, body: &str) {
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {id}\nversion: \"{version}\"\ndescription: fixture\n---\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn global_update_prunes_dependencies_no_longer_reachable_from_any_root() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let config = root.join("config/fastskill");
    let storage = config.join("skills");
    let source = root.join("source/root-skill");
    write_skill_directory(&source, "root-skill", "2.0.0", "updated root without child");
    write_skill_directory(
        &storage.join("root-skill"),
        "root-skill",
        "1.0.0",
        "old root",
    );
    write_skill_directory(
        &storage.join("obsolete-child"),
        "obsolete-child",
        "1.0.0",
        "old child",
    );
    let checksum = |id: &str| {
        fastskill_core::core::project_removal::managed_tree_digest(&storage.join(id)).unwrap()
    };
    let now = Utc::now();
    let mut lock = GlobalSkillsLock::new_empty();
    lock.covered_roots = vec!["root-skill".to_string()];
    lock.skills.push(GlobalLockedSkillEntry {
        id: "root-skill".to_string(),
        name: "root-skill".to_string(),
        origin: Origin::Local {
            path: source,
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(checksum("root-skill")),
        },
        dependencies: vec!["obsolete-child".to_string()],
        groups: Vec::new(),
        installed_at: now,
        last_checked_at: None,
        last_updated_at: None,
    });
    lock.skills.push(GlobalLockedSkillEntry {
        id: "obsolete-child".to_string(),
        name: "obsolete-child".to_string(),
        origin: Origin::Local {
            path: root.join("source/obsolete-child"),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(checksum("obsolete-child")),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        installed_at: now,
        last_checked_at: None,
        last_updated_at: None,
    });
    std::fs::create_dir_all(&config).unwrap();
    lock.save_to_file(&config.join("global-skills.lock"))
        .unwrap();

    let result = run(
        root,
        root,
        &[
            "--global",
            "skill",
            "update",
            "root-skill",
            "--no-reindex",
            "--json",
        ],
    );
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let result_json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(result_json["targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|target| {
            target["id"] == "obsolete-child" && target["target_revision"] == "removed"
        }));
    assert!(!storage.join("obsolete-child").exists());
    let reloaded = GlobalSkillsLock::load_from_file(&config.join("global-skills.lock")).unwrap();
    assert_eq!(reloaded.covered_roots, ["root-skill"]);
    assert!(reloaded
        .skills
        .iter()
        .all(|entry| entry.id != "obsolete-child"));
}

#[test]
fn lifecycle_json_domain_errors_emit_one_parseable_failure_object() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    write_project(&project, "");
    let update_project = root.join("update-project");
    std::fs::create_dir_all(&update_project).unwrap();
    write_project(
        &update_project,
        "demo = { origin = { type = \"local\", path = \"source/demo\" } }",
    );

    let cases: &[(&Path, &[&str])] = &[
        (&project, &["skill", "add", "missing/local-skill", "--json"]),
        (&update_project, &["skill", "update", "--json"]),
        (
            &project,
            &[
                "--global",
                "--skills-dir",
                "custom",
                "skill",
                "remove",
                "missing-skill",
                "--json",
            ],
        ),
    ];
    for (cwd, args) in cases {
        let result = run(root, cwd, args);
        assert!(
            !result.status.success(),
            "{} unexpectedly succeeded",
            args.join(" ")
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap_or_else(|e| {
            panic!(
                "{} did not emit one JSON value: {e}; stdout={} stderr={}",
                args.join(" "),
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            )
        });
        assert_eq!(value["outcome"], "failed", "{}", args.join(" "));
        assert!(value["diagnostics"].as_array().is_some());
    }

    let no_project = root.join("no-project");
    std::fs::create_dir_all(&no_project).unwrap();
    let install = run(root, &no_project, &["project", "install", "--json"]);
    assert!(!install.status.success());
    let value: serde_json::Value = serde_json::from_slice(&install.stdout).unwrap();
    assert!(matches!(
        value["outcome"].as_str(),
        Some("failed" | "blocked")
    ));
    assert!(value["diagnostics"].as_array().is_some());
}

#[test]
fn non_lifecycle_json_errors_emit_one_parseable_failure_object() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let cases: &[&[&str]] = &[
        &["skill", "list", "--json"],
        &["skill", "list", "--format", "json"],
        &["skill", "search", "query", "--json"],
    ];

    for args in cases {
        let result = run(root, root, args);
        assert!(
            !result.status.success(),
            "{} unexpectedly succeeded",
            args.join(" ")
        );
        let value: serde_json::Value =
            serde_json::from_slice(&result.stdout).unwrap_or_else(|error| {
                panic!(
                    "{} did not emit one JSON value: {error}; stdout={} stderr={}",
                    args.join(" "),
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                )
            });
        assert_eq!(value["success"], false, "{}", args.join(" "));
        assert!(value["error"]["message"].as_str().is_some());
    }

    let bundle = run(root, root, &["bundle", "override", "member", "--json"]);
    assert!(!bundle.status.success());
    let value: serde_json::Value = serde_json::from_slice(&bundle.stdout).unwrap();
    assert_eq!(value["outcome"], "failed");
    assert!(value["diagnostics"].as_array().is_some());
}

#[test]
fn cache_commands_keep_machine_output_parseable_at_the_dispatch_boundary() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    for args in [
        &["cache", "info", "--json"][..],
        &["cache", "clean", "--json"][..],
    ] {
        let result = run(root, root, args);
        assert!(
            result.status.success(),
            "{}: {}",
            args.join(" "),
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap_or_else(|error| {
            panic!(
                "{} did not emit one JSON value: {error}; stdout={}",
                args.join(" "),
                String::from_utf8_lossy(&result.stdout)
            )
        });
    }
}

#[test]
fn add_preview_rejects_an_unmanaged_destination_with_one_json_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(root, "");
    let source = root.join("source/demo");
    write_skill_directory(&source, "demo", "1.0.0", "source content");
    let unmanaged = root.join(".claude/skills/demo");
    write_skill_directory(&unmanaged, "demo", "0.1.0", "unmanaged content");
    let before = std::fs::read(unmanaged.join("SKILL.md")).unwrap();
    let source_arg = source.to_string_lossy().to_string();

    let result = run(
        root,
        root,
        &["skill", "add", &source_arg, "--dry-run", "--json"],
    );

    assert!(!result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(matches!(
        value["outcome"].as_str(),
        Some("blocked" | "failed")
    ));
    assert_eq!(std::fs::read(unmanaged.join("SKILL.md")).unwrap(), before);
    assert!(!root.join("skills.lock").exists());
}

#[test]
fn read_locked_from_nested_directory_returns_one_combined_json_value() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(
        root,
        "demo = { origin = { type = \"local\", path = \"source\" } }",
    );
    write_skill(root, "demo", "2.0.0");
    let mut lock = ProjectSkillsLock::new_empty();
    lock.skills.push(ProjectLockedSkillEntry {
        id: "demo".to_string(),
        name: "demo".to_string(),
        origin: Origin::Local {
            path: "source".into(),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some("fixture".to_string()),
        },
        dependencies: vec!["locked-child".to_string()],
        groups: Vec::new(),
        depth: 0,
        parent_skill: None,
        required_by: Vec::new(),
    });
    lock.save_to_file(&root.join("skills.lock")).unwrap();
    let nested = root.join("nested/deeper");
    std::fs::create_dir_all(&nested).unwrap();

    let result = run(
        root,
        &nested,
        &[
            "skill", "read", "demo", "--locked", "--meta", "--tree", "--json",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["metadata"]["version"], "1.0.0");
    assert_eq!(value["actual_tree"]["id"], "demo");

    let versioned = run(root, root, &["skill", "read", "demo@1.0.0"]);
    assert!(!versioned.status.success());
    assert!(String::from_utf8_lossy(&versioned.stderr).contains("repo versions"));
}

#[test]
fn read_locked_uses_global_scope_without_ambient_project_lock() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(root, "");
    write_skill(root, "global-demo", "2.0.0");
    let mut lock = GlobalSkillsLock::new_empty();
    lock.skills.push(GlobalLockedSkillEntry {
        id: "global-demo".to_string(),
        name: "global-demo".to_string(),
        origin: Origin::Local {
            path: "global-source".into(),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some("fixture".to_string()),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        installed_at: Utc::now(),
        last_checked_at: None,
        last_updated_at: None,
    });
    let config = root.join("config/fastskill");
    std::fs::create_dir_all(&config).unwrap();
    lock.save_to_file(&config.join("global-skills.lock"))
        .unwrap();

    let result = run(
        root,
        root,
        &[
            "--global",
            "skill",
            "read",
            "global-demo",
            "--locked",
            "--meta",
            "--json",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value[0]["version"], "1.0.0");
}

#[test]
fn list_check_detects_missing_declared_state_and_keeps_json_parseable() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(root, "missing = \"2.0.0\"");

    let result = run(root, root, &["skill", "list", "--check", "--json"]);
    assert!(!result.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(rows[0]["id"], "missing");
    assert_eq!(rows[0]["reconciliation"], "missing-lock");
}

#[test]
fn list_check_reports_manifest_constraint_different_from_lock() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(
        root,
        "demo = { origin = { type = \"repository\", repo = \"team\", skill = \"demo\", version = \"2.0.0\" } }",
    );
    write_skill(root, "demo", "1.0.0");
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots.push("demo".to_string());
    lock.skills.push(ProjectLockedSkillEntry {
        id: "demo".to_string(),
        name: "demo".to_string(),
        origin: Origin::Repository {
            repo: "team".to_string(),
            skill: "demo".to_string(),
            version: Some(fastskill_core::core::VersionConstraint::parse("1.0.0").unwrap()),
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some("fixture".to_string()),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        depth: 0,
        parent_skill: None,
        required_by: Vec::new(),
    });
    lock.save_to_file(&root.join("skills.lock")).unwrap();

    let result = run(root, root, &["skill", "list", "--check", "--json"]);
    assert!(!result.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(rows[0]["desired_constraint"], "=2.0.0");
    assert_eq!(rows[0]["locked_version"], "1.0.0");
    assert_eq!(rows[0]["reconciliation"], "intent-mismatch");
}

#[test]
fn search_requires_query_and_empty_json_is_an_array() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(root, "");

    let missing = run(root, root, &["skill", "search"]);
    assert!(!missing.status.success());
    let empty = run(
        root,
        root,
        &["skill", "search", "no-such-skill", "--local", "--json"],
    );
    assert!(
        empty.status.success(),
        "{}",
        String::from_utf8_lossy(&empty.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert_eq!(value, serde_json::json!([]));
}

#[test]
fn global_add_rejects_skills_directory_override_before_mutation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\nversion: \"1.0.0\"\ndescription: fixture\n---\n# Demo\n",
    )
    .unwrap();
    let target = root.join("requested");
    let source_arg = source.to_string_lossy().to_string();
    let target_arg = target.to_string_lossy().to_string();
    let result = run(
        root,
        root,
        &[
            "--global",
            "--skills-dir",
            &target_arg,
            "skill",
            "add",
            &source_arg,
        ],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be used together"));
    assert!(!target.exists());
}

#[test]
fn global_update_rejects_skills_directory_override_before_mutation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let target = root.join("requested");
    let target_arg = target.to_string_lossy().to_string();
    let result = run(
        root,
        root,
        &["--global", "--skills-dir", &target_arg, "skill", "update"],
    );

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be combined"));
    assert!(!target.exists());
}

#[test]
fn global_add_and_update_share_preview_json_and_apply_real_content() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: global-demo\nversion: \"1.0.0\"\ndescription: fixture\n---\n# One\n",
    )
    .unwrap();
    let source_arg = source.to_string_lossy().to_string();
    let lock_path = root.join("config/fastskill/global-skills.lock");
    let installed = root.join("config/fastskill/skills/global-demo/SKILL.md");

    let preview = run(
        root,
        root,
        &[
            "--global",
            "skill",
            "add",
            &source_arg,
            "--dry-run",
            "--json",
        ],
    );
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["scope"], "global");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["targets"][0]["id"], "global-demo");
    assert!(!lock_path.exists());
    assert!(!installed.exists());

    let added = run(
        root,
        root,
        &["--global", "skill", "add", &source_arg, "--json"],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&added.stdout).unwrap();
    assert_eq!(value["outcome"], "changed");
    assert!(lock_path.exists());
    assert!(installed.exists());

    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: global-demo\nversion: \"2.0.0\"\ndescription: fixture\n---\n# Two\n",
    )
    .unwrap();
    let updated = run(
        root,
        root,
        &["--global", "skill", "update", "global-demo", "--json"],
    );
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(value["scope"], "global");
    assert_eq!(value["outcome"], "changed");
    assert_eq!(value["targets"][0]["target_revision"], "2.0.0");
    assert!(std::fs::read_to_string(&installed)
        .unwrap()
        .contains("# Two"));
    let lock = GlobalSkillsLock::load_from_file(&lock_path).unwrap();
    assert_eq!(lock.skills[0].resolved.version, "2.0.0");
}

#[test]
fn global_list_explains_direct_and_transitive_owners_with_root_group_selection() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let mut lock = GlobalSkillsLock::new_empty();
    let entry = |id: &str, dependencies: Vec<String>, groups: Vec<String>| GlobalLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin: Origin::Local {
            path: root.join(id),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some("fixture".to_string()),
        },
        dependencies,
        groups,
        installed_at: Utc::now(),
        last_checked_at: None,
        last_updated_at: None,
    };
    lock.covered_roots.push("root-skill".to_string());
    lock.skills.push(entry(
        "root-skill",
        vec!["child-skill".to_string()],
        vec!["dev".to_string()],
    ));
    lock.skills
        .push(entry("child-skill", Vec::new(), Vec::new()));
    let config = root.join("config/fastskill");
    std::fs::create_dir_all(&config).unwrap();
    lock.save_to_file(&config.join("global-skills.lock"))
        .unwrap();

    let result = run(
        root,
        root,
        &["--global", "skill", "list", "--only", "dev", "--json"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let child = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "child-skill")
        .unwrap();
    assert!(child["owners"]
        .as_array()
        .unwrap()
        .iter()
        .any(|owner| owner == "global-root:root-skill"));
    assert!(child["owners"]
        .as_array()
        .unwrap()
        .iter()
        .any(|owner| owner == "required-by:root-skill"));
    let direct = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "root-skill")
        .unwrap();
    assert!(direct["owners"]
        .as_array()
        .unwrap()
        .iter()
        .any(|owner| owner == "direct:global"));
}

#[test]
fn recursive_add_dry_run_and_json_use_one_multi_root_plan() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(root, "");
    let collection = root.join("collection");
    for id in ["one", "two"] {
        let skill = collection.join(id);
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {id}\nversion: \"1.0.0\"\ndescription: fixture\n---\n# {id}\n"),
        )
        .unwrap();
    }
    let source = collection.to_string_lossy().to_string();

    let preview = run(
        root,
        root,
        &[
            "skill",
            "add",
            &source,
            "--recursive",
            "--dry-run",
            "--json",
        ],
    );
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["targets"].as_array().unwrap().len(), 2);
    assert!(!root.join("skills.lock").exists());
    assert!(!root.join(".claude/skills/one").exists());

    let applied = run(
        root,
        root,
        &["skill", "add", &source, "--recursive", "--json"],
    );
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(value["targets"].as_array().unwrap().len(), 2);
    assert!(root.join(".claude/skills/one/SKILL.md").exists());
    assert!(root.join(".claude/skills/two/SKILL.md").exists());
    let manifest = std::fs::read_to_string(root.join("skill-project.toml")).unwrap();
    assert!(manifest.contains("one"));
    assert!(manifest.contains("two"));
}

#[test]
fn add_rejects_invalid_or_conflicting_source_selectors_before_mutation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let unknown = run(
        root,
        root,
        &["skill", "add", "demo", "--source-type", "unknown"],
    );
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("invalid value"));

    let conflicting = run(
        root,
        root,
        &[
            "skill",
            "add",
            "https://example.invalid/demo.git",
            "--branch",
            "main",
            "--tag",
            "v1",
        ],
    );
    assert!(!conflicting.status.success());
    assert!(String::from_utf8_lossy(&conflicting.stderr).contains("cannot be used together"));

    let inapplicable = run(root, root, &["skill", "add", "demo", "--branch", "main"]);
    assert!(!inapplicable.status.success());
    assert!(String::from_utf8_lossy(&inapplicable.stderr).contains("only valid for Git"));
    assert!(!root.join("skill-project.toml").exists());
    assert!(!root.join("skills.lock").exists());
}

#[test]
fn local_repository_catalog_lists_and_rejects_http_only_options() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(root, "");
    let catalog = root.join("catalog");
    let catalog_skill = catalog.join("catalog-demo");
    std::fs::create_dir_all(&catalog_skill).unwrap();
    std::fs::write(
        catalog_skill.join("SKILL.md"),
        "---\nname: catalog-demo\nversion: \"1.2.3\"\ndescription: fixture\n---\n# Demo\n",
    )
    .unwrap();
    let catalog_arg = catalog.to_string_lossy().to_string();
    let added = run(
        root,
        root,
        &[
            "repo",
            "add",
            "local-fixture",
            &catalog_arg,
            "--repo-type",
            "local",
        ],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let listed = run(
        root,
        root,
        &["repo", "skills", "--repository", "local-fixture", "--json"],
    );
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let skills: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(skills[0]["id"], "catalog-demo");
    let shown = run(
        root,
        root,
        &[
            "repo",
            "show",
            "catalog-demo",
            "--repository",
            "local-fixture",
        ],
    );
    assert!(shown.status.success());
    assert!(String::from_utf8_lossy(&shown.stdout).contains("Version: 1.2.3"));
    let versions = run(
        root,
        root,
        &[
            "repo",
            "versions",
            "catalog-demo",
            "--repository",
            "local-fixture",
        ],
    );
    assert!(versions.status.success());
    assert!(String::from_utf8_lossy(&versions.stdout).contains("1.2.3"));
    let absent = run(
        root,
        root,
        &["repo", "show", "absent", "--repository", "local-fixture"],
    );
    assert!(!absent.status.success());

    let invalid = run(
        root,
        root,
        &[
            "repo",
            "skills",
            "--repository",
            "local-fixture",
            "--all-versions",
        ],
    );
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("HTTP registry index"));

    let tested = run(root, root, &["repo", "test", "local-fixture"]);
    assert!(tested.status.success());
    let text = String::from_utf8_lossy(&tested.stdout);
    assert!(text.contains("Connectivity:"));
    assert!(text.contains("Catalog:"));
    assert!(text.contains("Acquisition:"));

    let missing_catalog = root.join("missing-catalog").to_string_lossy().to_string();
    let broken = run(
        root,
        root,
        &[
            "repo",
            "add",
            "broken-fixture",
            &missing_catalog,
            "--repo-type",
            "local",
        ],
    );
    assert!(broken.status.success());
    let partial = run(root, root, &["skill", "search", "catalog-demo", "--json"]);
    assert!(!partial.status.success());
    let value: serde_json::Value = serde_json::from_slice(&partial.stdout).unwrap();
    assert_eq!(value["outcome"], "partial");
    assert_eq!(value["results"][0]["id"], "catalog-demo");
    assert_eq!(value["failures"][0]["repository"], "broken-fixture");
}

#[test]
fn list_check_reconciles_relative_intent_and_permits_extraneous_content() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(
        root,
        "demo = { origin = { type = \"local\", path = \"source\" } }",
    );
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: demo\nversion: \"1.0.0\"\ndescription: fixture\n---\n# Demo\n",
    )
    .unwrap();
    let installed = run(root, root, &["project", "install"]);
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let reconciled = run(root, root, &["skill", "list", "--check", "--json"]);
    assert!(
        reconciled.status.success(),
        "{}{}",
        String::from_utf8_lossy(&reconciled.stdout),
        String::from_utf8_lossy(&reconciled.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&reconciled.stdout).unwrap();
    assert_eq!(rows[0]["reconciliation"], "ok");

    write_skill(root, "personal", "1.0.0");
    let with_personal = run(root, root, &["skill", "list", "--check", "--json"]);
    assert!(with_personal.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&with_personal.stdout).unwrap();
    assert!(rows.as_array().unwrap().iter().any(|row| {
        row["id"] == "personal"
            && row["reconciliation"] == "extraneous"
            && row["extraneous"] == true
    }));

    std::fs::write(
        root.join(".claude/skills/demo/SKILL.md"),
        "---\nname: demo\nversion: \"9.0.0\"\ndescription: edited\n---\n# Edited\n",
    )
    .unwrap();
    let drifted = run(root, root, &["skill", "list", "--check", "--json"]);
    assert!(!drifted.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&drifted.stdout).unwrap();
    let demo = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "demo")
        .unwrap();
    assert!(matches!(
        demo["reconciliation"].as_str(),
        Some("revision-mismatch" | "content-mismatch")
    ));
}

#[test]
fn repos_add_persists_git_tag_and_rejects_ambiguous_refs() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    write_project(root, "");

    let added = run(
        root,
        root,
        &[
            "repo",
            "add",
            "stable",
            "--repo-type",
            "git-marketplace",
            "https://github.com/example/skills.git",
            "--tag",
            "v1.2.0",
        ],
    );
    assert!(
        added.status.success(),
        "{}{}",
        String::from_utf8_lossy(&added.stdout),
        String::from_utf8_lossy(&added.stderr)
    );
    let manifest = std::fs::read_to_string(root.join("skill-project.toml")).unwrap();
    assert!(manifest.contains("tag = \"v1.2.0\""), "{manifest}");

    let ambiguous = run(
        root,
        root,
        &[
            "repo",
            "add",
            "ambiguous",
            "--repo-type",
            "git-marketplace",
            "https://github.com/example/skills.git",
            "--branch",
            "main",
            "--tag",
            "v1.2.0",
        ],
    );
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("--branch conflicts with --tag"));
    let manifest = std::fs::read_to_string(root.join("skill-project.toml")).unwrap();
    assert!(!manifest.contains("name = \"ambiguous\""), "{manifest}");
}

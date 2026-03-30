//! Tests for recursive (transitive) dependency resolution during install

use fastskill::core::dependency_resolver::{DependencyResolver, SkillInstallItem};
use fastskill::core::manifest::{SkillEntry, SkillSource};
use std::path::PathBuf;
use tempfile::TempDir;

// ── helpers ──────────────────────────────────────────────────────────────────

fn local_entry(id: &str) -> SkillEntry {
    SkillEntry {
        id: id.to_string(),
        source: SkillSource::Local {
            path: PathBuf::from(format!("local/{}", id)),
            editable: false,
        },
        version: None,
        groups: vec![],
        editable: false,
    }
}

fn write_manifest(dir: &std::path::Path, skill_id: &str, deps_toml: &str) {
    let skill_dir = dir.join(skill_id);
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("skill-project.toml"),
        format!("[dependencies]\n{}", deps_toml),
    )
    .unwrap();
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_single_skill_no_transitive() {
    let dir = TempDir::new().unwrap();
    let mut resolver = DependencyResolver::new(5);

    let result = resolver
        .resolve_dependencies(vec![local_entry("standalone")], dir.path())
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.id, "standalone");
    assert_eq!(result[0].depth, 0);
    assert!(result[0].parent_skill.is_none());
}

#[tokio::test]
async fn test_transitive_chain_a_to_b_to_c() {
    // project → skill-a (depth 0) → skill-b (depth 1) → skill-c (depth 2)
    let dir = TempDir::new().unwrap();

    write_manifest(
        dir.path(),
        "skill-a",
        r#"skill-b = { source = "local", path = "local/skill-b" }"#,
    );
    write_manifest(
        dir.path(),
        "skill-b",
        r#"skill-c = { source = "local", path = "local/skill-c" }"#,
    );
    // skill-c has no manifest → leaf

    let mut resolver = DependencyResolver::new(5);
    let result = resolver
        .resolve_dependencies(vec![local_entry("skill-a")], dir.path())
        .await
        .unwrap();

    let ids: Vec<&str> = result.iter().map(|i| i.entry.id.as_str()).collect();
    assert!(ids.contains(&"skill-a"), "skill-a must be present");
    assert!(ids.contains(&"skill-b"), "skill-b must be present");
    assert!(ids.contains(&"skill-c"), "skill-c must be present");

    // Topological ordering: skill-a before skill-b before skill-c
    let pos_a = ids.iter().position(|&x| x == "skill-a").unwrap();
    let pos_b = ids.iter().position(|&x| x == "skill-b").unwrap();
    let pos_c = ids.iter().position(|&x| x == "skill-c").unwrap();
    assert!(pos_a < pos_b, "skill-a must precede skill-b");
    assert!(pos_b < pos_c, "skill-b must precede skill-c");
}

#[tokio::test]
async fn test_deduplication_same_transitive_dep() {
    // skill-a and skill-b both depend on shared-lib
    let dir = TempDir::new().unwrap();

    write_manifest(
        dir.path(),
        "skill-a",
        r#"shared-lib = { source = "local", path = "local/shared-lib" }"#,
    );
    write_manifest(
        dir.path(),
        "skill-b",
        r#"shared-lib = { source = "local", path = "local/shared-lib" }"#,
    );

    let mut resolver = DependencyResolver::new(5);
    let result = resolver
        .resolve_dependencies(
            vec![local_entry("skill-a"), local_entry("skill-b")],
            dir.path(),
        )
        .await
        .unwrap();

    // shared-lib must appear exactly once
    let shared_lib_count = result.iter().filter(|i| i.entry.id == "shared-lib").count();
    assert_eq!(shared_lib_count, 1, "shared-lib must appear exactly once");
}

#[tokio::test]
async fn test_depth_limit_enforced() {
    // Chain: a → b → c → d, but max_depth = 2 → c is at depth 2, d should NOT appear
    let dir = TempDir::new().unwrap();

    write_manifest(
        dir.path(),
        "a",
        r#"b = { source = "local", path = "local/b" }"#,
    );
    write_manifest(
        dir.path(),
        "b",
        r#"c = { source = "local", path = "local/c" }"#,
    );
    write_manifest(
        dir.path(),
        "c",
        r#"d = { source = "local", path = "local/d" }"#,
    );

    let mut resolver = DependencyResolver::new(2);
    let result = resolver
        .resolve_dependencies(vec![local_entry("a")], dir.path())
        .await
        .unwrap();

    let ids: Vec<&str> = result.iter().map(|i| i.entry.id.as_str()).collect();
    assert!(ids.contains(&"a"), "a must be present");
    assert!(ids.contains(&"b"), "b must be present at depth 1");
    assert!(ids.contains(&"c"), "c must be present at depth 2");
    assert!(
        !ids.contains(&"d"),
        "d must be excluded (depth 3 > limit 2)"
    );
}

#[tokio::test]
async fn test_missing_manifest_is_graceful() {
    // Skill with no skill-project.toml → resolver continues without error
    let dir = TempDir::new().unwrap();
    let mut resolver = DependencyResolver::new(5);

    let result = resolver
        .resolve_dependencies(vec![local_entry("no-manifest")], dir.path())
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].entry.id, "no-manifest");
}

#[tokio::test]
async fn test_empty_initial_list() {
    let dir = TempDir::new().unwrap();
    let mut resolver = DependencyResolver::new(5);

    let result = resolver
        .resolve_dependencies(vec![], dir.path())
        .await
        .unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn test_depth_tracking() {
    let dir = TempDir::new().unwrap();

    write_manifest(
        dir.path(),
        "root",
        r#"child = { source = "local", path = "local/child" }"#,
    );

    let mut resolver = DependencyResolver::new(5);
    let result = resolver
        .resolve_dependencies(vec![local_entry("root")], dir.path())
        .await
        .unwrap();

    let root_item = result.iter().find(|i| i.entry.id == "root").unwrap();
    let child_item = result.iter().find(|i| i.entry.id == "child").unwrap();

    assert_eq!(root_item.depth, 0);
    assert_eq!(child_item.depth, 1);
    assert_eq!(child_item.parent_skill.as_deref(), Some("root"));
}

#[tokio::test]
async fn test_topological_sort_is_stable() {
    // topological_sort must return the same order as input (no-op for BFS output)
    let resolver = DependencyResolver::new(5);

    let items: Vec<SkillInstallItem> = vec!["x", "y", "z"]
        .into_iter()
        .enumerate()
        .map(|(i, id)| SkillInstallItem {
            entry: local_entry(id),
            depth: i as u32,
            parent_skill: None,
        })
        .collect();

    let sorted = resolver.topological_sort(items.clone());
    for (orig, sort) in items.iter().zip(sorted.iter()) {
        assert_eq!(orig.entry.id, sort.entry.id);
    }
}

#[tokio::test]
async fn test_install_depth_default_from_config() {
    // Verify that FastSkillToolConfig defaults install_depth to 5
    use fastskill::core::manifest::{FastSkillToolConfig, ToolSection};

    let toml_str = r#"
[tool.fastskill]
skills_directory = ".claude/skills"
"#;

    let parsed: fastskill::core::manifest::SkillProjectToml = toml::from_str(toml_str).unwrap();

    let depth = parsed
        .tool
        .as_ref()
        .and_then(|t| t.fastskill.as_ref())
        .map(|cfg| cfg.install_depth)
        .unwrap_or(0);

    assert_eq!(depth, 5, "default install_depth must be 5");
}

#[tokio::test]
async fn test_skip_transitive_default_is_false() {
    let toml_str = r#"
[tool.fastskill]
skills_directory = ".claude/skills"
"#;

    let parsed: fastskill::core::manifest::SkillProjectToml = toml::from_str(toml_str).unwrap();

    let skip = parsed
        .tool
        .as_ref()
        .and_then(|t| t.fastskill.as_ref())
        .map(|cfg| cfg.skip_transitive)
        .unwrap_or(true); // wrong default to force the assertion

    assert!(!skip, "skip_transitive must default to false");
}

#[tokio::test]
async fn test_custom_depth_from_config() {
    let toml_str = r#"
[tool.fastskill]
skills_directory = ".claude/skills"
install_depth = 3
"#;

    let parsed: fastskill::core::manifest::SkillProjectToml = toml::from_str(toml_str).unwrap();

    let depth = parsed
        .tool
        .and_then(|t| t.fastskill)
        .map(|cfg| cfg.install_depth)
        .unwrap_or(0);

    assert_eq!(depth, 3);
}

#[tokio::test]
async fn test_skip_transitive_can_be_enabled() {
    let toml_str = r#"
[tool.fastskill]
skills_directory = ".claude/skills"
skip_transitive = true
"#;

    let parsed: fastskill::core::manifest::SkillProjectToml = toml::from_str(toml_str).unwrap();

    let skip = parsed
        .tool
        .and_then(|t| t.fastskill)
        .map(|cfg| cfg.skip_transitive)
        .unwrap_or(false);

    assert!(skip);
}

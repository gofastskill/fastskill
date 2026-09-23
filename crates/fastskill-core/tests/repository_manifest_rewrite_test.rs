//! `repos add` / `repos remove` rewrite skill-project.toml. The rewrite must
//! only touch the repositories it owns, not tables the typed model lacks.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use fastskill_core::core::repository::{
    RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use tempfile::TempDir;

const MANIFEST: &str = r#"[dependencies]
alpha = "1.0.0"

[bundles.team]
version = "1.0.0"
artifact = "team.zip"

[overrides.alpha]
path = "vendor/alpha"

[extension]
value = "keep"

[tool.other]
flag = true

[tool.fastskill]
skills_directory = "skills"

[[tool.fastskill.repositories]]
name = "existing"
type = "local"
priority = 0
path = "./existing"
"#;

fn local_repo(name: &str) -> RepositoryDefinition {
    RepositoryDefinition {
        name: name.to_string(),
        repo_type: RepositoryType::Local,
        priority: 1,
        config: RepositoryConfig::Local {
            path: std::path::PathBuf::from("./added"),
        },
        auth: None,
        storage: None,
    }
}

fn assert_unowned_tables_kept(path: &std::path::Path) {
    let written: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    assert_eq!(
        written["bundles"]["team"]["artifact"].as_str(),
        Some("team.zip"),
        "[bundles] lost:\n{written:#?}"
    );
    assert_eq!(
        written["overrides"]["alpha"]["path"].as_str(),
        Some("vendor/alpha"),
        "[overrides] lost"
    );
    assert_eq!(written["extension"]["value"].as_str(), Some("keep"));
    assert_eq!(written["tool"]["other"]["flag"].as_bool(), Some(true));
    assert_eq!(written["dependencies"]["alpha"].as_str(), Some("1.0.0"));
}

fn repository_names(path: &std::path::Path) -> Vec<String> {
    let written: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    written["tool"]["fastskill"]["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|repo| repo["name"].as_str().unwrap().to_string())
        .collect()
}

/// The CLI builds the manager from the Manifest's repositories
/// (`from_definitions`), mutates it, then calls `save()`; mirror that state.
#[test]
fn repos_add_and_remove_keep_tables_they_do_not_own() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("skill-project.toml");
    std::fs::write(&path, MANIFEST).unwrap();

    let mut manager = RepositoryManager::new(path.clone());
    let mut existing = local_repo("existing");
    existing.priority = 0;
    manager
        .add_repository("existing".to_string(), existing)
        .unwrap();
    manager
        .add_repository("added".to_string(), local_repo("added"))
        .unwrap();
    manager.save().unwrap();
    assert_unowned_tables_kept(&path);
    let mut names = repository_names(&path);
    names.sort();
    assert_eq!(names, ["added", "existing"]);

    manager.remove_repository("existing").unwrap();
    manager.save().unwrap();
    assert_unowned_tables_kept(&path);
    assert_eq!(repository_names(&path), ["added"]);
}

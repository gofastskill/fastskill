#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::*;
use crate::ServiceConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

fn write_skill(path: &Path, id: &str, dependencies: &[(&str, &Path)]) -> PathBuf {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {id}\nversion: \"1.0.0\"\ndescription: fixture\n---\nBody\n"),
    )
    .unwrap();
    if !dependencies.is_empty() {
        let declarations = dependencies
            .iter()
            .map(|(dependency, source)| {
                format!(
                    "{dependency} = {{ origin = {{ type = \"local\", path = {:?} }} }}",
                    source
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            path.join("skill-project.toml"),
            format!("[dependencies]\n{declarations}\n"),
        )
        .unwrap();
    }
    path.to_path_buf()
}

fn write_versioned_skill(path: &Path, id: &str, version: &str) -> PathBuf {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {id}\nversion: \"{version}\"\ndescription: fixture\n---\nBody\n"),
    )
    .unwrap();
    path.to_path_buf()
}

async fn service(root: &Path) -> FastSkillService {
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: root.join("installed"),
        skill_cache_root: Some(root.join("cache")),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    service
}

fn root(path: PathBuf, id: &str) -> ResolutionRoot {
    ResolutionRoot {
        origin: Origin::Local {
            path,
            editable: false,
        },
        expected_id: Some(id.to_string()),
        groups: Vec::new(),
        locked: None,
    }
}

#[test]
fn compatible_repository_requirements_merge_in_any_order() {
    let requirement = |raw: &str| Origin::Repository {
        repo: "team".to_string(),
        skill: "shared".to_string(),
        version: Some(VersionConstraint::parse(raw).unwrap()),
    };
    let left =
        merge_requirement_origins("shared", &[requirement(">=1.0.0"), requirement("<2.0.0")])
            .unwrap();
    let right =
        merge_requirement_origins("shared", &[requirement("<2.0.0"), requirement(">=1.0.0")])
            .unwrap();
    assert_eq!(left, right);
    assert!(origins_accept_same_resolution(&left, &right, "1.5.0"));
}

#[test]
fn incompatible_and_missing_requirements_are_rejected() {
    let local = Origin::Local {
        path: "one".into(),
        editable: false,
    };
    let other = Origin::Local {
        path: "two".into(),
        editable: false,
    };
    assert!(merge_requirement_origins("shared", &[]).is_err());
    assert!(merge_requirement_origins("shared", &[local.clone(), other]).is_err());
    assert_eq!(
        merge_requirement_origins("shared", std::slice::from_ref(&local)).unwrap(),
        local
    );
    assert!(!origins_accept_same_resolution(
        &Origin::Local {
            path: "one".into(),
            editable: false
        },
        &Origin::Local {
            path: "two".into(),
            editable: false
        },
        "1.0.0"
    ));
}

#[test]
fn local_origin_compatibility_uses_filesystem_identity_across_path_forms() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("shared");
    let alias_parent = temp.path().join("alias");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&alias_parent).unwrap();
    let canonical = source.canonicalize().unwrap();
    let requested = Origin::Local {
        path: alias_parent.join("..").join("shared"),
        editable: false,
    };
    let recorded = Origin::Local {
        path: canonical.clone(),
        editable: false,
    };

    assert!(origins_accept_same_resolution(
        &requested, &recorded, "1.0.0"
    ));
    assert_eq!(
        merge_requirement_origins("shared", &[requested.clone(), recorded.clone()]).unwrap(),
        requested
    );
    assert!(!origins_accept_same_resolution(
        &requested,
        &Origin::Local {
            path: canonical,
            editable: true,
        },
        "1.0.0"
    ));
}

#[test]
fn locked_integrity_and_constraint_error_paths_are_explicit() {
    let editable = Origin::Local {
        path: "editable".into(),
        editable: true,
    };
    let empty = Resolved {
        version: "1.0.0".to_string(),
        commit_hash: None,
        checksum: None,
    };
    validate_locked_resolution(&editable, &empty, "editable").unwrap();

    let immutable = Origin::Local {
        path: "immutable".into(),
        editable: false,
    };
    assert!(validate_locked_resolution(&immutable, &empty, "immutable")
        .unwrap_err()
        .to_string()
        .contains("no content digest"));

    let git = Origin::Git {
        url: "https://example.test/repo.git".to_string(),
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };
    let missing_commit = Resolved {
        checksum: Some("digest".to_string()),
        ..empty.clone()
    };
    assert!(validate_locked_resolution(&git, &missing_commit, "git")
        .unwrap_err()
        .to_string()
        .contains("no commit"));

    let constrained = Origin::Repository {
        repo: "team".to_string(),
        skill: "shared".to_string(),
        version: Some(VersionConstraint::parse(">=1.0.0").unwrap()),
    };
    assert!(
        ensure_origin_accepts_version("shared", &constrained, "not-semver")
            .unwrap_err()
            .to_string()
            .contains("invalid")
    );
}

#[test]
fn repository_requirement_merging_covers_unconstrained_and_wrong_repo_cases() {
    let unconstrained = |repo: &str| Origin::Repository {
        repo: repo.to_string(),
        skill: "shared".to_string(),
        version: None,
    };
    assert_eq!(
        merge_requirement_origins(
            "shared",
            &[
                unconstrained("team"),
                Origin::Repository {
                    repo: "team".to_string(),
                    skill: "shared".to_string(),
                    version: Some(VersionConstraint::parse("*").unwrap()),
                },
            ],
        )
        .unwrap(),
        unconstrained("team")
    );
    assert!(
        merge_requirement_origins("shared", &[unconstrained("team"), unconstrained("other")],)
            .unwrap_err()
            .to_string()
            .contains("incompatible repositories")
    );
}

#[test]
fn locked_remote_origins_are_pinned_without_changing_recorded_intent() {
    let git = Origin::Git {
        url: "https://example.test/repo.git".to_string(),
        r#ref: GitRef::Branch("main".to_string()),
        subdir: Some("skill".into()),
    };
    let resolved = Resolved {
        version: "1.2.3".to_string(),
        commit_hash: Some("0123456789abcdef".to_string()),
        checksum: Some("digest".to_string()),
    };
    assert!(matches!(
        pinned_origin(&git, &resolved, "demo").unwrap(),
        Origin::Git {
            r#ref: GitRef::Commit(commit),
            ..
        } if commit == "0123456789abcdef"
    ));
    let repository = Origin::Repository {
        repo: "team".to_string(),
        skill: "demo".to_string(),
        version: Some(VersionConstraint::parse(">=1").unwrap()),
    };
    assert!(matches!(
        pinned_origin(&repository, &resolved, "demo").unwrap(),
        Origin::Repository { version: Some(version), .. }
            if version.as_exact().is_some_and(|version| version == "1.2.3")
    ));
    let missing_commit = Resolved {
        commit_hash: None,
        ..resolved
    };
    assert!(pinned_origin(&git, &missing_commit, "demo").is_err());
}

#[tokio::test]
async fn prepares_complete_closure_with_multiple_parents() {
    let temp = TempDir::new().unwrap();
    let shared = write_skill(&temp.path().join("shared"), "shared", &[]);
    let left = write_skill(&temp.path().join("left"), "left", &[("shared", &shared)]);
    let right = write_skill(&temp.path().join("right"), "right", &[("shared", &shared)]);
    let source = write_skill(
        &temp.path().join("root"),
        "root",
        &[("left", &left), ("right", &right)],
    );
    let service = service(temp.path()).await;

    let plan = prepare_resolution(
        &service,
        vec![root(source, "root")],
        &HashMap::new(),
        5,
        false,
    )
    .await
    .unwrap();

    let ids: BTreeSet<_> = plan
        .candidates
        .iter()
        .map(|candidate| candidate.prepared.id())
        .collect();
    assert_eq!(ids, BTreeSet::from(["left", "right", "root", "shared"]));
    let shared = plan
        .candidates
        .iter()
        .find(|candidate| candidate.prepared.id() == "shared")
        .unwrap();
    assert_eq!(
        shared.required_by,
        BTreeSet::from(["left".into(), "right".into()])
    );
    assert_eq!(shared.depth, 2);
}

#[tokio::test]
async fn merges_a_later_deeper_parent_constraint_into_recorded_origin() {
    use crate::core::repository::{
        RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
    };

    let temp = TempDir::new().unwrap();
    let repository = temp.path().join("repository");
    write_versioned_skill(&repository.join("shared-v1"), "shared", "1.5.0");
    write_versioned_skill(&repository.join("shared-v2"), "shared", "2.0.0");
    let alpha = write_skill(&temp.path().join("alpha"), "alpha", &[]);
    std::fs::write(
            alpha.join("skill-project.toml"),
            "[dependencies]\nshared = { origin = { type = \"repository\", repo = \"team\", skill = \"shared\", version = \">=1.0.0\" } }\n",
        )
        .unwrap();
    let child = write_skill(&temp.path().join("child"), "child", &[]);
    std::fs::write(
            child.join("skill-project.toml"),
            "[dependencies]\nshared = { origin = { type = \"repository\", repo = \"team\", skill = \"shared\", version = \"<2.0.0\" } }\n",
        )
        .unwrap();
    let beta = write_skill(&temp.path().join("beta"), "beta", &[("child", &child)]);
    let mut service = service(temp.path()).await;
    service = service.with_repository_manager(Arc::new(RepositoryManager::from_definitions(vec![
        RepositoryDefinition {
            name: "team".to_string(),
            repo_type: RepositoryType::Local,
            priority: 0,
            config: RepositoryConfig::Local { path: repository },
            auth: None,
            storage: None,
        },
    ])));

    let plan = prepare_resolution(
        &service,
        vec![root(alpha.clone(), "alpha"), root(beta.clone(), "beta")],
        &HashMap::new(),
        5,
        false,
    )
    .await
    .unwrap();
    let shared = plan
        .candidates
        .iter()
        .find(|candidate| candidate.prepared.id() == "shared")
        .unwrap();
    assert!(matches!(
        &shared.origin,
        Origin::Repository { version: Some(version), .. }
            if version.satisfies("1.5.0").unwrap()
                && !version.satisfies("2.0.0").unwrap()
    ));
    assert_eq!(shared.prepared.resolved().version, "1.5.0");
    assert_eq!(
        shared.required_by,
        BTreeSet::from(["alpha".into(), "child".into()])
    );

    std::fs::write(
            child.join("skill-project.toml"),
            "[dependencies]\nshared = { origin = { type = \"repository\", repo = \"team\", skill = \"shared\", version = \"<1.0.0\" } }\n",
        )
        .unwrap();
    let incompatible = prepare_resolution(
        &service,
        vec![root(alpha, "alpha"), root(beta, "beta")],
        &HashMap::new(),
        5,
        false,
    )
    .await
    .unwrap_err();
    assert!(incompatible.to_string().contains("satisf"));
}

#[test]
fn locked_child_must_satisfy_the_parents_declared_constraint() {
    let requested = Origin::Repository {
        repo: "team".to_string(),
        skill: "shared".to_string(),
        version: Some(VersionConstraint::parse("<2.0.0").unwrap()),
    };
    let locked = ResolutionLockEntry {
        origin: Origin::Repository {
            repo: "team".to_string(),
            skill: "shared".to_string(),
            version: Some(VersionConstraint::parse(">=2.0.0").unwrap()),
        },
        resolved: Resolved {
            version: "2.1.0".to_string(),
            commit_hash: None,
            checksum: Some("digest".to_string()),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
    };

    let error = validate_locked_requirement("shared", &requested, &locked).unwrap_err();
    assert!(error.to_string().contains("does not satisfy"));
}

#[tokio::test]
async fn strict_closure_rejects_a_locked_child_outside_parent_constraint_before_fetch() {
    let temp = TempDir::new().unwrap();
    let source = write_skill(&temp.path().join("root"), "root", &[]);
    std::fs::write(
            source.join("skill-project.toml"),
            "[dependencies]\nshared = { origin = { type = \"repository\", repo = \"team\", skill = \"shared\", version = \"<2.0.0\" } }\n",
        )
        .unwrap();
    let service = service(temp.path()).await;
    let root_resolved = Resolved {
        version: "1.0.0".to_string(),
        commit_hash: None,
        checksum: Some(crate::core::install::content_digest(&source).unwrap()),
    };
    let root_origin = Origin::Local {
        path: source,
        editable: false,
    };
    let mut locked = HashMap::new();
    locked.insert(
        "root".to_string(),
        ResolutionLockEntry {
            origin: root_origin.clone(),
            resolved: root_resolved.clone(),
            dependencies: vec!["shared".to_string()],
            groups: Vec::new(),
        },
    );
    locked.insert(
        "shared".to_string(),
        ResolutionLockEntry {
            origin: Origin::Repository {
                repo: "team".to_string(),
                skill: "shared".to_string(),
                version: Some(VersionConstraint::parse(">=2.0.0").unwrap()),
            },
            resolved: Resolved {
                version: "2.1.0".to_string(),
                commit_hash: None,
                checksum: Some("digest".to_string()),
            },
            dependencies: Vec::new(),
            groups: Vec::new(),
        },
    );
    let error = prepare_locked_resolution_entries(
        &service,
        vec![ResolutionRoot {
            origin: root_origin,
            expected_id: Some("root".to_string()),
            groups: Vec::new(),
            locked: Some(root_resolved),
        }],
        &locked,
        5,
        false,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("does not satisfy"));
}

#[tokio::test]
async fn rejects_cycle_duplicate_root_and_truncated_depth() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    write_skill(&first, "first", &[("second", &second)]);
    write_skill(&second, "second", &[("first", &first)]);
    let service = service(temp.path()).await;

    let cycle = prepare_resolution(
        &service,
        vec![root(first.clone(), "first")],
        &HashMap::new(),
        5,
        false,
    )
    .await
    .unwrap_err();
    assert!(cycle.to_string().contains("first -> second -> first"));

    let duplicate = prepare_resolution(
        &service,
        vec![root(first.clone(), "first"), root(first.clone(), "first")],
        &HashMap::new(),
        5,
        false,
    )
    .await
    .unwrap_err();
    assert!(duplicate.to_string().contains("selected more than once"));

    let depth = prepare_resolution(
        &service,
        vec![root(first, "first")],
        &HashMap::new(),
        1,
        false,
    )
    .await
    .unwrap_err();
    assert!(depth.to_string().contains("depth limit 1"));
    assert!(
        prepare_resolution(&service, Vec::new(), &HashMap::new(), 0, false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn locked_restore_detects_source_drift_and_missing_facts() {
    let temp = TempDir::new().unwrap();
    let source = write_skill(&temp.path().join("root"), "root", &[]);
    let service = service(temp.path()).await;
    let initial = prepare_resolution(
        &service,
        vec![root(source.clone(), "root")],
        &HashMap::new(),
        5,
        false,
    )
    .await
    .unwrap();
    let resolved = initial.candidates[0].prepared.resolved().clone();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: root\nversion: \"2.0.0\"\ndescription: changed\n---\nBody\n",
    )
    .unwrap();
    let mut locked = HashMap::new();
    locked.insert("root".to_string(), resolved.clone());
    let mut selected = root(source.clone(), "root");
    selected.locked = Some(resolved);
    let error = prepare_locked_resolution(&service, vec![selected], &locked, 5, false)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("locked version") || error.to_string().contains("checksum"));

    let missing = prepare_locked_resolution(
        &service,
        vec![root(source, "root")],
        &HashMap::new(),
        5,
        false,
    )
    .await
    .unwrap_err();
    assert!(missing.to_string().contains("missing immutable facts"));
}

#[tokio::test]
async fn preview_uses_disposable_catalog_and_content_cache() {
    use crate::core::repository::{
        RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
    };

    let temp = TempDir::new().unwrap();
    let repository = temp.path().join("repository");
    write_skill(&repository.join("demo"), "demo", &[]);
    let mut service = service(temp.path()).await;
    service = service.with_repository_manager(Arc::new(RepositoryManager::from_definitions(vec![
        RepositoryDefinition {
            name: "team".to_string(),
            repo_type: RepositoryType::Local,
            priority: 0,
            config: RepositoryConfig::Local { path: repository },
            auth: None,
            storage: None,
        },
    ])));
    let cache = service.config().skill_cache_root.as_ref().unwrap();
    let entries = |path: &Path| {
        std::fs::read_dir(path)
            .ok()
            .into_iter()
            .flatten()
            .map(|entry| entry.unwrap().file_name())
            .collect::<BTreeSet<_>>()
    };
    let before = entries(cache);
    let selected = ResolutionRoot {
        origin: Origin::Repository {
            repo: "team".to_string(),
            skill: "demo".to_string(),
            version: None,
        },
        expected_id: Some("demo".to_string()),
        groups: Vec::new(),
        locked: None,
    };

    let plan = prepare_resolution_preview(&service, vec![selected], &HashMap::new(), 5)
        .await
        .unwrap();

    assert_eq!(plan.root_ids, vec!["demo"]);
    assert_eq!(plan.refreshed_repositories, vec!["team"]);
    let after = entries(cache);
    assert_eq!(after, before);
}

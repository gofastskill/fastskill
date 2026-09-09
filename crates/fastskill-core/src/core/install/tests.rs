use super::*;
use crate::{FastSkillService, ServiceConfig};
use tempfile::TempDir as TestTempDir;

const VALID_SKILL_MD: &str =
    "---\nname: test-skill\nversion: \"1.0.0\"\ndescription: A test skill\n---\nBody\n";

fn write_valid_skill(parent: &Path, dir_name: &str) -> PathBuf {
    let dir = parent.join(dir_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), VALID_SKILL_MD).unwrap();
    dir
}

/// Set up a project directory (skill-project.toml + skills dir) and chdir
/// into it, returning a guard that restores the cwd and holds the temp dir.
fn setup_project() -> (TestTempDir, crate::test_utils::DirGuard, PathBuf) {
    let tmp = TestTempDir::new().unwrap();
    let original_dir = std::env::current_dir().ok();
    std::env::set_current_dir(tmp.path()).unwrap();
    std::fs::write(
        tmp.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let skills_dir = tmp.path().join(".claude/skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    (tmp, crate::test_utils::DirGuard(original_dir), skills_dir)
}

async fn make_service(storage: &Path) -> FastSkillService {
    let config = ServiceConfig {
        skill_storage_path: storage.to_path_buf(),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();
    service
}

// ── add_from_origin: Local, end-to-end ────────────────────────────────────

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_add_from_origin_local_end_to_end() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let service = make_service(&skills_dir).await;

    let origin = Origin::Local {
        path: src.clone(),
        editable: false,
    };
    let outcome = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("add should succeed");

    assert_eq!(outcome.id, "test-skill");
    assert_eq!(outcome.resolved.version, "1.0.0");
    assert!(
        outcome.resolved.checksum.is_some(),
        "immutable installs must record a canonical content digest"
    );
    assert!(skills_dir.join("test-skill/SKILL.md").exists());

    // Manifest + lock were written.
    let project = SkillProjectToml::load_from_file(&tmp.path().join("skill-project.toml"))
        .expect("manifest should load");
    assert!(project
        .dependencies
        .expect("deps section")
        .dependencies
        .contains_key("test-skill"));
    let lock = ProjectSkillsLock::load_from_file(&tmp.path().join("skills.lock"))
        .expect("lock should load");
    assert_eq!(lock.skills.len(), 1);
    assert_eq!(lock.skills[0].id, "test-skill");

    // Registered with the skill manager.
    let id = SkillId::new("test-skill".to_string()).unwrap();
    assert!(service
        .skill_manager()
        .get_skill(&id)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn prepare_rejects_wrong_identity_before_replacing_files() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let existing = skills_dir.join("expected-skill");
    std::fs::create_dir_all(&existing).unwrap();
    std::fs::write(existing.join("marker"), "working").unwrap();
    let service = make_service(&skills_dir).await;

    let error = service
        .prepare_install(
            Origin::Local {
                path: src,
                editable: false,
            },
            "expected-skill",
            None,
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("expected-skill"));
    assert_eq!(
        std::fs::read_to_string(existing.join("marker")).unwrap(),
        "working"
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn prepared_skill_exposes_dependencies_before_apply() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    std::fs::write(
        src.join("skill-project.toml"),
        "[dependencies]\nchild = { origin = { type = \"local\", path = \"child\" } }\n",
    )
    .unwrap();
    let service = make_service(&skills_dir).await;

    let prepared = service
        .prepare_install(
            Origin::Local {
                path: src,
                editable: false,
            },
            "test-skill",
            None,
        )
        .await
        .unwrap();

    assert_eq!(prepared.dependencies().len(), 1);
    assert_eq!(prepared.dependencies()[0].id, "child");
    assert!(!skills_dir.join("test-skill").exists());
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn strict_prepare_rejects_changed_content_before_apply() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let service = make_service(&skills_dir).await;
    let origin = Origin::Local {
        path: src.clone(),
        editable: false,
    };
    let added = service
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        VALID_SKILL_MD.replace("Body", "changed"),
    )
    .unwrap();

    let error = service
        .prepare_install(origin, "test-skill", Some(&added.resolved))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("checksum"));
    let installed = std::fs::read_to_string(skills_dir.join("test-skill/SKILL.md")).unwrap();
    assert_eq!(installed, VALID_SKILL_MD);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_add_from_origin_records_and_preserves_groups() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let service = make_service(&skills_dir).await;
    let origin = Origin::Local {
        path: src.clone(),
        editable: false,
    };

    // Fresh add with an explicit group records it on manifest + lock.
    service
        .add_from_origin(origin.clone(), AddMode::Fresh, vec!["dev".to_string()])
        .await
        .expect("fresh add should succeed");

    let groups_in_manifest = || {
        let project =
            SkillProjectToml::load_from_file(&tmp.path().join("skill-project.toml")).unwrap();
        match project
            .dependencies
            .unwrap()
            .dependencies
            .remove("test-skill")
        {
            Some(DependencySpec::Inline { groups, .. }) => groups,
            _ => None,
        }
    };
    let lock_groups = || {
        let lock = ProjectSkillsLock::load_from_file(&tmp.path().join("skills.lock")).unwrap();
        lock.skills[0].groups.clone()
    };
    assert_eq!(groups_in_manifest(), Some(vec!["dev".to_string()]));
    assert_eq!(lock_groups(), vec!["dev".to_string()]);

    // Update with an empty groups list must PRESERVE the existing group.
    service
        .add_from_origin(origin, AddMode::Update, vec![])
        .await
        .expect("update should succeed");
    assert_eq!(
        groups_in_manifest(),
        Some(vec!["dev".to_string()]),
        "update with empty groups must preserve existing groups"
    );
    assert_eq!(lock_groups(), vec!["dev".to_string()]);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_add_from_origin_fresh_conflict() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let service = make_service(&skills_dir).await;

    let origin = Origin::Local {
        path: src.clone(),
        editable: false,
    };
    service
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("first add should succeed");

    let result = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await;
    assert!(matches!(result, Err(ServiceError::AlreadyIndexed(_))));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_add_from_origin_update_overwrites() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let service = make_service(&skills_dir).await;

    let origin = Origin::Local {
        path: src.clone(),
        editable: false,
    };
    service
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("first add should succeed");

    // Update the source content, then re-add via Update mode.
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: test-skill\nversion: \"2.0.0\"\ndescription: updated\n---\nBody\n",
    )
    .unwrap();

    let outcome = service
        .add_from_origin(origin, AddMode::Update, vec![])
        .await
        .expect("update should succeed");
    assert_eq!(outcome.resolved.version, "2.0.0");

    let id = SkillId::new("test-skill".to_string()).unwrap();
    let skill = service
        .skill_manager()
        .get_skill(&id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(skill.version, "2.0.0");
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_add_from_origin_local_editable_symlinks() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let service = make_service(&skills_dir).await;

    let origin = Origin::Local {
        path: src.clone(),
        editable: true,
    };
    let outcome = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("editable add should succeed");

    let storage_path = skills_dir.join(&outcome.id);
    assert!(storage_path.is_symlink(), "editable install must symlink");
}

#[tokio::test]
async fn test_add_from_origin_local_nonexistent_path() {
    let tmp = TestTempDir::new().unwrap();
    let storage = tmp.path().join("storage");
    let service = make_service(&storage).await;

    let origin = Origin::Local {
        path: tmp.path().join("does-not-exist"),
        editable: false,
    };
    let result = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await;
    assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
}

// ── add_from_origin: ZipUrl, end-to-end (mock HTTP) ───────────────────────

fn build_skill_zip() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = zip::ZipWriter::new(cursor);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("test-skill/SKILL.md", opts).unwrap();
        writer.write_all(VALID_SKILL_MD.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buf
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_add_from_origin_zip_url_end_to_end() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_tmp, _guard, skills_dir) = setup_project();
    let service = make_service(&skills_dir).await;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pkg.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(build_skill_zip()))
        .mount(&server)
        .await;

    let origin = Origin::ZipUrl {
        url: format!("{}/pkg.zip", server.uri()),
    };
    let outcome = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("zip-url add should succeed");
    assert_eq!(outcome.id, "test-skill");
    assert!(skills_dir.join("test-skill/SKILL.md").exists());
}

// ── add_from_origin: Repository without a repository manager ─────────────

#[tokio::test]
async fn test_add_from_origin_repository_requires_manager() {
    let tmp = TestTempDir::new().unwrap();
    let storage = tmp.path().join("storage");
    let service = make_service(&storage).await;

    let origin = Origin::Repository {
        repo: "default".to_string(),
        skill: "scope/skill".to_string(),
        version: None,
    };
    let result = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await;
    assert!(matches!(result, Err(ServiceError::Config(_))));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn local_repository_add_refreshes_catalog_and_acquires_exact_folder() {
    use crate::core::repository::{
        RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
    };
    use std::sync::Arc;

    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (project, _guard, skills_dir) = setup_project();
    let repository = project.path().join("repository");
    let source = repository.join("odd-folder");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: Display Name\nversion: 1.2.0\ndescription: local repository\nmetadata:\n  id: team/reviewer\n---\nBody\n",
    )
    .unwrap();
    let manager = RepositoryManager::from_definitions(vec![RepositoryDefinition {
        name: "team".to_string(),
        repo_type: RepositoryType::Local,
        priority: 0,
        config: RepositoryConfig::Local { path: repository },
        auth: None,
        storage: None,
    }]);
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        skill_cache_root: Some(project.path().join("cache")),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let service = service.with_repository_manager(Arc::new(manager));
    assert_eq!(
        service.refresh_repository_metadata("team").await.unwrap(),
        "team"
    );
    let outcome = service
        .add_from_origin(
            Origin::Repository {
                repo: "team".to_string(),
                skill: "team/reviewer".to_string(),
                version: None,
            },
            AddMode::Fresh,
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(outcome.id, "team/reviewer");
    assert!(skills_dir.join("team/reviewer/SKILL.md").exists());
    assert!(outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("refreshed")));
}

// ── fetch_git: malformed immutable commit ids fail before network access ──

#[tokio::test]
async fn test_add_from_origin_rejects_malformed_git_commit() {
    let tmp = TestTempDir::new().unwrap();
    let storage = tmp.path().join("storage");
    let service = make_service(&storage).await;

    let origin = Origin::Git {
        url: "https://example.com/x.git".to_string(),
        r#ref: GitRef::Commit("deadbeef".to_string()),
        subdir: None,
    };
    let result = service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await;
    let error = result.unwrap_err();
    assert!(error.to_string().contains("40- or 64-character"));
}

// ── preflight ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_preflight_git_tag_is_immutable() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::Git {
        url: "u".to_string(),
        r#ref: GitRef::Tag("v1.0.0".to_string()),
        subdir: None,
    };
    assert!(matches!(
        service.preflight(&origin).await.unwrap(),
        UpdatePreflight::Immutable { .. }
    ));
}

#[tokio::test]
async fn test_preflight_git_commit_is_immutable() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::Git {
        url: "u".to_string(),
        r#ref: GitRef::Commit("abc123".to_string()),
        subdir: None,
    };
    assert!(matches!(
        service.preflight(&origin).await.unwrap(),
        UpdatePreflight::Immutable { .. }
    ));
}

#[tokio::test]
async fn test_preflight_git_branch_is_updatable() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::Git {
        url: "u".to_string(),
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };
    assert!(matches!(
        service.preflight(&origin).await.unwrap(),
        UpdatePreflight::Updatable
    ));
}

#[tokio::test]
async fn test_preflight_git_default_is_updatable() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::Git {
        url: "u".to_string(),
        r#ref: GitRef::Default,
        subdir: None,
    };
    assert!(matches!(
        service.preflight(&origin).await.unwrap(),
        UpdatePreflight::Updatable
    ));
}

#[tokio::test]
async fn test_preflight_local_editable_is_immutable() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::Local {
        path: tmp.path().to_path_buf(),
        editable: true,
    };
    assert!(matches!(
        service.preflight(&origin).await.unwrap(),
        UpdatePreflight::Immutable { .. }
    ));
}

#[tokio::test]
async fn test_preflight_local_copy_is_updatable() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::Local {
        path: tmp.path().to_path_buf(),
        editable: false,
    };
    assert!(matches!(
        service.preflight(&origin).await.unwrap(),
        UpdatePreflight::Updatable
    ));
}

#[tokio::test]
async fn test_preflight_zip_url_is_updatable() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::ZipUrl {
        url: "https://example.com/x.zip".to_string(),
    };
    assert!(matches!(
        service.preflight(&origin).await.unwrap(),
        UpdatePreflight::Updatable
    ));
}

#[tokio::test]
async fn test_preflight_repository_requires_manager() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let origin = Origin::Repository {
        repo: "default".to_string(),
        skill: "scope/skill".to_string(),
        version: None,
    };
    let result = service.preflight(&origin).await;
    assert!(matches!(result, Err(ServiceError::Config(_))));
}

#[tokio::test]
async fn offline_preparation_reports_each_missing_acquisition_fact() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let local = write_valid_skill(tmp.path(), "local-offline");
    let local_origin = Origin::Local {
        path: local.clone(),
        editable: false,
    };
    let local_expected = Resolved {
        version: "1.0.0".to_string(),
        commit_hash: None,
        checksum: Some(content_digest(&local).unwrap()),
    };
    let prepared = service
        .prepare_install_offline(local_origin, "test-skill", &local_expected)
        .await
        .unwrap();
    assert_eq!(prepared.id(), "test-skill");

    let unresolved = Resolved {
        version: "1.0.0".to_string(),
        commit_hash: None,
        checksum: Some("missing".to_string()),
    };

    let git = Origin::Git {
        url: "https://example.invalid/skill.git".to_string(),
        r#ref: GitRef::Branch("main".to_string()),
        subdir: None,
    };
    let error = service
        .prepare_install_offline(git.clone(), "test-skill", &unresolved)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("locked commit"));

    let pinned = Resolved {
        commit_hash: Some("a".repeat(40)),
        ..unresolved.clone()
    };
    let error = service
        .prepare_install_offline(git, "test-skill", &pinned)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("missing from the cache"));

    let repository = Origin::Repository {
        repo: "default".to_string(),
        skill: "test-skill".to_string(),
        version: None,
    };
    let error = service
        .prepare_install_offline(repository, "test-skill", &unresolved)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("No repositories configured"));

    let zip = Origin::ZipUrl {
        url: "https://example.invalid/skill.zip".to_string(),
    };
    let error = service
        .prepare_install_offline(zip, "test-skill", &unresolved)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no verified artifact"));

    let error = service
        .refresh_repository_metadata("default")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("No repositories configured"));
    let error = service
        .refresh_repository_requirement("default", "test-skill")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("No repositories configured"));
}

#[tokio::test]
async fn offline_preparation_restores_cached_repository_and_zip_artifacts() {
    use crate::core::cache::{SourceIndex, SourceIndexEntry, ZipValidator, ZipValidators};
    use crate::core::repository::{RepositoryConfig, RepositoryDefinition, RepositoryType};
    use chrono::Utc;
    use std::sync::Arc;

    let tmp = TestTempDir::new().unwrap();
    let storage = tmp.path().join("storage");
    let cache_root = tmp.path().join("cache");
    let config = ServiceConfig {
        skill_storage_path: storage,
        skill_cache_root: Some(cache_root),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();
    service = service.with_repository_manager(Arc::new(RepositoryManager::from_definitions(vec![
        RepositoryDefinition {
            name: "team".to_string(),
            repo_type: RepositoryType::HttpRegistry,
            priority: 0,
            config: RepositoryConfig::HttpRegistry {
                index_url: "http://127.0.0.1:1/index".to_string(),
            },
            auth: None,
            storage: None,
        },
    ])));

    let cached = write_valid_skill(tmp.path(), "cached-repository");
    let checksum = content_digest(&cached).unwrap();
    service
        .skill_cache()
        .put(
            &CacheIdentity::Registry {
                source: "team".to_string(),
                skill: "test-skill".to_string(),
                version: "1.0.0".to_string(),
            },
            &cached,
        )
        .unwrap();
    service
        .skill_cache()
        .write_source_index(
            "team",
            &SourceIndex {
                fetched_at: Utc::now(),
                entries: vec![SourceIndexEntry {
                    skill: "test-skill".to_string(),
                    versions: vec!["1.0.0".to_string()],
                    name: "test-skill".to_string(),
                    description: String::new(),
                }],
            },
        )
        .unwrap();
    let repository_origin = Origin::Repository {
        repo: "default".to_string(),
        skill: "test-skill".to_string(),
        version: None,
    };
    let expected = Resolved {
        version: "1.0.0".to_string(),
        commit_hash: None,
        checksum: Some(checksum.clone()),
    };
    let restored = service
        .prepare_install_offline(repository_origin.clone(), "test-skill", &expected)
        .await
        .unwrap();
    assert_eq!(
        restored.resolved().checksum.as_deref(),
        Some(checksum.as_str())
    );
    let fresh = service
        .prepare_add_offline(repository_origin, Some("test-skill"))
        .await
        .unwrap();
    assert_eq!(fresh.resolved().version, "1.0.0");

    let zip_url = "https://example.invalid/test-skill.zip";
    let zip_hash = "cached-zip-hash";
    service
        .skill_cache()
        .put(
            &CacheIdentity::ZipUrl {
                content_hash: zip_hash.to_string(),
            },
            &cached,
        )
        .unwrap();
    let mut validators = ZipValidators::default();
    validators.insert(
        zip_url,
        ZipValidator {
            etag: Some("fixture-etag".to_string()),
            last_modified: None,
            content_hash: zip_hash.to_string(),
            fetched_at: Utc::now(),
        },
    );
    service
        .skill_cache()
        .write_zip_validators(&validators)
        .unwrap();
    let restored_zip = service
        .prepare_install_offline(
            Origin::ZipUrl {
                url: zip_url.to_string(),
            },
            "test-skill",
            &expected,
        )
        .await
        .unwrap();
    assert_eq!(restored_zip.id(), "test-skill");
}

#[tokio::test]
async fn http_registry_requirement_refresh_persists_versions_and_reports_failures() {
    use crate::core::registry::client::IndexEntry;
    use crate::core::repository::{RepositoryConfig, RepositoryDefinition, RepositoryType};
    use std::sync::Arc;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let tmp = TestTempDir::new().unwrap();
    let server = MockServer::start().await;
    let entry = IndexEntry {
        name: "test-skill".to_string(),
        vers: "1.0.0".to_string(),
        deps: Vec::new(),
        cksum: "sha256:fixture".to_string(),
        features: std::collections::HashMap::new(),
        yanked: false,
        links: None,
        download_url: "https://example.invalid/test-skill.zip".to_string(),
        metadata: None,
    };
    Mock::given(method("GET"))
        .and(path("/index/test-skill"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(serde_json::to_string(&entry).unwrap()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/index/missing"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let manager = RepositoryManager::from_definitions(vec![RepositoryDefinition {
        name: "team".to_string(),
        repo_type: RepositoryType::HttpRegistry,
        priority: 0,
        config: RepositoryConfig::HttpRegistry {
            index_url: format!("{}/index", server.uri()),
        },
        auth: None,
        storage: None,
    }]);
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: tmp.path().join("storage"),
        skill_cache_root: Some(tmp.path().join("cache")),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    let service = service.with_repository_manager(Arc::new(manager));

    assert_eq!(
        service
            .refresh_repository_requirement("team", "test-skill")
            .await
            .unwrap(),
        "team"
    );
    let index = service
        .skill_cache()
        .read_source_index("team")
        .unwrap()
        .unwrap();
    assert_eq!(index.entries[0].versions, ["1.0.0"]);

    let constrained = Origin::Repository {
        repo: "team".to_string(),
        skill: "test-skill".to_string(),
        version: Some(VersionConstraint::parse(">=1.0.0").unwrap()),
    };
    assert!(matches!(
        service.preflight(&constrained).await.unwrap(),
        UpdatePreflight::Updatable
    ));
    let id = SkillId::new("test-skill".to_string()).unwrap();
    service
        .skill_manager()
        .force_register_skill(SkillDefinition::new(
            id,
            "test-skill".to_string(),
            "installed".to_string(),
            "1.0.0".to_string(),
            Origin::Local {
                path: tmp.path().join("installed"),
                editable: false,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        service.preflight(&constrained).await.unwrap(),
        UpdatePreflight::UpToDate
    ));
    let unavailable = Origin::Repository {
        repo: "team".to_string(),
        skill: "test-skill".to_string(),
        version: Some(VersionConstraint::parse(">9.0.0").unwrap()),
    };
    assert!(matches!(
        service.preflight(&unavailable).await.unwrap(),
        UpdatePreflight::UpToDate
    ));

    let error = service
        .refresh_repository_requirement("team", "missing")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("repos refresh team"));

    let invalid_manager = RepositoryManager::from_definitions(vec![RepositoryDefinition {
        name: "invalid".to_string(),
        repo_type: RepositoryType::HttpRegistry,
        priority: 0,
        config: RepositoryConfig::HttpRegistry {
            index_url: "://invalid".to_string(),
        },
        auth: None,
        storage: None,
    }]);
    let invalid = service.with_repository_manager(Arc::new(invalid_manager));
    assert!(invalid
        .refresh_repository_requirement("invalid", "test-skill")
        .await
        .unwrap_err()
        .to_string()
        .contains("failed to connect"));
}

#[tokio::test]
async fn relative_local_origin_uses_injected_project_root() {
    let tmp = TestTempDir::new().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    write_valid_skill(&project, "source");
    std::fs::write(
        project.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let service = make_service(&project.join("skills"))
        .await
        .with_project_root(project.clone());

    let prepared = service
        .prepare_add(
            Origin::Local {
                path: "source".into(),
                editable: false,
            },
            Some("test-skill"),
        )
        .await
        .unwrap();
    assert_eq!(prepared.id(), "test-skill");

    service
        .add_from_origin(
            Origin::Local {
                path: "source".into(),
                editable: false,
            },
            AddMode::Fresh,
            Vec::new(),
        )
        .await
        .unwrap();
    assert!(project.join("skills/test-skill/SKILL.md").exists());
}

#[tokio::test]
async fn prepared_acquisition_covers_cached_git_subdir_and_repository_errors() {
    let tmp = TestTempDir::new().unwrap();
    let service = make_service(&tmp.path().join("storage")).await;
    let cached = write_valid_skill(tmp.path(), "cached-git");
    let sha = "b".repeat(40);
    service
        .skill_cache()
        .put(&CacheIdentity::Git { sha: sha.clone() }, &cached)
        .unwrap();
    let error = service
        .prepare_add(
            Origin::Git {
                url: "https://example.invalid/repo.git".to_string(),
                r#ref: GitRef::Commit(sha),
                subdir: Some("missing".into()),
            },
            Some("test-skill"),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not exist"));

    let error = service
        .prepare_add(
            Origin::Repository {
                repo: "default".to_string(),
                skill: "test-skill".to_string(),
                version: Some(VersionConstraint::parse("1.0.0").unwrap()),
            },
            Some("test-skill"),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("No repositories configured"));
}

#[tokio::test]
async fn legacy_single_add_reports_missing_and_skill_level_manifests() {
    let tmp = TestTempDir::new().unwrap();
    let source = write_valid_skill(tmp.path(), "source");
    let project = tmp.path().join("empty-project");
    std::fs::create_dir(&project).unwrap();
    let service = make_service(&tmp.path().join("storage"))
        .await
        .with_project_root(project.clone());
    let error = service
        .add_from_origin(
            Origin::Local {
                path: source.clone(),
                editable: false,
            },
            AddMode::Fresh,
            Vec::new(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("skill-project.toml not found"));

    std::fs::write(
        project.join("skill-project.toml"),
        "[metadata]\nid = \"project\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    std::fs::write(project.join("SKILL.md"), VALID_SKILL_MD).unwrap();
    let service = make_service(&tmp.path().join("storage-two"))
        .await
        .with_project_root(project);
    let error = service
        .add_from_origin(
            Origin::Local {
                path: source,
                editable: false,
            },
            AddMode::Fresh,
            Vec::new(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("skill-level"));
}

// ── resolve_repo_name ──────────────────────────────────────────────────────

#[test]
fn test_resolve_repo_name_default_alias() {
    use crate::core::repository::{RepositoryConfig, RepositoryDefinition, RepositoryType};
    let manager = RepositoryManager::from_definitions(vec![RepositoryDefinition {
        name: "my-registry".to_string(),
        repo_type: RepositoryType::HttpRegistry,
        priority: 0,
        config: RepositoryConfig::HttpRegistry {
            index_url: "https://example.com/index".to_string(),
        },
        auth: None,
        storage: None,
    }]);
    assert_eq!(
        resolve_repo_name(&manager, "default").unwrap(),
        "my-registry"
    );
    assert_eq!(
        resolve_repo_name(&manager, "my-registry").unwrap(),
        "my-registry"
    );
}

#[test]
fn test_resolve_repo_name_no_repositories_errors() {
    let manager = RepositoryManager::from_definitions(Vec::new());
    assert!(resolve_repo_name(&manager, "default").is_err());
}

// ── safe_subdir_join ───────────────────────────────────────────────────────

#[test]
fn test_safe_subdir_join_rejects_dotdot() {
    let root = TestTempDir::new().unwrap();
    let result = safe_subdir_join(root.path(), Path::new("../../../etc"));
    assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
}

#[test]
fn test_safe_subdir_join_rejects_absolute() {
    let root = TestTempDir::new().unwrap();
    let result = safe_subdir_join(root.path(), Path::new("/etc/passwd"));
    assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
}

#[test]
fn test_safe_subdir_join_accepts_nested_relative() {
    let root = TestTempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("skills/inner")).unwrap();
    let joined = safe_subdir_join(root.path(), Path::new("skills/inner")).unwrap();
    assert_eq!(joined, root.path().join("skills").join("inner"));
}

// ── copy_dir_recursive ─────────────────────────────────────────────────────

#[cfg(unix)]
#[tokio::test]
async fn test_copy_dir_recursive_rejects_symlink() {
    use std::os::unix::fs::symlink;
    let tmp = TestTempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("SKILL.md"), "# skill\n").unwrap();
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "TOP SECRET").unwrap();
    symlink(&secret, src.join("creds")).unwrap();

    let dst = tmp.path().join("dst");
    let result = copy_dir_recursive(&src, &dst).await;
    assert!(matches!(result, Err(ServiceError::Validation(_))));
    assert!(!dst.join("creds").exists());
}

#[tokio::test]
async fn test_copy_dir_recursive_copies_regular_tree() {
    let tmp = TestTempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("SKILL.md"), "# skill\n").unwrap();
    std::fs::write(src.join("nested/file.txt"), "data").unwrap();

    let dst = tmp.path().join("dst");
    copy_dir_recursive(&src, &dst).await.unwrap();
    assert!(dst.join("SKILL.md").exists());
    assert!(dst.join("nested/file.txt").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn failed_replacement_preserves_existing_install() {
    use std::os::unix::fs::symlink;

    let tmp = TestTempDir::new().unwrap();
    let source = tmp.path().join("source");
    let destination = tmp.path().join("installed");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("marker"), "working").unwrap();
    std::fs::write(source.join("SKILL.md"), VALID_SKILL_MD).unwrap();
    symlink(source.join("SKILL.md"), source.join("unsafe-link")).unwrap();

    assert!(move_or_copy_into_storage(&source, &destination)
        .await
        .is_err());
    assert_eq!(
        std::fs::read_to_string(destination.join("marker")).unwrap(),
        "working"
    );
}

// ── strip_git_dir (cache-bloat bugfix) ────────────────────────────────────

#[tokio::test]
async fn test_strip_git_dir_removes_a_directory_git() {
    let tmp = TestTempDir::new().unwrap();
    let root = tmp.path().join("clone");
    std::fs::create_dir_all(root.join(".git/objects")).unwrap();
    std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(root.join("SKILL.md"), "# skill\n").unwrap();

    strip_git_dir(&root).await.unwrap();

    assert!(!root.join(".git").exists(), ".git must be removed");
    assert!(
        root.join("SKILL.md").exists(),
        "sibling skill content must be untouched"
    );
}

#[tokio::test]
async fn test_strip_git_dir_is_a_noop_when_absent() {
    let tmp = TestTempDir::new().unwrap();
    let root = tmp.path().join("clone");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("SKILL.md"), "# skill\n").unwrap();

    // Must not error just because there is nothing to strip.
    strip_git_dir(&root).await.unwrap();
    assert!(root.join("SKILL.md").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn test_strip_git_dir_unlinks_without_following_a_symlinked_git() {
    use std::os::unix::fs::symlink;
    let tmp = TestTempDir::new().unwrap();
    let root = tmp.path().join("clone");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("SKILL.md"), "# skill\n").unwrap();

    // A directory outside `root` that a symlinked `.git` could otherwise
    // redirect a recursive removal into; it must survive untouched.
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("sentinel.txt"), "do not delete me").unwrap();
    symlink(&outside, root.join(".git")).unwrap();

    strip_git_dir(&root).await.unwrap();

    assert!(
        !root.join(".git").exists(),
        "the symlink entry itself must be gone"
    );
    assert!(
        outside.join("sentinel.txt").is_file(),
        "must never follow the symlink to delete its target"
    );
}

// ── compute_local_tree_hash / hash_bytes (US-004) ─────────────────────────

#[test]
fn test_tree_hash_stable_across_mtime_touch() {
    let tmp = TestTempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("SKILL.md"), "---\nname: x\n---\n").unwrap();
    std::fs::write(src.join("nested/file.txt"), "data").unwrap();

    let before = compute_local_tree_hash(&src).unwrap();

    // Touch the file's mtime only, content untouched. Open for *write*:
    // Windows refuses to set a file's modified time through a read-only
    // handle ("Access is denied"), while unix is happy either way.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(src.join("nested/file.txt"))
        .unwrap();
    let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    file.set_modified(new_time).unwrap();

    let after = compute_local_tree_hash(&src).unwrap();
    assert_eq!(
        before, after,
        "touching a file's mtime alone must not change the tree-hash"
    );
}

#[test]
fn test_tree_hash_changes_when_content_changes() {
    let tmp = TestTempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("SKILL.md"), "---\nname: x\n---\nv1\n").unwrap();

    let before = compute_local_tree_hash(&src).unwrap();

    std::fs::write(src.join("SKILL.md"), "---\nname: x\n---\nv2\n").unwrap();
    let after = compute_local_tree_hash(&src).unwrap();

    assert_ne!(
        before, after,
        "changing a file's content must change the tree-hash"
    );
}

#[test]
fn test_tree_hash_independent_of_directory_read_order() {
    let tmp = TestTempDir::new().unwrap();

    // Same relative paths/contents, written in a different order.
    let a = tmp.path().join("a");
    std::fs::create_dir_all(a.join("nested")).unwrap();
    std::fs::write(a.join("SKILL.md"), "one").unwrap();
    std::fs::write(a.join("nested/file.txt"), "two").unwrap();

    let b = tmp.path().join("b");
    std::fs::create_dir_all(b.join("nested")).unwrap();
    std::fs::write(b.join("nested/file.txt"), "two").unwrap();
    std::fs::write(b.join("SKILL.md"), "one").unwrap();

    assert_eq!(
        compute_local_tree_hash(&a).unwrap(),
        compute_local_tree_hash(&b).unwrap(),
        "identical (path, content) pairs must hash the same regardless of write/read order"
    );
}

#[test]
fn test_tree_hash_sensitive_to_path_not_just_content() {
    let tmp = TestTempDir::new().unwrap();

    let a = tmp.path().join("a");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::write(a.join("one.txt"), "same").unwrap();

    let b = tmp.path().join("b");
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(b.join("two.txt"), "same").unwrap();

    assert_ne!(
        compute_local_tree_hash(&a).unwrap(),
        compute_local_tree_hash(&b).unwrap(),
        "renaming a file must change the tree-hash even with identical content"
    );
}

#[cfg(unix)]
#[test]
fn test_tree_hash_rejects_symlink() {
    use std::os::unix::fs::symlink;
    let tmp = TestTempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("SKILL.md"), "# skill\n").unwrap();
    let secret = tmp.path().join("secret.txt");
    std::fs::write(&secret, "TOP SECRET").unwrap();
    symlink(&secret, src.join("creds")).unwrap();

    let result = compute_local_tree_hash(&src);
    assert!(matches!(result, Err(ServiceError::Validation(_))));
}

#[test]
fn test_hash_bytes_is_deterministic_and_content_sensitive() {
    let h1 = hash_bytes(b"hello");
    let h2 = hash_bytes(b"hello");
    let h3 = hash_bytes(b"world");
    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
}

// ── fetch_local: content-cache hit/miss (US-004) ──────────────────────────

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_fetch_local_dir_second_install_hits_cache_not_source() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let cache_root = TestTempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        skill_cache_root: Some(cache_root.path().to_path_buf()),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let origin = Origin::Local {
        path: src.clone(),
        editable: false,
    };

    service
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("first add should succeed (cache miss)");
    let entries_after_first = service.skill_cache().stats().unwrap().local.entry_count;
    assert_eq!(entries_after_first, 1);

    // Mutate the *original* source path so a re-copy would be observable
    // as different content -- proving a hit never touches it again.
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: test-skill\nversion: \"1.0.0\"\ndescription: A test skill\n---\nMUTATED\n",
    )
    .unwrap();

    // Re-install (Update) from a *content-identical-to-the-original*
    // copy of the source at a different path, so it resolves to the
    // same tree-hash identity as the first install without ever
    // re-reading the (now mutated) original.
    let src2 = write_valid_skill(tmp.path(), "src-skill-2");
    let origin2 = Origin::Local {
        path: src2,
        editable: false,
    };
    let outcome = service
        .add_from_origin(origin2, AddMode::Update, vec![])
        .await
        .expect("second add should succeed (cache hit)");
    assert_eq!(
        service.skill_cache().stats().unwrap().local.entry_count,
        entries_after_first,
        "second install of the same identity must reuse one cache entry"
    );

    let installed = std::fs::read_to_string(skills_dir.join(&outcome.id).join("SKILL.md")).unwrap();
    assert_eq!(
        installed, VALID_SKILL_MD,
        "cache hit must be byte-identical (FR-8)"
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_fetch_local_editable_bypasses_cache() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let src = write_valid_skill(tmp.path(), "src-skill");
    let cache_root = TestTempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        skill_cache_root: Some(cache_root.path().to_path_buf()),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let origin = Origin::Local {
        path: src.clone(),
        editable: true,
    };
    service
        .add_from_origin(origin, AddMode::Fresh, vec![])
        .await
        .expect("editable add should succeed");

    // FR-7: nothing gets published to the content cache for an editable install.
    let identity = CacheIdentity::Local {
        tree_hash: compute_local_tree_hash(&src).unwrap(),
    };
    assert!(
        service.skill_cache().get(&identity).is_none(),
        "editable install must bypass the content cache entirely"
    );
}

// ── fetch_local: `.zip` content-cache (US-004) ─────────────────────────────

fn build_local_skill_zip(compression: zip::CompressionMethod) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = zip::ZipWriter::new(cursor);
        let opts = SimpleFileOptions::default().compression_method(compression);
        writer.start_file("test-skill/SKILL.md", opts).unwrap();
        writer.write_all(VALID_SKILL_MD.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buf
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_fetch_local_zip_second_install_hits_cache_not_source() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let cache_root = TestTempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        skill_cache_root: Some(cache_root.path().to_path_buf()),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    let zip_bytes = build_local_skill_zip(zip::CompressionMethod::Stored);
    let zip_a = tmp.path().join("a.zip");
    std::fs::write(&zip_a, &zip_bytes).unwrap();

    service
        .add_from_origin(
            Origin::Local {
                path: zip_a,
                editable: false,
            },
            AddMode::Fresh,
            vec![],
        )
        .await
        .expect("first zip add should succeed (cache miss)");
    let entries_after_first = service.skill_cache().stats().unwrap().local.entry_count;
    assert_eq!(entries_after_first, 1);

    // Byte-identical zip at a different path -- same archive identity.
    let zip_b = tmp.path().join("b.zip");
    std::fs::write(&zip_b, &zip_bytes).unwrap();
    service
        .add_from_origin(
            Origin::Local {
                path: zip_b,
                editable: false,
            },
            AddMode::Update,
            vec![],
        )
        .await
        .expect("second zip add should succeed (cache hit)");
    assert_eq!(
        service.skill_cache().stats().unwrap().local.entry_count,
        entries_after_first,
        "second install of a byte-identical zip must reuse one cache entry"
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_local_zip_identity_hashes_archive_bytes_not_extracted_tree() {
    let _lock = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, _guard, skills_dir) = setup_project();
    let cache_root = TestTempDir::new().unwrap();
    let config = ServiceConfig {
        skill_storage_path: skills_dir.clone(),
        skill_cache_root: Some(cache_root.path().to_path_buf()),
        ..Default::default()
    };
    let mut service = FastSkillService::new(config).await.unwrap();
    service.initialize().await.unwrap();

    // Two archives that extract to byte-identical content, but whose own
    // bytes differ (different compression method). If identity were based
    // on the extracted tree, the second install would be a cache hit; per
    // FR (US-004), a `.zip` hashes the archive's own bytes, so it must not
    // be.
    let stored = tmp.path().join("stored.zip");
    std::fs::write(
        &stored,
        build_local_skill_zip(zip::CompressionMethod::Stored),
    )
    .unwrap();
    let deflated = tmp.path().join("deflated.zip");
    std::fs::write(
        &deflated,
        build_local_skill_zip(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    assert_ne!(
        std::fs::read(&stored).unwrap(),
        std::fs::read(&deflated).unwrap(),
        "test fixture sanity: the two archives must differ at the byte level"
    );

    service
        .add_from_origin(
            Origin::Local {
                path: stored,
                editable: false,
            },
            AddMode::Fresh,
            vec![],
        )
        .await
        .expect("stored-compression zip add should succeed");
    service
        .add_from_origin(
            Origin::Local {
                path: deflated,
                editable: false,
            },
            AddMode::Update,
            vec![],
        )
        .await
        .expect("deflated-compression zip add should succeed");

    assert_eq!(
        service.skill_cache().stats().unwrap().local.entry_count,
        2,
        "differently-encoded archives with identical extracted content must be \
             distinct cache identities (archive bytes, not the extracted tree)"
    );
}

// ── derive_skill_id_and_version ────────────────────────────────────────────

#[test]
fn test_derive_skill_id_and_version_toml_wins() {
    let tmp = TestTempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: from-md\nversion: \"2.0.0\"\ndescription: d\n---\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("skill-project.toml"),
        "[metadata]\nid = \"from-toml\"\nversion = \"1.5.0\"\n",
    )
    .unwrap();
    let content = std::fs::read_to_string(tmp.path().join("SKILL.md")).unwrap();
    let frontmatter = parse_yaml_frontmatter(&content).unwrap();
    let (id, version) = derive_skill_id_and_version(tmp.path(), &frontmatter).unwrap();
    assert_eq!(id.as_str(), "from-toml");
    assert_eq!(version, "1.5.0");
}

#[test]
fn test_derive_skill_id_and_version_falls_back_to_frontmatter_name() {
    let tmp = TestTempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("SKILL.md"),
        "---\nname: fallback-name\ndescription: d\n---\n",
    )
    .unwrap();
    let content = std::fs::read_to_string(tmp.path().join("SKILL.md")).unwrap();
    let frontmatter = parse_yaml_frontmatter(&content).unwrap();
    let (id, version) = derive_skill_id_and_version(tmp.path(), &frontmatter).unwrap();
    assert_eq!(id.as_str(), "fallback-name");
    assert_eq!(version, "1.0.0");
}

#[tokio::test]
async fn storage_helpers_report_invalid_inputs_and_remove_each_path_kind() {
    let tmp = TestTempDir::new().unwrap();
    let invalid = write_valid_skill(tmp.path(), "invalid-manifest");
    std::fs::write(invalid.join("skill-project.toml"), "[metadata\n").unwrap();
    let content = std::fs::read_to_string(invalid.join("SKILL.md")).unwrap();
    let frontmatter = parse_yaml_frontmatter(&content).unwrap();
    assert!(derive_skill_id_and_version(&invalid, &frontmatter).is_err());

    let file = tmp.path().join("file");
    std::fs::write(&file, "x").unwrap();
    remove_existing_storage_path(&file).await.unwrap();
    assert!(!file.exists());
    let directory = tmp.path().join("directory");
    std::fs::create_dir(&directory).unwrap();
    remove_existing_storage_path(&directory).await.unwrap();
    assert!(!directory.exists());

    assert!(git_head_commit(tmp.path()).await.is_err());
    assert!(move_or_copy_into_storage(tmp.path(), Path::new("/"))
        .await
        .is_err());
    assert!(symlink_into_storage(tmp.path(), Path::new("/"))
        .await
        .is_err());
}

#[cfg(unix)]
#[test]
fn safe_subdir_join_rejects_a_symlink_escape() {
    let tmp = TestTempDir::new().unwrap();
    let root = tmp.path().join("root");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();

    let error = safe_subdir_join(&root, Path::new("escape")).unwrap_err();
    assert!(error.to_string().contains("escapes the cloned repository"));
}

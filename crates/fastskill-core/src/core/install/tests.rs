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

// ── fetch_git: GitRef::Commit is a clear, fast error (no clone-by-commit) ──

#[tokio::test]
async fn test_add_from_origin_git_commit_ref_unsupported() {
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
    assert!(matches!(result, Err(ServiceError::InvalidOperation(_))));
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

    let baseline = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst);
    service
        .add_from_origin(origin.clone(), AddMode::Fresh, vec![])
        .await
        .expect("first add should succeed (cache miss)");
    assert_eq!(
        LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
        1,
        "first install must copy from the source (cache miss)"
    );

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
        LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
        1,
        "second install of the same identity must hit the cache, not copy again"
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

    let baseline = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst);
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
    assert_eq!(
        LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
        1,
        "first install must extract the zip (cache miss)"
    );

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
        LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
        1,
        "second install of a byte-identical zip must hit the cache, not re-extract"
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

    let baseline = LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst);
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
        LOCAL_COPY_INVOCATIONS.load(Ordering::SeqCst) - baseline,
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

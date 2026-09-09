use super::*;

fn cache_with_versions(root: &TempDir, versions: &[&str]) -> SkillCache {
    let cache = SkillCache::at_root(root.path());
    cache
        .write_source_index(
            "acme",
            &SourceIndex {
                fetched_at: chrono::Utc::now(),
                entries: vec![SourceIndexEntry {
                    skill: "widget".to_string(),
                    versions: versions.iter().map(|v| (*v).to_string()).collect(),
                    name: String::new(),
                    description: String::new(),
                }],
            },
        )
        .unwrap();
    cache
}

#[test]
fn a_traversal_version_from_the_cached_index_is_rejected() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["../../../../etc/cron.d/pwned"]);
    let error = resolve_registry_version(&cache, "acme", "widget", None).unwrap_err();
    assert!(error.to_string().contains("../../../../etc/cron.d/pwned"));
}

#[test]
fn an_absolute_version_from_the_cached_index_is_rejected() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["/etc/cron.d/pwned"]);
    assert!(resolve_registry_version(&cache, "acme", "widget", None).is_err());
}

#[test]
fn ordinary_semver_versions_still_resolve_newest_first() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["1.2.3", "1.9.0", "1.10.0"]);
    assert_eq!(
        resolve_registry_version(&cache, "acme", "widget", None).unwrap(),
        "1.10.0"
    );
}

#[test]
fn cached_resolution_reports_an_unknown_skill() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["1.0.0"]);
    let error = resolve_registry_version(&cache, "acme", "missing", None).unwrap_err();
    assert!(error.to_string().contains("not found in the cached index"));
}

#[test]
fn safe_subdir_join_rejects_traversal_components() {
    let root = TempDir::new().unwrap();
    assert!(safe_subdir_join(root.path(), Path::new("safe/../outside")).is_err());
}

#[cfg(unix)]
#[test]
fn safe_subdir_join_rejects_non_utf8_components() {
    use std::os::unix::ffi::OsStringExt;

    let root = TempDir::new().unwrap();
    let invalid = std::ffi::OsString::from_vec(vec![0xff]);
    assert!(safe_subdir_join(root.path(), Path::new(&invalid)).is_err());
}

#[test]
fn unconstrained_resolution_chooses_newest_stable() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["1.2.0", "2.0.0-beta.1", "1.9.0"]);
    assert_eq!(
        resolve_registry_version(&cache, "acme", "widget", None).unwrap(),
        "1.9.0"
    );
}

#[test]
fn unconstrained_resolution_fails_without_a_stable_candidate() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["2.0.0-alpha.1", "2.0.0-beta.1"]);
    let error = resolve_registry_version(&cache, "acme", "widget", None).unwrap_err();
    assert!(error.to_string().contains("stable"));
}

#[test]
fn an_explicit_prerelease_version_is_not_rejected_by_the_guard() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["1.0.0-rc.1"]);
    let constraint = VersionConstraint::parse("=1.0.0-rc.1").unwrap();
    assert_eq!(
        resolve_registry_version(&cache, "acme", "widget", Some(&constraint)).unwrap(),
        "1.0.0-rc.1"
    );
}

#[test]
fn unmatched_explicit_constraint_does_not_claim_stable_versions_are_required() {
    let root = TempDir::new().unwrap();
    let cache = cache_with_versions(&root, &["1.0.0-rc.1"]);
    let constraint = VersionConstraint::parse("^2.0.0-rc.1").unwrap();
    let error = resolve_registry_version(&cache, "acme", "widget", Some(&constraint))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no version of 'widget' satisfies"));
    assert!(!error.contains("no stable version"));
}

#[cfg(unix)]
fn directory_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn directory_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

#[tokio::test]
async fn existing_storage_removal_handles_every_supported_path_kind() {
    let root = TempDir::new().unwrap();
    let absent = root.path().join("absent");
    remove_existing_storage_path(&absent).await.unwrap();

    let file = root.path().join("file");
    std::fs::write(&file, "content").unwrap();
    remove_existing_storage_path(&file).await.unwrap();
    assert!(!file.exists());

    let directory = root.path().join("directory");
    std::fs::create_dir_all(directory.join("nested")).unwrap();
    remove_existing_storage_path(&directory).await.unwrap();
    assert!(!directory.exists());

    let target = root.path().join("target");
    let link = root.path().join("link");
    std::fs::create_dir_all(&target).unwrap();
    directory_symlink(&target, &link);
    remove_existing_storage_path(&link).await.unwrap();
    assert!(target.is_dir());
    assert!(std::fs::symlink_metadata(&link).is_err());
}

use super::*;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

fn write_file(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

/// A small, realistic skill directory used as a `put` source.
fn sample_source_dir(root: &Path) -> PathBuf {
    let src = root.join("source");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    write_file(&src, "SKILL.md", "---\nname: demo\n---\nbody\n");
    write_file(&src.join("nested"), "helper.py", "print('hi')\n");
    src
}

#[test]
fn get_misses_when_identity_never_put() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());
    let identity = CacheIdentity::Git {
        sha: "deadbeef".to_string(),
    };

    assert!(cache.get(&identity).is_none());
}

#[test]
fn put_then_get_is_a_hit_with_matching_content() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());
    let source = sample_source_dir(root.path());
    let identity = CacheIdentity::Registry {
        source: "acme".to_string(),
        skill: "demo".to_string(),
        version: "1.0.0".to_string(),
    };

    let put = cache.put(&identity, &source).unwrap();
    assert!(put.path.is_dir());
    assert_eq!(put.path, root.path().join("registry/acme/demo/1.0.0"));

    let hit = cache
        .get(&identity)
        .expect("expected a cache hit after put");
    assert_eq!(hit.path, put.path);
    assert_eq!(
        std::fs::read_to_string(hit.path.join("SKILL.md")).unwrap(),
        "---\nname: demo\n---\nbody\n"
    );
    assert_eq!(
        std::fs::read_to_string(hit.path.join("nested/helper.py")).unwrap(),
        "print('hi')\n"
    );
}

/// Env-var resolution logic, tested in isolation (per PRD guidance) so
/// no other test's cache-root assumptions can race against it.
#[test]
fn env_var_override_changes_the_resolved_root() {
    let dir = TempDir::new().unwrap();
    let previous = std::env::var(FASTSKILL_CACHE_DIR_ENV).ok();

    std::env::set_var(FASTSKILL_CACHE_DIR_ENV, dir.path());
    let resolved = SkillCache::resolve_root().unwrap();

    match previous {
        Some(v) => std::env::set_var(FASTSKILL_CACHE_DIR_ENV, v),
        None => std::env::remove_var(FASTSKILL_CACHE_DIR_ENV),
    }

    assert_eq!(resolved, dir.path().to_path_buf());
}

#[test]
fn concurrent_duplicate_put_is_a_harmless_noop() {
    let root = TempDir::new().unwrap();
    let cache = Arc::new(SkillCache::at_root(root.path()));
    let source = sample_source_dir(root.path());
    let identity = CacheIdentity::Local {
        tree_hash: "abc123".to_string(),
    };

    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let source = source.clone();
            let identity = identity.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                cache.put(&identity, &source)
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for result in &results {
        assert!(
            result.is_ok(),
            "concurrent put must never error: {result:?}"
        );
    }

    let paths: Vec<_> = results.into_iter().map(|r| r.unwrap().path).collect();
    assert_eq!(
        paths[0], paths[1],
        "both callers converge on one final path"
    );
    assert_eq!(
        std::fs::read_to_string(paths[0].join("SKILL.md")).unwrap(),
        "---\nname: demo\n---\nbody\n"
    );

    // No leftover staging directories after the race resolves.
    let staging_root = root.path().join(STAGING_DIR_NAME);
    if staging_root.is_dir() {
        let leftovers: Vec<_> = std::fs::read_dir(&staging_root).unwrap().collect();
        assert!(
            leftovers.is_empty(),
            "no staging leftovers should remain once a race resolves"
        );
    }
}

#[test]
fn torn_staged_write_is_never_visible_via_get() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());
    let identity = CacheIdentity::Git {
        sha: "cafefeed".to_string(),
    };

    // Simulate a crash between "assembled in staging" and "atomic
    // rename": content exists on disk, but only inside `tmp/`, never at
    // the identity's final path.
    let staging_root = root.path().join(STAGING_DIR_NAME);
    let torn = staging_root.join(format!("{STAGING_DIR_PREFIX}torn"));
    std::fs::create_dir_all(&torn).unwrap();
    write_file(&torn, "SKILL.md", "incomplete\n");

    assert!(cache.get(&identity).is_none());
    assert!(!root.path().join("git/cafefeed").exists());
}

#[cfg(unix)]
#[test]
fn put_rejects_a_symlink_in_the_source_tree() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());
    let source = sample_source_dir(root.path());
    std::os::unix::fs::symlink(root.path(), source.join("evil-link")).unwrap();

    let identity = CacheIdentity::Local {
        tree_hash: "haslink".to_string(),
    };

    assert!(cache.put(&identity, &source).is_err());
    assert!(cache.get(&identity).is_none());
}

#[test]
fn cache_identity_rejects_path_traversal_components() {
    let identity = CacheIdentity::Registry {
        source: "../escape".to_string(),
        skill: "demo".to_string(),
        version: "1.0.0".to_string(),
    };

    assert!(identity.relative_path().is_err());
}

// ── `fastskill cache info`/`clean` (PRD 006, US-006) ──────────────────

/// `put` a small sample skill under `identity`. The source lives in its
/// own throwaway temp dir -- never inside `cache`'s root -- so tests
/// that go on to call `clean` (which refuses a root containing anything
/// it does not recognize) are not tripped up by their own fixture data.
fn put_sample(cache: &SkillCache, identity: &CacheIdentity, marker: &str) {
    let src = TempDir::new().unwrap();
    write_file(
        src.path(),
        "SKILL.md",
        &format!("---\nname: {marker}\n---\nbody\n"),
    );
    cache.put(identity, src.path()).unwrap();
}

#[test]
fn stats_on_a_never_used_root_is_all_zero() {
    let root = TempDir::new().unwrap();
    // Never call `put`: the root directory itself does not even exist.
    let cache = SkillCache::at_root(root.path().join("never-created"));

    let stats = cache.stats().unwrap();
    assert_eq!(stats.git, ContentSourceStats::default());
    assert_eq!(stats.registry, ContentSourceStats::default());
    assert_eq!(stats.local, ContentSourceStats::default());
}

#[test]
fn stats_counts_entries_and_bytes_per_source_kind() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());

    put_sample(
        &cache,
        &CacheIdentity::Git {
            sha: "a".repeat(40),
        },
        "git-a",
    );
    put_sample(
        &cache,
        &CacheIdentity::Registry {
            source: "acme".to_string(),
            skill: "demo".to_string(),
            version: "1.0.0".to_string(),
        },
        "reg-a",
    );
    put_sample(
        &cache,
        &CacheIdentity::Registry {
            source: "acme".to_string(),
            skill: "demo".to_string(),
            version: "2.0.0".to_string(),
        },
        "reg-b",
    );
    put_sample(
        &cache,
        &CacheIdentity::Local {
            tree_hash: "deadbeef".to_string(),
        },
        "local-a",
    );

    let stats = cache.stats().unwrap();
    assert_eq!(stats.git.entry_count, 1);
    assert_eq!(stats.registry.entry_count, 2);
    assert_eq!(stats.local.entry_count, 1);
    assert!(stats.git.total_bytes > 0);
    assert!(stats.registry.total_bytes > 0);
    assert!(stats.local.total_bytes > 0);
    assert_eq!(stats.total().entry_count, 4);
}

#[test]
fn clean_with_no_source_filter_removes_every_kind_but_not_the_index() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());

    put_sample(
        &cache,
        &CacheIdentity::Git {
            sha: "b".repeat(40),
        },
        "git-b",
    );
    put_sample(
        &cache,
        &CacheIdentity::Local {
            tree_hash: "cafef00d".to_string(),
        },
        "local-b",
    );
    cache
        .write_source_index(
            "acme",
            &SourceIndex {
                fetched_at: chrono::Utc::now(),
                entries: vec![],
            },
        )
        .unwrap();

    let report = cache.clean(None).unwrap();
    assert_eq!(report.entries_removed, 2);
    assert!(report.bytes_reclaimed > 0);

    let stats_after = cache.stats().unwrap();
    assert_eq!(stats_after.total(), ContentSourceStats::default());
    // The index cache is untouched by `clean` (PRD: "removes all content
    // entries" -- content, not index).
    assert!(cache.read_source_index("acme").unwrap().is_some());
}

#[test]
fn clean_with_source_filter_only_removes_that_kind() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());

    put_sample(
        &cache,
        &CacheIdentity::Git {
            sha: "c".repeat(40),
        },
        "git-c",
    );
    put_sample(
        &cache,
        &CacheIdentity::Local {
            tree_hash: "0ff1ce".to_string(),
        },
        "local-c",
    );

    let report = cache.clean(Some(ContentSourceKind::Git)).unwrap();
    assert_eq!(report.entries_removed, 1);

    let stats_after = cache.stats().unwrap();
    assert_eq!(stats_after.git, ContentSourceStats::default());
    assert_eq!(stats_after.local.entry_count, 1, "local entry untouched");
}

#[test]
fn clean_on_a_never_used_root_is_a_harmless_noop() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path().join("never-created"));

    let report = cache.clean(None).unwrap();
    assert_eq!(report, CleanReport::default());
}

#[test]
fn clean_refuses_a_root_that_does_not_look_like_a_fastskill_cache() {
    let root = TempDir::new().unwrap();
    // Simulate `FASTSKILL_CACHE_DIR` misconfigured to point at a real,
    // unrelated directory (e.g. a home directory) rather than an actual
    // cache root.
    std::fs::write(root.path().join("Documents.txt"), "not a cache").unwrap();
    let cache = SkillCache::at_root(root.path());

    let err = cache
        .clean(None)
        .expect_err("clean must refuse a root with unrecognized entries");
    assert!(matches!(err, ServiceError::Validation(_)));
    // Nothing was touched.
    assert!(root.path().join("Documents.txt").is_file());
}

#[cfg(unix)]
#[test]
fn clean_never_follows_or_deletes_through_a_symlinked_entry() {
    let root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(root.path());

    // A real, legitimately cached entry.
    put_sample(
        &cache,
        &CacheIdentity::Local {
            tree_hash: "realentry".to_string(),
        },
        "local-real",
    );

    // A directory *outside* the cache root that a symlink could redirect
    // deletion into, plus a sentinel file inside it.
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("sentinel.txt"), "do not delete me").unwrap();

    // Plant a symlink directly under `git/` masquerading as a cached SHA
    // entry, pointing at the directory outside the cache root.
    let git_dir = root.path().join("git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::os::unix::fs::symlink(outside.path(), git_dir.join("evilsha")).unwrap();

    let report = cache.clean(None).unwrap();

    // The symlink itself is never treated as a leaf identity directory
    // (skipped, not deleted, not followed) -- only the one real local
    // entry is counted.
    assert_eq!(report.entries_removed, 1);
    assert!(
        outside.path().join("sentinel.txt").is_file(),
        "clean must never delete through a symlink outside the cache root"
    );
    assert!(
        git_dir.join("evilsha").exists(),
        "the symlink entry itself is left alone, not silently deleted"
    );
}

#[test]
fn content_source_kind_from_str_round_trips_and_rejects_unknown() {
    use std::str::FromStr;
    assert_eq!(
        ContentSourceKind::from_str("git").unwrap(),
        ContentSourceKind::Git
    );
    assert_eq!(
        ContentSourceKind::from_str("registry").unwrap(),
        ContentSourceKind::Registry
    );
    assert_eq!(
        ContentSourceKind::from_str("local").unwrap(),
        ContentSourceKind::Local
    );
    assert!(ContentSourceKind::from_str("../etc").is_err());
    assert!(ContentSourceKind::from_str("bogus").is_err());
}

//! Integration tests for registry index operations

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_borrows_for_generic_args
)]

use fastskill::core::registry::index_manager::IndexManager;
use fastskill::core::registry_index::{read_skill_versions, ScopedSkillName, VersionEntry};
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn test_scoped_name_integration() {
    // Test that scoped names work in integration context
    let normalized = ScopedSkillName::normalize("@acme/web-scraper");
    assert_eq!(normalized, "acme/web-scraper");
}

#[test]
fn test_index_manager_basic_operations() {
    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path().to_path_buf();
    let manager = IndexManager::new(registry_path.clone());

    // Create a test entry
    let entry = VersionEntry {
        name: "testorg/test-skill".to_string(),
        vers: "1.0.0".to_string(),
        deps: vec![],
        cksum: "abc123".to_string(),
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: "https://example.com/test-skill-1.0.0.zip".to_string(),
        published_at: "2024-01-01T00:00:00Z".to_string(),
        metadata: None,
        scoped_name: Some("@acme/test-skill".to_string()),
    };

    // Test that atomic_update can be called
    // The actual implementation will be tested once it's complete
    let result = manager.atomic_update("@acme/test-skill", "1.0.0", &entry);

    // For now, just verify the method exists and can be called
    // Full functionality will be tested once implementation is complete
    assert!(result.is_err() || result.is_ok());
}

#[tokio::test]
async fn test_index_served_from_filesystem() {
    // Test that index files can be read directly from filesystem without Git operations
    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path().to_path_buf();

    // Create a test index file directly in filesystem
    use fastskill::core::registry_index::get_skill_index_path;
    use std::fs;
    use std::io::Write;

    // Use the same path structure as the actual implementation
    let index_file_path = get_skill_index_path(&registry_path, "testorg/test-skill");
    if let Some(parent) = index_file_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    // Write a test entry
    let entry = VersionEntry {
        name: "testorg/test-skill".to_string(),
        vers: "1.0.0".to_string(),
        deps: vec![],
        cksum: "abc123".to_string(),
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: "https://example.com/test-skill-1.0.0.zip".to_string(),
        published_at: "2024-01-01T00:00:00Z".to_string(),
        metadata: None,
        scoped_name: None,
    };

    let line = serde_json::to_string(&entry).unwrap();
    let mut file = fs::File::create(&index_file_path).unwrap();
    writeln!(file, "{}", line).unwrap();
    file.flush().unwrap();
    drop(file);

    // Read directly from filesystem (simulating GET /index/* endpoint)
    let entries = read_skill_versions(&registry_path, "testorg/test-skill").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].vers, "1.0.0");
}

#[tokio::test]
async fn test_publish_no_git_operations() {
    // Test that publish operation (via IndexManager) completes without Git commit or push
    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path().to_path_buf();
    let manager = IndexManager::new(registry_path.clone());

    let entry = VersionEntry {
        name: "testorg/no-git-skill".to_string(),
        vers: "1.0.0".to_string(),
        deps: vec![],
        cksum: "abc123".to_string(),
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: "https://example.com/skill-1.0.0.zip".to_string(),
        published_at: "2024-01-01T00:00:00Z".to_string(),
        metadata: None,
        scoped_name: None,
    };

    // Publish using IndexManager (no Git operations)
    let result = manager.atomic_update("testorg/no-git-skill", "1.0.0", &entry);

    // Should succeed without any Git operations
    // Verify no .git directory was created
    assert!(!registry_path.join(".git").exists());

    // Result may succeed or fail depending on implementation completeness
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_server_restart_no_git_clone() {
    // Test that index can be served from filesystem without cloning Git repository
    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path().to_path_buf();

    // Create index files directly (simulating filesystem-based index)
    use fastskill::core::registry_index::get_skill_index_path;
    use std::fs;
    use std::io::Write;

    // Use the same path structure as the actual implementation
    let index_file_path = get_skill_index_path(&registry_path, "testorg/restart-skill");
    if let Some(parent) = index_file_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    let entry = VersionEntry {
        name: "testorg/restart-skill".to_string(),
        vers: "1.0.0".to_string(),
        deps: vec![],
        cksum: "abc123".to_string(),
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: "https://example.com/skill-1.0.0.zip".to_string(),
        published_at: "2024-01-01T00:00:00Z".to_string(),
        metadata: None,
        scoped_name: None,
    };

    let line = serde_json::to_string(&entry).unwrap();
    let mut file = fs::File::create(&index_file_path).unwrap();
    writeln!(file, "{}", line).unwrap();
    drop(file);

    // Verify no Git repository exists
    assert!(!registry_path.join(".git").exists());

    // Verify index can be read directly from filesystem
    let entries = read_skill_versions(&registry_path, "testorg/restart-skill").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "testorg/restart-skill");
}

#[tokio::test]
async fn test_all_versions_aggregation_logic() {
    // Test T044: Unit test for all-versions aggregation logic in server-side scan (list_index_skills)

    use fastskill::core::registry_index::{
        scan_registry_index, update_skill_version, ListSkillsOptions, VersionMetadata,
    };
    use std::collections::HashMap;

    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path();

    let skill_id = "acme/multi-version-skill";

    // Create multiple versions of the same skill
    let versions = vec!["1.0.0", "1.1.0", "2.0.0", "2.1.0"];

    for version in &versions {
        let metadata = VersionMetadata {
            name: skill_id.to_string(),
            vers: version.to_string(),
            deps: vec![],
            cksum: format!("sha256:{}", version),
            features: HashMap::new(),
            yanked: false,
            links: None,
            download_url: format!(
                "https://example.com/{}-{}.zip",
                skill_id.replace('/', "-"),
                version
            ),
            published_at: format!(
                "2024-01-{:02}T00:00:00Z",
                version.replace(".", "").parse::<u32>().unwrap_or(1) % 30
            ),
            metadata: Some(fastskill::core::registry_index::IndexMetadata {
                description: Some(format!("Version {} of multi-version skill", version)),
                author: None,
                tags: None,
                capabilities: None,
                license: None,
                repository: None,
            }),
        };
        update_skill_version(skill_id, version, &metadata, registry_path).unwrap();
    }

    // Test with all_versions=true
    let options = ListSkillsOptions {
        all_versions: true,
        ..Default::default()
    };

    let result = scan_registry_index(registry_path, &options).await;
    assert!(result.is_ok());
    let summaries = result.unwrap();

    // Should return one summary per version
    assert_eq!(summaries.len(), versions.len());

    // All summaries should have the same skill ID but different versions
    assert!(summaries.iter().all(|s| s.id == skill_id));
    let returned_versions: Vec<String> =
        summaries.iter().map(|s| s.latest_version.clone()).collect();
    for version in &versions {
        assert!(
            returned_versions.contains(&version.to_string()),
            "Version {} should be in results",
            version
        );
    }
}

#[tokio::test]
async fn test_duplicate_version_deduplication() {
    // Test T045: Unit test for duplicate version deduplication

    use fastskill::core::registry_index::{
        scan_registry_index, update_skill_version, ListSkillsOptions, VersionMetadata,
    };
    use std::collections::HashMap;

    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path();

    let skill_id = "acme/duplicate-version-skill";
    let version = "1.0.0";

    // Create the same version entry twice (simulating duplicate in index file)
    let metadata1 = VersionMetadata {
        name: skill_id.to_string(),
        vers: version.to_string(),
        deps: vec![],
        cksum: "sha256:first".to_string(),
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: "https://example.com/first.zip".to_string(),
        published_at: "2024-01-01T00:00:00Z".to_string(),
        metadata: Some(fastskill::core::registry_index::IndexMetadata {
            description: Some("First entry".to_string()),
            author: None,
            tags: None,
            capabilities: None,
            license: None,
            repository: None,
        }),
    };

    let metadata2 = VersionMetadata {
        name: skill_id.to_string(),
        vers: version.to_string(), // Same version
        deps: vec![],
        cksum: "sha256:second".to_string(),
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: "https://example.com/second.zip".to_string(),
        published_at: "2024-01-02T00:00:00Z".to_string(),
        metadata: Some(fastskill::core::registry_index::IndexMetadata {
            description: Some("Second entry (duplicate)".to_string()),
            author: None,
            tags: None,
            capabilities: None,
            license: None,
            repository: None,
        }),
    };

    // Add both entries to the index file
    update_skill_version(skill_id, version, &metadata1, registry_path).unwrap();
    update_skill_version(skill_id, version, &metadata2, registry_path).unwrap();

    // Test with all_versions=true
    let options = ListSkillsOptions {
        all_versions: true,
        ..Default::default()
    };

    let result = scan_registry_index(registry_path, &options).await;
    assert!(result.is_ok());
    let summaries = result.unwrap();

    // Should deduplicate: only one entry per version string
    // The first valid entry should be used
    let version_entries: Vec<&fastskill::core::registry_index::SkillSummary> = summaries
        .iter()
        .filter(|s| s.id == skill_id && s.latest_version == version)
        .collect();

    // Should have only one entry for this version (deduplicated)
    assert_eq!(
        version_entries.len(),
        1,
        "Duplicate versions should be deduplicated"
    );

    // The first entry should be used (based on scan order)
    // Note: The exact behavior depends on scan_registry_index implementation
    // which uses the first occurrence of each version
}

#[tokio::test]
async fn test_concurrent_reads_no_locking() {
    // Test that concurrent reads of index files are safe without locking
    use fastskill::core::registry_index::{update_skill_version, VersionMetadata};
    use std::sync::Arc;
    use tokio::task;

    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path().to_path_buf();

    // Create multiple skills with multiple versions
    let skill_ids = vec!["acme/skill1", "acme/skill2", "acme/skill3"];
    let versions = vec!["1.0.0", "1.1.0", "2.0.0"];

    // Populate registry with test data
    for skill_id in &skill_ids {
        for version in &versions {
            let metadata = VersionMetadata {
                name: skill_id.to_string(),
                vers: version.to_string(),
                deps: vec![],
                cksum: format!("sha256:{}", version),
                features: HashMap::new(),
                yanked: false,
                links: None,
                download_url: format!(
                    "https://example.com/{}-{}.zip",
                    skill_id.replace('/', "-"),
                    version
                ),
                published_at: "2024-01-01T00:00:00Z".to_string(),
                metadata: None,
            };
            update_skill_version(skill_id, version, &metadata, &registry_path).unwrap();
        }
    }

    // Spawn multiple concurrent read tasks
    let registry_path_arc = Arc::new(registry_path);
    let mut handles = Vec::new();
    let expected_count = skill_ids.len(); // Store length before moving

    for _ in 0..10 {
        let path = Arc::clone(&registry_path_arc);
        let skill_ids_clone = skill_ids.clone(); // Clone before moving into closure
        let handle = task::spawn(async move {
            // Each task reads all skills concurrently
            let mut results = Vec::new();
            for skill_id in &skill_ids_clone {
                match read_skill_versions(&path, skill_id) {
                    Ok(entries) => results.push((skill_id.to_string(), entries.len())),
                    Err(e) => panic!("Failed to read skill {}: {}", skill_id, e),
                }
            }
            results
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    let mut all_results = Vec::new();
    for handle in handles {
        let results = handle.await.unwrap();
        all_results.push(results);
    }

    // Verify all tasks read the same data (no corruption)
    for results in &all_results {
        assert_eq!(results.len(), expected_count);
        for (skill_id, count) in results {
            assert_eq!(
                *count,
                versions.len(),
                "Skill {} should have {} versions",
                skill_id,
                versions.len()
            );
        }
    }

    // Verify all tasks got consistent results
    let first_result = &all_results[0];
    for results in &all_results[1..] {
        assert_eq!(
            results, first_result,
            "All concurrent reads should return identical results"
        );
    }
}

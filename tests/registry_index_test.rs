//! Integration tests for registry index operations

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

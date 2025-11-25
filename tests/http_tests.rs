//! Integration tests for HTTP/registry endpoints

#[test]
fn test_marketplace_json_structure() {
    use fastskill::core::sources::{MarketplaceJson, MarketplaceSkill};

    let skill = MarketplaceSkill {
        id: "test-skill".to_string(),
        name: "Test Skill".to_string(),
        description: "A test skill".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Test Author".to_string()),
        tags: vec!["test".to_string(), "example".to_string()],
        capabilities: vec!["testing".to_string()],
        download_url: Some("https://example.com/skill.zip".to_string()),
    };

    let marketplace = MarketplaceJson {
        version: "1.0".to_string(),
        skills: vec![skill],
    };

    // Verify serialization
    let json = serde_json::to_string(&marketplace).unwrap();
    assert!(json.contains("test-skill"));
    assert!(json.contains("Test Skill"));
    assert!(json.contains("1.0.0"));
}

#[test]
fn test_marketplace_json_validation() {
    use fastskill::core::sources::MarketplaceSkill;

    // Valid skill
    let valid = MarketplaceSkill {
        id: "valid".to_string(),
        name: "Valid Skill".to_string(),
        description: "Valid description".to_string(),
        version: "1.0.0".to_string(),
        author: None,
        tags: vec![],
        capabilities: vec![],
        download_url: None,
    };

    assert!(!valid.id.is_empty());
    assert!(!valid.name.is_empty());
    assert!(!valid.description.is_empty());
}

/// Integration test to verify static directory path resolution works correctly.
/// This test ensures that the static directory can be found regardless of
/// the current working directory, preventing regressions of the bug where
/// hardcoded relative paths would fail when running from different directories.
#[test]
fn test_static_directory_resolution() {
    use fastskill::http::server::FastSkillServer;

    // Test that resolve_static_dir can find the static directory
    let static_dir = FastSkillServer::resolve_static_dir();

    // The path should exist (this test runs from the crate root)
    assert!(
        static_dir.exists(),
        "Static directory should exist at: {}. \
         This indicates the path resolution is broken. \
         Check that the static files are in the expected location.",
        static_dir.display()
    );

    // Verify it's actually a directory
    assert!(
        static_dir.is_dir(),
        "Static path should be a directory: {}",
        static_dir.display()
    );

    // Verify expected files exist - these are required for the registry UI to work
    let index_html = static_dir.join("index.html");
    let app_js = static_dir.join("app.js");
    let styles_css = static_dir.join("styles.css");

    assert!(
        index_html.exists(),
        "index.html should exist in static directory: {}. \
         Without this file, the registry UI will return 404.",
        static_dir.display()
    );
    assert!(
        app_js.exists(),
        "app.js should exist in static directory: {}. \
         This file is required for the registry UI functionality.",
        static_dir.display()
    );
    assert!(
        styles_css.exists(),
        "styles.css should exist in static directory: {}. \
         This file is required for the registry UI styling.",
        static_dir.display()
    );
}

/// Test that static directory resolution works from different working directories.
/// This simulates the bug scenario where the binary is run from a different
/// directory than the source code, ensuring path resolution handles this correctly.
#[test]
fn test_static_directory_resolution_from_different_cwd() {
    use fastskill::http::server::FastSkillServer;
    use std::env;

    // Save the original working directory
    let original_cwd = env::current_dir().expect("Should be able to get current directory");

    // Test resolution from the original directory
    let static_dir_original = FastSkillServer::resolve_static_dir();
    assert!(
        static_dir_original.exists(),
        "Static directory should be resolvable from original directory: {}",
        original_cwd.display()
    );

    // Change to a temporary directory (simulating running from a different location)
    let temp_dir = tempfile::tempdir().expect("Should be able to create temp directory");
    env::set_current_dir(temp_dir.path()).expect("Should be able to change directory");

    // Test that resolution still works from the temp directory
    let static_dir_from_temp = FastSkillServer::resolve_static_dir();
    assert!(
        static_dir_from_temp.exists(),
        "Static directory should be resolvable even when running from a different directory. \
         Original CWD: {}, Temp CWD: {}, Resolved path: {}. \
         This test ensures path resolution doesn't break when the binary is run from different locations.",
        original_cwd.display(),
        temp_dir.path().display(),
        static_dir_from_temp.display()
    );

    // Verify both resolutions point to the same directory
    assert_eq!(
        static_dir_original
            .canonicalize()
            .unwrap_or_else(|_| static_dir_original.clone()),
        static_dir_from_temp
            .canonicalize()
            .unwrap_or_else(|_| static_dir_from_temp.clone()),
        "Static directory resolution should be consistent regardless of working directory. \
         Original: {}, From temp: {}",
        static_dir_original.display(),
        static_dir_from_temp.display()
    );

    // Restore original working directory
    env::set_current_dir(&original_cwd).expect("Should be able to restore original directory");
}

/// Test that the resolved static directory path has the expected structure.
/// This ensures the path resolution logic is working correctly and returns
/// a path that matches the expected source code structure.
#[test]
fn test_static_directory_path_structure() {
    use fastskill::http::server::FastSkillServer;

    let static_dir = FastSkillServer::resolve_static_dir();

    // The path should end with "static"
    assert!(
        static_dir.ends_with("static"),
        "Static directory path should end with 'static': {}",
        static_dir.display()
    );

    // The parent should be "http"
    if let Some(parent) = static_dir.parent() {
        assert!(
            parent.ends_with("http"),
            "Static directory parent should be 'http': {}",
            parent.display()
        );
    }

    // Verify the path contains the expected components
    let path_str = static_dir.to_string_lossy();
    assert!(
        path_str.contains("static"),
        "Path should contain 'static': {}",
        static_dir.display()
    );
}

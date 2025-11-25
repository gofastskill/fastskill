//! Tests for registry API endpoints

// Note: These are placeholder tests - actual integration tests would require
// a running server and mock marketplace.json responses
// Full implementation would use tokio-test and http mocking

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


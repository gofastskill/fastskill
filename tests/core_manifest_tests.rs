//! Unit tests for TOML parsing, serialization, and context detection

use fastskill::core::manifest::{
    DependenciesSection, DependencySource, DependencySpec, FastSkillToolConfig, MetadataSection,
    ProjectContext, SkillProjectToml, ToolSection,
};
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn test_toml_parsing_basic() {
    let toml_content = r#"
[metadata]
id = "test-skill"
version = "1.0.0"
description = "A test skill"
"#;

    let project: SkillProjectToml = toml::from_str(toml_content).unwrap();

    assert!(project.metadata.is_some());
    let metadata = project.metadata.as_ref().unwrap();
    assert_eq!(metadata.id, Some("test-skill".to_string()));
    assert_eq!(metadata.version, Some("1.0.0".to_string()));
    assert_eq!(metadata.description, Some("A test skill".to_string()));
}

#[test]
fn test_toml_parsing_with_dependencies() {
    let toml_content = r#"
[dependencies]
web-scraper = "1.0.0"
dev-tools = { source = "git", url = "https://github.com/org/dev-tools.git" }
"#;

    let project: SkillProjectToml = toml::from_str(toml_content).unwrap();

    assert!(project.dependencies.is_some());
    let deps = project.dependencies.as_ref().unwrap();
    assert!(deps.dependencies.contains_key("web-scraper"));
    assert!(deps.dependencies.contains_key("dev-tools"));
}

#[test]
fn test_toml_serialization() {
    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("test-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: Some("A test skill".to_string()),
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    let toml_string = toml::to_string_pretty(&project).unwrap();
    assert!(toml_string.contains("test-skill"));
    assert!(toml_string.contains("1.0.0"));
    assert!(toml_string.contains("A test skill"));
}

#[test]
fn test_context_detection_skill_level() {
    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("test-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    let context = fastskill::core::project::detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Skill);
}

#[test]
fn test_context_detection_project_level() {
    let mut deps = HashMap::new();
    deps.insert(
        "web-scraper".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    );

    let project = SkillProjectToml {
        metadata: None,
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    let context = fastskill::core::project::detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Project);
}

#[test]
fn test_context_detection_ambiguous() {
    let project = SkillProjectToml {
        metadata: None,
        dependencies: None,
        tool: None,
    };

    let context = fastskill::core::project::detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Ambiguous);
}

#[test]
fn test_validation_skill_level() {
    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: Some("test-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    let result = project.validate_for_context(ProjectContext::Skill);
    assert!(result.is_ok());
}

#[test]
fn test_validation_skill_level_missing_id() {
    let project = SkillProjectToml {
        metadata: Some(MetadataSection {
            id: None,
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    let result = project.validate_for_context(ProjectContext::Skill);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("id"));
}

#[test]
fn test_validation_project_level() {
    let mut deps = HashMap::new();
    deps.insert(
        "web-scraper".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    );

    let project = SkillProjectToml {
        metadata: None,
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: Some(ToolSection {
            fastskill: Some(FastSkillToolConfig {
                skills_directory: Some(PathBuf::from(".cursor/skills")),
                embedding: None,
                repositories: None,
            }),
        }),
    };

    let result = project.validate_for_context(ProjectContext::Project);
    assert!(result.is_ok());
}

#[test]
fn test_validation_project_level_missing_dependencies() {
    let project = SkillProjectToml {
        metadata: None,
        dependencies: None,
        tool: None,
    };

    let result = project.validate_for_context(ProjectContext::Project);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("dependencies"));
}

/// T022: Test reading dependencies from skill-project.toml
#[test]
fn test_reading_dependencies_from_skill_project_toml() {
    let toml_content = r#"
[dependencies]
web-scraper = "1.0.0"
dev-tools = { source = "git", url = "https://github.com/org/dev-tools.git" }
monitoring = { source = "local", path = "./local-skills/monitoring" }
"#;

    let project: SkillProjectToml = toml::from_str(toml_content).unwrap();

    assert!(project.dependencies.is_some());
    let deps = project.dependencies.as_ref().unwrap();
    assert_eq!(deps.dependencies.len(), 3);
    assert!(deps.dependencies.contains_key("web-scraper"));
    assert!(deps.dependencies.contains_key("dev-tools"));
    assert!(deps.dependencies.contains_key("monitoring"));

    // Check version dependency
    if let DependencySpec::Version(version) = deps.dependencies.get("web-scraper").unwrap() {
        assert_eq!(version, "1.0.0");
    } else {
        panic!("Expected version dependency for web-scraper");
    }

    // Check git dependency
    if let DependencySpec::Inline { source, .. } = deps.dependencies.get("dev-tools").unwrap() {
        assert!(matches!(source, DependencySource::Git));
    } else {
        panic!("Expected inline dependency for dev-tools");
    }
}

/// T023: Test reading repositories from skill-project.toml
#[test]
fn test_reading_repositories_from_skill_project_toml() {
    let toml_content = r#"
[[tool.fastskill.repositories]]
name = "default-registry"
type = "http-registry"
priority = 1
index_url = "https://registry.example.com"

[[tool.fastskill.repositories]]
name = "git-marketplace"
type = "git-marketplace"
priority = 2
url = "https://github.com/org/skills-marketplace.git"
branch = "main"
"#;

    let project: SkillProjectToml = toml::from_str(toml_content).unwrap();

    assert!(project.tool.is_some());
    let tool = project.tool.as_ref().unwrap();
    assert!(tool.fastskill.is_some());
    let fastskill_config = tool.fastskill.as_ref().unwrap();
    assert!(fastskill_config.repositories.is_some());
    let repos = fastskill_config.repositories.as_ref().unwrap();
    assert_eq!(repos.len(), 2);

    let registry_repo = repos.iter().find(|r| r.name == "default-registry").unwrap();
    assert_eq!(registry_repo.priority, 1);
    assert!(matches!(
        registry_repo.r#type,
        fastskill::core::manifest::RepositoryType::HttpRegistry
    ));

    let git_repo = repos.iter().find(|r| r.name == "git-marketplace").unwrap();
    assert_eq!(git_repo.priority, 2);
    assert!(matches!(
        git_repo.r#type,
        fastskill::core::manifest::RepositoryType::GitMarketplace
    ));
}

/// T035: Test reading metadata from skill-project.toml
#[test]
fn test_reading_metadata_from_skill_project_toml() {
    let toml_content = r#"
[metadata]
id = "test-skill"
version = "1.0.0"
description = "A test skill"
author = "Test Author"
tags = ["test", "example"]
capabilities = ["capability1", "capability2"]
download_url = "https://example.com/skill.zip"
"#;

    let project: SkillProjectToml = toml::from_str(toml_content).unwrap();

    assert!(project.metadata.is_some());
    let metadata = project.metadata.as_ref().unwrap();
    assert_eq!(metadata.id, Some("test-skill".to_string()));
    assert_eq!(metadata.version, Some("1.0.0".to_string()));
    assert_eq!(metadata.description, Some("A test skill".to_string()));
    assert_eq!(metadata.author, Some("Test Author".to_string()));
    assert_eq!(
        metadata.download_url,
        Some("https://example.com/skill.zip".to_string())
    );
}

/// T036: Test skill-level context detection
#[test]
fn test_skill_level_context_detection() {
    use fastskill::core::project::detect_context;
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md to indicate skill-level context
    fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n").unwrap();

    // Create skill-project.toml with metadata
    let project_file = skill_dir.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[metadata]
id = "test-skill"
version = "1.0.0"
"#,
    )
    .unwrap();

    let context = detect_context(&project_file);
    assert_eq!(context, ProjectContext::Skill);
}

/// T045: Test ambiguous context detection
#[test]
fn test_ambiguous_context_detection() {
    use fastskill::core::manifest::SkillProjectToml;
    use fastskill::core::project::detect_context_from_content;

    // Empty project - should be ambiguous
    let project = SkillProjectToml {
        metadata: None,
        dependencies: None,
        tool: None,
    };

    let context = detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Ambiguous);

    // Project with empty metadata and empty dependencies - ambiguous
    let project = SkillProjectToml {
        metadata: Some(fastskill::core::manifest::MetadataSection {
            id: None,
            version: None,
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: Some(fastskill::core::manifest::DependenciesSection {
            dependencies: std::collections::HashMap::new(),
        }),
        tool: None,
    };

    let context = detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Ambiguous);
}

/// T046: Test content-based context resolution
#[test]
fn test_content_based_context_resolution() {
    use fastskill::core::manifest::{DependenciesSection, DependencySpec, SkillProjectToml};
    use fastskill::core::project::detect_context_from_content;
    use std::collections::HashMap;

    // Project with metadata.id - should resolve to Skill
    let project = SkillProjectToml {
        metadata: Some(fastskill::core::manifest::MetadataSection {
            id: Some("test-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: None,
        tool: None,
    };

    let context = detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Skill);

    // Project with dependencies - should resolve to Project
    let mut deps = HashMap::new();
    deps.insert(
        "web-scraper".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    );
    let project = SkillProjectToml {
        metadata: None,
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    let context = detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Project);

    // Project with both metadata.id and dependencies - metadata.id takes precedence (Skill)
    let mut deps = HashMap::new();
    deps.insert(
        "web-scraper".to_string(),
        DependencySpec::Version("1.0.0".to_string()),
    );
    let project = SkillProjectToml {
        metadata: Some(fastskill::core::manifest::MetadataSection {
            id: Some("test-skill".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            name: None,
        }),
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    let context = detect_context_from_content(&project);
    assert_eq!(context, ProjectContext::Skill);
}

/// T061: Test malformed TOML error messages
#[test]
fn test_malformed_toml_error_messages() {
    use fastskill::core::manifest::SkillProjectToml;
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let project_file = temp_dir.path().join("skill-project.toml");

    // Test malformed TOML - missing closing bracket
    fs::write(
        &project_file,
        r#"
[metadata
id = "test-skill"
"#,
    )
    .unwrap();

    let result = SkillProjectToml::load_from_file(&project_file);
    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_msg = format!("{}", error);
    // Error should mention line number or syntax issue
    assert!(
        error_msg.contains("line") || error_msg.contains("syntax") || error_msg.contains("parse"),
        "Error message should mention line number or syntax: {}",
        error_msg
    );

    // Test malformed TOML - invalid value type
    fs::write(
        &project_file,
        r#"
[metadata]
id = "test-skill"
version = [invalid]
"#,
    )
    .unwrap();

    let result = SkillProjectToml::load_from_file(&project_file);
    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_msg = format!("{}", error);
    assert!(
        error_msg.contains("line") || error_msg.contains("syntax") || error_msg.contains("parse"),
        "Error message should mention line number or syntax: {}",
        error_msg
    );
}

/// T062: Test missing required sections error
#[test]
fn test_missing_required_sections_error() {
    use fastskill::core::manifest::{ProjectContext, SkillProjectToml};
    use std::fs;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();

    // Create SKILL.md to indicate skill-level context
    fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n").unwrap();

    // Create skill-project.toml without required metadata.id
    let project_file = skill_dir.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[metadata]
description = "A test skill"
# Missing id and version
"#,
    )
    .unwrap();

    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let result = project.validate_for_context(ProjectContext::Skill);
    assert!(result.is_err());
    let error_msg = result.unwrap_err();
    assert!(
        error_msg.contains("id") || error_msg.contains("version"),
        "Error should mention missing id or version: {}",
        error_msg
    );
    assert!(
        error_msg.contains("SKILL.md") || error_msg.contains("skill-level"),
        "Error should mention context: {}",
        error_msg
    );

    // Test project-level missing dependencies
    let project_root = temp_dir.path();
    let project_file = project_root.join("skill-project.toml");
    fs::write(
        &project_file,
        r#"
[metadata]
name = "My Project"
# Missing dependencies section
"#,
    )
    .unwrap();

    let project = SkillProjectToml::load_from_file(&project_file).unwrap();
    let result = project.validate_for_context(ProjectContext::Project);
    assert!(result.is_err());
    let error_msg = result.unwrap_err();
    assert!(
        error_msg.contains("dependencies"),
        "Error should mention missing dependencies: {}",
        error_msg
    );
    assert!(
        error_msg.contains("project root") || error_msg.contains("project-level"),
        "Error should mention context: {}",
        error_msg
    );
}

/// T073: Verify all dependency source types work (git, local, zip-url, source)
#[test]
fn test_all_dependency_source_types() {
    use fastskill::core::manifest::{
        DependenciesSection, DependencySource, DependencySpec, SkillProjectToml,
        SourceSpecificFields,
    };
    use std::collections::HashMap;

    // Test git source
    let mut deps = HashMap::new();
    deps.insert(
        "git-skill".to_string(),
        DependencySpec::Inline {
            source: DependencySource::Git,
            source_specific: SourceSpecificFields {
                url: Some("https://github.com/org/git-skill.git".to_string()),
                branch: Some("main".to_string()),
                path: None,
                name: None,
                skill: None,
                zip_url: None,
                version: None,
            },
            groups: None,
            editable: None,
        },
    );

    // Test local source
    deps.insert(
        "local-skill".to_string(),
        DependencySpec::Inline {
            source: DependencySource::Local,
            source_specific: SourceSpecificFields {
                url: None,
                branch: None,
                path: Some("/path/to/local/skill".to_string()),
                name: None,
                skill: None,
                zip_url: None,
                version: None,
            },
            groups: None,
            editable: Some(true),
        },
    );

    // Test zip-url source
    deps.insert(
        "zip-skill".to_string(),
        DependencySpec::Inline {
            source: DependencySource::ZipUrl,
            source_specific: SourceSpecificFields {
                url: None,
                branch: None,
                path: None,
                name: None,
                skill: None,
                zip_url: Some("https://example.com/skill.zip".to_string()),
                version: Some("1.0.0".to_string()),
            },
            groups: None,
            editable: None,
        },
    );

    // Test source source
    deps.insert(
        "source-skill".to_string(),
        DependencySpec::Inline {
            source: DependencySource::Source,
            source_specific: SourceSpecificFields {
                url: None,
                branch: None,
                path: None,
                name: Some("registry-name".to_string()),
                skill: Some("skill-id".to_string()),
                zip_url: None,
                version: Some("1.0.0".to_string()),
            },
            groups: None,
            editable: None,
        },
    );

    let project = SkillProjectToml {
        metadata: None,
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    // Verify all dependencies can be converted to SkillEntry
    let entries = project.to_skill_entries().unwrap();
    assert_eq!(entries.len(), 4);

    // Verify git source
    let git_entry = entries.iter().find(|e| e.id == "git-skill").unwrap();
    assert!(matches!(
        git_entry.source,
        fastskill::core::manifest::SkillSource::Git { .. }
    ));

    // Verify local source
    let local_entry = entries.iter().find(|e| e.id == "local-skill").unwrap();
    assert!(matches!(
        local_entry.source,
        fastskill::core::manifest::SkillSource::Local { .. }
    ));
    assert!(local_entry.editable);

    // Verify zip-url source
    let zip_entry = entries.iter().find(|e| e.id == "zip-skill").unwrap();
    assert!(matches!(
        zip_entry.source,
        fastskill::core::manifest::SkillSource::ZipUrl { .. }
    ));

    // Verify source source
    let source_entry = entries.iter().find(|e| e.id == "source-skill").unwrap();
    assert!(matches!(
        source_entry.source,
        fastskill::core::manifest::SkillSource::Source { .. }
    ));
}

/// T074: Verify dependency groups and editable installs work
#[test]
fn test_dependency_groups_and_editable_installs() {
    use fastskill::core::manifest::{
        DependenciesSection, DependencySource, DependencySpec, SkillProjectToml,
        SourceSpecificFields,
    };
    use std::collections::HashMap;

    let mut deps = HashMap::new();

    // Test dependency with groups
    deps.insert(
        "dev-tool".to_string(),
        DependencySpec::Inline {
            source: DependencySource::Git,
            source_specific: SourceSpecificFields {
                url: Some("https://github.com/org/dev-tool.git".to_string()),
                branch: None,
                path: None,
                name: None,
                skill: None,
                zip_url: None,
                version: None,
            },
            groups: Some(vec!["dev".to_string(), "testing".to_string()]),
            editable: None,
        },
    );

    // Test editable dependency
    deps.insert(
        "editable-skill".to_string(),
        DependencySpec::Inline {
            source: DependencySource::Local,
            source_specific: SourceSpecificFields {
                url: None,
                branch: None,
                path: Some("/path/to/editable".to_string()),
                name: None,
                skill: None,
                zip_url: None,
                version: None,
            },
            groups: Some(vec!["dev".to_string()]),
            editable: Some(true),
        },
    );

    // Test dependency with both groups and editable
    deps.insert(
        "full-featured".to_string(),
        DependencySpec::Inline {
            source: DependencySource::Local,
            source_specific: SourceSpecificFields {
                url: None,
                branch: None,
                path: Some("/path/to/full".to_string()),
                name: None,
                skill: None,
                zip_url: None,
                version: None,
            },
            groups: Some(vec!["dev".to_string(), "optional".to_string()]),
            editable: Some(true),
        },
    );

    let project = SkillProjectToml {
        metadata: None,
        dependencies: Some(DependenciesSection { dependencies: deps }),
        tool: None,
    };

    let entries = project.to_skill_entries().unwrap();
    assert_eq!(entries.len(), 3);

    // Verify groups
    let dev_tool = entries.iter().find(|e| e.id == "dev-tool").unwrap();
    assert_eq!(dev_tool.groups.len(), 2);
    assert!(dev_tool.groups.contains(&"dev".to_string()));
    assert!(dev_tool.groups.contains(&"testing".to_string()));
    assert!(!dev_tool.editable);

    // Verify editable
    let editable_skill = entries.iter().find(|e| e.id == "editable-skill").unwrap();
    assert!(editable_skill.editable);
    assert_eq!(editable_skill.groups.len(), 1);
    assert!(editable_skill.groups.contains(&"dev".to_string()));

    // Verify both groups and editable
    let full_featured = entries.iter().find(|e| e.id == "full-featured").unwrap();
    assert!(full_featured.editable);
    assert_eq!(full_featured.groups.len(), 2);
    assert!(full_featured.groups.contains(&"dev".to_string()));
    assert!(full_featured.groups.contains(&"optional".to_string()));
}

/// T063: Test repository priority conflict resolution (first occurrence wins)
#[test]
fn test_repository_priority_conflict_resolution() {
    use fastskill::core::manifest::{RepositoryConnection, RepositoryDefinition, SkillProjectToml};

    // Create skill-project.toml with duplicate repository names
    let toml_content = r#"
[[tool.fastskill.repositories]]
name = "duplicate-repo"
type = "http-registry"
priority = 1
index_url = "https://first.example.com"

[[tool.fastskill.repositories]]
name = "duplicate-repo"
type = "http-registry"
priority = 2
index_url = "https://second.example.com"

[[tool.fastskill.repositories]]
name = "unique-repo"
type = "http-registry"
priority = 3
index_url = "https://unique.example.com"
"#;

    let project: SkillProjectToml = toml::from_str(toml_content).unwrap();

    assert!(project.tool.is_some());
    let tool = project.tool.as_ref().unwrap();
    assert!(tool.fastskill.is_some());
    let fastskill_config = tool.fastskill.as_ref().unwrap();
    assert!(fastskill_config.repositories.is_some());

    let repos = fastskill_config.repositories.as_ref().unwrap();
    // All repositories should be present in the raw TOML
    assert_eq!(repos.len(), 3);

    // T068: When converting to a map (e.g., for RepositoryManager),
    // first occurrence should win
    use std::collections::HashMap;
    let mut repo_map: HashMap<String, &RepositoryDefinition> = HashMap::new();
    for repo in repos.iter() {
        // First occurrence wins - only insert if not already present
        repo_map.entry(repo.name.clone()).or_insert(repo);
    }

    // Should only have 2 repositories (duplicate-repo and unique-repo)
    assert_eq!(repo_map.len(), 2);

    // First occurrence should be kept (priority 1, index_url "https://first.example.com")
    let first_repo = repo_map.get("duplicate-repo").unwrap();
    assert_eq!(first_repo.priority, 1);
    if let RepositoryConnection::HttpRegistry { index_url } = &first_repo.connection {
        assert_eq!(index_url, "https://first.example.com");
    } else {
        panic!("Expected HttpRegistry connection");
    }

    // Unique repo should be present
    assert!(repo_map.contains_key("unique-repo"));
}

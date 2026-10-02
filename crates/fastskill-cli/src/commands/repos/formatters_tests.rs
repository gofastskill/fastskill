use super::*;
use fastskill_core::core::registry_index::SkillSummary;
use fastskill_core::core::repository::{RepositoryAuth, StorageConfig};
use std::path::PathBuf;

fn repo(
    name: &str,
    config: RepositoryConfig,
    auth: Option<RepositoryAuth>,
) -> RepositoryDefinition {
    let repo_type = match &config {
        RepositoryConfig::GitMarketplace { .. } => RepositoryType::GitMarketplace,
        RepositoryConfig::HttpRegistry { .. } => RepositoryType::HttpRegistry,
        RepositoryConfig::ZipUrl { .. } => RepositoryType::ZipUrl,
        RepositoryConfig::Local { .. } => RepositoryType::Local,
    };
    RepositoryDefinition {
        name: name.to_string(),
        repo_type,
        priority: 1,
        config,
        auth,
        storage: None,
    }
}

fn every_kind() -> Vec<RepositoryDefinition> {
    vec![
        repo(
            "git",
            RepositoryConfig::GitMarketplace {
                url: "https://git.example/r.git".to_string(),
                branch: Some("main".to_string()),
                tag: Some("v1".to_string()),
            },
            Some(RepositoryAuth::Pat {
                env_var: "GIT_TOKEN".to_string(),
            }),
        ),
        repo(
            "registry",
            RepositoryConfig::HttpRegistry {
                index_url: "https://registry.example/index".to_string(),
            },
            Some(RepositoryAuth::Command {
                command: vec!["my-login".to_string(), "secret-arg".to_string()],
            }),
        ),
        repo(
            "zip",
            RepositoryConfig::ZipUrl {
                base_url: "https://zip.example/".to_string(),
            },
            None,
        ),
        repo(
            "local",
            RepositoryConfig::Local {
                path: PathBuf::from("/skills"),
            },
            None,
        ),
    ]
}

#[test]
fn details_show_every_config_kind_and_the_auth_type_only() {
    let mut repos = every_kind();
    repos[2].storage = Some(StorageConfig {
        storage_type: "s3".to_string(),
        repository: None,
        bucket: Some("b".to_string()),
        region: None,
        endpoint: None,
        base_url: None,
    });
    let text: String = repos.iter().map(format_repository_details).collect();
    for expected in [
        "Branch: main",
        "Tag: v1",
        "Index URL: https://registry.example/index",
        "Base URL: https://zip.example/",
        "Path: /skills",
        "Storage:",
        "Auth: pat",
        "Auth: command",
    ] {
        assert!(text.contains(expected), "{expected} missing from {text}");
    }
    assert!(!text.contains("secret-arg"), "{text}");

    let grid: String = repos.iter().map(format_repository_details_grid).collect();
    for expected in [
        "branch=main",
        "tag=v1",
        "index_url=https://registry.example/index",
        "base_url=https://zip.example/",
        "path=/skills",
        "auth=command",
    ] {
        assert!(grid.contains(expected), "{expected} missing from {grid}");
    }

    let xml: String = repos.iter().map(format_repository_details_xml).collect();
    for expected in [
        "<branch>main</branch>",
        "<tag>v1</tag>",
        "<config kind=\"zip-url\">",
        "<base_url>https://zip.example/</base_url>",
        "<config kind=\"local\">",
        "<path>/skills</path>",
        "<auth type=\"command\" />",
    ] {
        assert!(xml.contains(expected), "{expected} missing from {xml}");
    }
    assert!(!grid.contains("secret-arg") && !xml.contains("secret-arg"));
}

#[test]
fn lists_show_the_auth_type_in_every_format() {
    let repos = every_kind();
    let refs: Vec<&RepositoryDefinition> = repos.iter().collect();
    let text = format_repository_list(&refs);
    assert!(text.contains("type: zip-url"), "{text}");
    assert!(text.contains("type: local"), "{text}");
    assert!(text.contains("auth: command"), "{text}");

    let xml = format_repository_list_xml(&refs);
    assert!(
        xml.contains("<repository name=\"registry\" type=\"http-registry\" priority=\"1\" auth=\"command\" />"),
        "{xml}"
    );
    assert!(xml.contains("<repository name=\"zip\" type=\"zip-url\" priority=\"1\" />"));
    assert!(!xml.contains("secret-arg"));
}

fn summary(description: &str, published: bool) -> SkillSummary {
    SkillSummary {
        id: "scope/name".to_string(),
        scope: "scope".to_string(),
        name: "name".to_string(),
        description: description.to_string(),
        latest_version: "1.0.0".to_string(),
        published_at: published.then(chrono::Utc::now),
        versions: None,
    }
}

#[test]
fn skill_summaries_render_in_every_format() {
    let summaries = vec![summary(&"long ".repeat(20), true), summary("short", false)];
    assert!(format_table_output(&summaries, false).is_ok());
    assert!(format_table_output(&summaries, true).is_ok());
    assert!(format_grid_output(&summaries, false).is_ok());
    assert!(format_grid_output(&[], false).is_ok());
    assert!(format_xml_output(&summaries).is_ok());

    let xml = summaries_to_xml(&summaries);
    assert_eq!(xml.matches("<published>").count(), 1, "{xml}");
    assert!(xml.contains("<description>short</description>"));
}

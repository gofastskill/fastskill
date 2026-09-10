//! Unit tests for [`super`] (`SourcesManager`).
//!
//! Split out of `manager.rs` to keep that file under the 1000-line
//! source-size gate (`scripts/check-source-size.sh`), which excludes
//! `tests.rs` by name. Same pattern as `core/cache/tests.rs`.

use super::*;
use crate::core::cache::{SkillCache, SourceIndex, SourceIndexEntry};
use crate::core::repository::{
    RepositoryAuth, RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pat_auth() -> Option<SourceAuth> {
    Some(SourceAuth::Pat {
        env_var: "EXAMPLE_TOKEN".to_string(),
    })
}

fn zip_source_with_auth() -> SourceConfig {
    SourceConfig::ZipUrl {
        base_url: "http://127.0.0.1:1/skills.zip".to_string(),
        auth: pat_auth(),
    }
}

fn zip_source_without_auth() -> SourceConfig {
    SourceConfig::ZipUrl {
        base_url: "http://127.0.0.1:1/skills.zip".to_string(),
        auth: None,
    }
}

#[test]
fn reject_configured_zip_url_auth_rejects_when_configured() {
    let err = reject_configured_zip_url_auth("my-zip-source", &pat_auth())
        .expect_err("configured auth on a zip-url source must be rejected");
    assert_eq!(
        err.to_string(),
        "Zip URL error: Source 'my-zip-source' has `auth` configured, but zip-url sources \
         fetch via a plain HTTP GET and do not support an `auth` block -- fastskill does \
         not inject PAT/basic credentials into zip-url requests. Remove `auth` from this \
         source and use a pre-signed URL instead (e.g. an S3 or GCS presigned URL), which \
         embeds the credential in the URL itself and needs no separate `auth` configuration."
    );
}

#[test]
fn reject_configured_zip_url_auth_allows_when_absent() {
    assert!(reject_configured_zip_url_auth("my-zip-source", &None).is_ok());
}

/// Regression guard: this fix (the zip-url half of #273's git-auth fix)
/// must not change #273's git rejection message by even one character --
/// callers and tests elsewhere may assert on its exact text.
#[test]
fn reject_configured_git_auth_message_is_unchanged() {
    let err = reject_configured_git_auth("my-git-source", &pat_auth())
        .expect_err("configured auth on a git source must be rejected");
    assert_eq!(
        err.to_string(),
        "Git error: Source 'my-git-source' has `auth` configured, but git sources \
         authenticate via the system git credential helper or SSH agent, not via an \
         `auth` block -- fastskill does not inject PAT/basic credentials into git \
         operations. Remove `auth` from this source and either: (1) configure a git \
         credential helper (e.g. `git config credential.helper store`, or `gh auth \
         login`), or (2) use an SSH remote (e.g. `git@github.com:org/repo.git`) with a \
         key loaded in your SSH agent."
    );
}

#[test]
fn reject_configured_git_auth_allows_when_absent() {
    assert!(reject_configured_git_auth("my-git-source", &None).is_ok());
}

/// Call site 1: `get_skills_from_source`. Configured `auth` on a
/// zip-url source must be rejected before any network call is made --
/// the loopback base_url would otherwise fail with a connection-refused
/// `Network` error, not a `ZipUrl` one, so reaching the network at all
/// here would itself be a test failure.
#[tokio::test]
async fn get_skills_from_source_rejects_zip_url_auth() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_with_auth())
        .unwrap();
    let source_def = manager.get_source("zip-src").unwrap().clone();

    let err = manager
        .get_skills_from_source("zip-src", &source_def)
        .await
        .expect_err("configured auth must be rejected before any network call");
    assert!(matches!(err, SourcesError::ZipUrl(_)));
    assert!(err.to_string().contains("pre-signed URL"));
}

/// Call site 2: `get_marketplace_json`. Same guarantee as above, for
/// the other function that destructures `SourceConfig::ZipUrl` -- the
/// exact failure mode of the original bug was fixing only one of these.
#[tokio::test]
async fn get_marketplace_json_rejects_zip_url_auth() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_with_auth())
        .unwrap();

    let err = manager
        .get_marketplace_json("zip-src")
        .await
        .expect_err("configured auth must be rejected before any network call");
    assert!(matches!(err, SourcesError::ZipUrl(_)));
    assert!(err.to_string().contains("pre-signed URL"));
}

/// A zip-url source with no `auth` configured must proceed unaffected:
/// it should reach the network stage (and fail there, against a
/// deliberately unreachable loopback port) rather than being rejected
/// by the new auth gate.
#[tokio::test]
async fn get_skills_from_source_zip_url_without_auth_is_unaffected() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_without_auth())
        .unwrap();
    let source_def = manager.get_source("zip-src").unwrap().clone();

    let err = manager
        .get_skills_from_source("zip-src", &source_def)
        .await
        .expect_err("unreachable loopback URL should fail at the network stage");
    assert!(
        !matches!(err, SourcesError::ZipUrl(_)),
        "unexpected auth rejection for a source with no `auth` configured: {err}"
    );
}

/// Same as above for the `get_marketplace_json` call site.
#[tokio::test]
async fn get_marketplace_json_zip_url_without_auth_is_unaffected() {
    let mut manager = SourcesManager::new(PathBuf::from("/tmp/does-not-matter.toml"));
    manager
        .add_source("zip-src".to_string(), zip_source_without_auth())
        .unwrap();

    let err = manager
        .get_marketplace_json("zip-src")
        .await
        .expect_err("unreachable loopback URL should fail at the network stage");
    assert!(
        !matches!(err, SourcesError::ZipUrl(_)),
        "unexpected auth rejection for a source with no `auth` configured: {err}"
    );
}

fn local_source(path: &std::path::Path) -> SourceConfig {
    SourceConfig::Local {
        path: path.to_path_buf(),
    }
}

fn marketplace_body(skill_path: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "fixture",
        "owner": {"name": "Fixture Owner"},
        "metadata": {"description": "metadata fallback", "version": "2.1.0"},
        "plugins": [{
            "name": "fixture-plugin",
            "description": "Fixture plugin",
            "source": "./plugins/fixture",
            "skills": [skill_path]
        }]
    })
}

#[tokio::test]
async fn source_management_round_trips_sorted_configuration_and_local_skills() {
    let temp = TempDir::new().unwrap();
    let skills_root = temp.path().join("skills");
    let skill_dir = skills_root.join("demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Demo\ndescription: Local fixture\nversion: 1.2.3\n---\n# Demo\n",
    )
    .unwrap();

    let config_path = temp.path().join("config/sources.toml");
    let mut manager = SourcesManager::new(config_path.clone());
    manager.load().unwrap();
    manager
        .add_source_with_priority("later".to_string(), local_source(&skills_root), 20)
        .unwrap();
    manager
        .add_source_with_priority("first".to_string(), local_source(&skills_root), 5)
        .unwrap();
    let duplicate = manager
        .add_source("first".to_string(), local_source(&skills_root))
        .unwrap_err();
    assert!(matches!(duplicate, SourcesError::AlreadyExists(_)));
    assert_eq!(manager.list_sources()[0].name, "first");
    assert!(manager.get_source("later").is_some());

    let skills = manager.get_available_skills().await.unwrap();
    assert_eq!(skills.len(), 2);
    assert!(skills.iter().all(|skill| skill.name == "Demo"));
    assert!(skills
        .iter()
        .all(|skill| skill.source_name == "first" || skill.source_name == "later"));

    manager.save().unwrap();
    let mut loaded = SourcesManager::new(config_path);
    loaded.load().unwrap();
    assert_eq!(loaded.list_sources()[0].name, "first");
    loaded.remove_source("first").unwrap();
    assert!(matches!(
        loaded.remove_source("first"),
        Err(SourcesError::SourceNotFound(_))
    ));
    loaded.clear_cache().await;
}

#[tokio::test]
async fn claude_conversion_resolves_paths_descriptions_versions_and_urls() {
    let manager = SourcesManager::new(PathBuf::from("unused"));
    let claude: ClaudeCodeMarketplaceJson = serde_json::from_value(serde_json::json!({
        "name": "fixture",
        "owner": {"name": "Owner"},
        "metadata": {"description": "Metadata description", "version": "3.2.1"},
        "plugins": [
            {
                "name": "described",
                "description": "Plugin description",
                "source": "./plugins/pack/",
                "skills": ["./skills/one", "/skills/two", "skills/three/"]
            },
            {
                "name": "fallback",
                "skills": ["plain"]
            }
        ]
    }))
    .unwrap();

    let converted = manager
        .convert_claude_to_fastskill_format(
            claude,
            "https://github.com/acme/skills.git".to_string(),
            "source",
        )
        .await
        .unwrap();
    assert_eq!(
        converted
            .skills
            .iter()
            .map(|skill| skill.id.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two", "three", "plain"]
    );
    assert_eq!(converted.skills[0].description, "Plugin description");
    assert_eq!(converted.skills[3].description, "Metadata description");
    assert_eq!(converted.skills[0].version, "3.2.1");
    assert_eq!(converted.skills[0].author.as_deref(), Some("Owner"));
    assert_eq!(
        converted.skills[0].download_url.as_deref(),
        Some("https://github.com/acme/skills/tree/main/./plugins/pack/skills/one")
    );

    let minimal: ClaudeCodeMarketplaceJson = serde_json::from_value(serde_json::json!({
        "name": "fixture",
        "plugins": [{"name": "bare", "skills": ["skill-a"]}]
    }))
    .unwrap();
    let without_base = manager
        .convert_claude_to_fastskill_format(minimal.clone(), String::new(), "source")
        .await
        .unwrap();
    assert_eq!(without_base.skills[0].description, "Skill from bare");
    assert_eq!(without_base.skills[0].version, "1.0.0");
    assert!(without_base.skills[0].download_url.is_none());

    let hosted = manager
        .convert_claude_to_fastskill_format(
            minimal,
            "https://skills.example.test/base/".to_string(),
            "source",
        )
        .await
        .unwrap();
    assert_eq!(
        hosted.skills[0].download_url.as_deref(),
        Some("https://skills.example.test/base/./skill-a")
    );
}

#[test]
fn raw_url_conversion_handles_github_and_plain_hosts() {
    assert_eq!(
        SourcesManager::to_github_raw_url(
            "https://github.com/acme/skills.git",
            "release",
            "marketplace.json"
        ),
        "https://raw.githubusercontent.com/acme/skills/release/marketplace.json"
    );
    assert_eq!(
        SourcesManager::to_github_raw_url(
            "https://raw.githubusercontent.com/acme/skills/main",
            "ignored",
            "marketplace.json"
        ),
        "https://raw.githubusercontent.com/acme/skills/main/marketplace.json"
    );
    assert_eq!(
        SourcesManager::to_github_raw_url("https://example.test/base/", "", "marketplace.json"),
        "https://example.test/base/marketplace.json"
    );
}

#[tokio::test]
async fn marketplace_fetch_reports_http_parse_and_validation_failures() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/failure"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/malformed"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/invalid-skill"))
        .respond_with(ResponseTemplate::new(200).set_body_json(marketplace_body("/")))
        .mount(&server)
        .await;

    let manager = SourcesManager::new(PathBuf::from("unused"));
    let err = manager
        .try_fetch_marketplace(&format!("{}/failure", server.uri()), None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("HTTP 503"));

    let err = manager
        .try_fetch_marketplace(&format!("{}/malformed", server.uri()), None)
        .await
        .unwrap_err();
    assert!(matches!(err, SourcesError::Parse(_)));

    let err = manager
        .try_fetch_marketplace(&format!("{}/invalid-skill", server.uri()), None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("skills must have id"));
}

#[tokio::test]
async fn root_fallback_is_cached_and_clear_cache_forces_a_refetch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.claude-plugin/marketplace.json"))
        .respond_with(ResponseTemplate::new(404))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/marketplace.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(marketplace_body("./skill-a")))
        .expect(2)
        .mount(&server)
        .await;

    let mut manager = SourcesManager::with_cache_ttl(PathBuf::from("unused"), 60);
    manager
        .add_source(
            "remote".to_string(),
            SourceConfig::ZipUrl {
                base_url: server.uri(),
                auth: None,
            },
        )
        .unwrap();

    let first = manager.get_marketplace_json("remote").await.unwrap();
    let cached = manager.get_marketplace_json("remote").await.unwrap();
    assert_eq!(first.skills[0].id, "skill-a");
    assert_eq!(cached.skills[0].id, "skill-a");

    manager.clear_cache().await;
    manager.get_marketplace_json("remote").await.unwrap();
}

#[tokio::test]
async fn marketplace_lookup_rejects_unknown_and_local_sources() {
    let temp = TempDir::new().unwrap();
    let mut manager = SourcesManager::new(temp.path().join("sources.toml"));
    let err = manager.get_marketplace_json("missing").await.unwrap_err();
    assert!(matches!(err, SourcesError::SourceNotFound(_)));

    manager
        .add_source("local".to_string(), local_source(temp.path()))
        .unwrap();
    let err = manager.get_marketplace_json("local").await.unwrap_err();
    assert!(err
        .to_string()
        .contains("Local sources do not support marketplace.json"));
}

#[test]
fn repository_conversion_keeps_supported_types_auth_and_priority() {
    fn definition(
        name: &str,
        repo_type: RepositoryType,
        config: RepositoryConfig,
        priority: u32,
        auth: Option<RepositoryAuth>,
    ) -> RepositoryDefinition {
        RepositoryDefinition {
            name: name.to_string(),
            repo_type,
            priority,
            config,
            auth,
            storage: None,
        }
    }

    let repositories = RepositoryManager::from_definitions(vec![
        definition(
            "git",
            RepositoryType::GitMarketplace,
            RepositoryConfig::GitMarketplace {
                url: "https://github.com/acme/skills".to_string(),
                branch: Some("release".to_string()),
                tag: None,
            },
            3,
            Some(RepositoryAuth::Pat {
                env_var: "TOKEN".to_string(),
            }),
        ),
        definition(
            "zip",
            RepositoryType::ZipUrl,
            RepositoryConfig::ZipUrl {
                base_url: "https://example.test/skills".to_string(),
            },
            2,
            Some(RepositoryAuth::Pat {
                env_var: "ZIP_TOKEN".to_string(),
            }),
        ),
        definition(
            "local",
            RepositoryType::Local,
            RepositoryConfig::Local {
                path: PathBuf::from("local"),
            },
            1,
            None,
        ),
        definition(
            "registry",
            RepositoryType::HttpRegistry,
            RepositoryConfig::HttpRegistry {
                index_url: "https://example.test/index".to_string(),
            },
            0,
            None,
        ),
        definition(
            "mismatched",
            RepositoryType::GitMarketplace,
            RepositoryConfig::ZipUrl {
                base_url: "https://example.test/mismatch".to_string(),
            },
            4,
            None,
        ),
    ]);

    let sources = SourcesManager::from_repositories(&repositories)
        .unwrap()
        .unwrap();
    assert_eq!(
        sources
            .list_sources()
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>(),
        vec!["local", "zip", "git"]
    );
    assert!(matches!(
        &sources.get_source("git").unwrap().source,
        SourceConfig::Git {
            branch: Some(branch),
            auth: Some(SourceAuth::Pat { env_var }),
            ..
        } if branch == "release" && env_var == "TOKEN"
    ));
    assert!(matches!(
        &sources.get_source("zip").unwrap().source,
        SourceConfig::ZipUrl {
            auth: Some(SourceAuth::Pat { env_var }),
            ..
        } if env_var == "ZIP_TOKEN"
    ));

    let unsupported = RepositoryManager::from_definitions(vec![definition(
        "registry",
        RepositoryType::HttpRegistry,
        RepositoryConfig::HttpRegistry {
            index_url: "https://example.test/index".to_string(),
        },
        0,
        None,
    )]);
    assert!(SourcesManager::from_repositories(&unsupported)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn index_conversion_deduplicates_versions_and_empty_disk_indexes_are_ignored() {
    let marketplace = MarketplaceJson {
        version: "1.0".to_string(),
        skills: vec![
            MarketplaceSkill {
                id: "demo".to_string(),
                name: "Demo".to_string(),
                description: "first".to_string(),
                version: "2.0.0".to_string(),
                author: Some("Owner".to_string()),
                download_url: Some("https://example.test/demo".to_string()),
            },
            MarketplaceSkill {
                id: "demo".to_string(),
                name: "Ignored later name".to_string(),
                description: "ignored later description".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                download_url: None,
            },
            MarketplaceSkill {
                id: "demo".to_string(),
                name: "Demo".to_string(),
                description: "first".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                download_url: None,
            },
        ],
    };
    let index = marketplace_to_source_index(&marketplace);
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].versions, vec!["1.0.0", "2.0.0"]);
    let restored = source_index_to_marketplace(&index);
    assert_eq!(restored.skills.len(), 2);
    assert!(restored
        .skills
        .iter()
        .all(|skill| skill.author.is_none() && skill.download_url.is_none()));

    let cache_root = TempDir::new().unwrap();
    let cache = SkillCache::at_root(cache_root.path());
    cache
        .write_source_index(
            "empty",
            &SourceIndex {
                fetched_at: Utc::now(),
                entries: Vec::<SourceIndexEntry>::new(),
            },
        )
        .unwrap();
    let manager = SourcesManager::new(PathBuf::from("unused")).with_skill_cache(cache);
    assert!(manager.disk_index_marketplace("missing").await.is_none());
    assert!(manager.disk_index_marketplace("empty").await.is_none());
}

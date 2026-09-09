use super::*;
use fastskill_core::core::manifest::{
    AuthConfig, AuthType, RepositoryConnection, RepositoryDefinition as ManifestRepository,
    RepositoryType as ManifestRepositoryType,
};
use fastskill_core::core::repository::{RepositoryAuth, RepositoryConfig, RepositoryType};
use std::ffi::OsString;
use std::fs;
use std::sync::Mutex;

static CONFIG_PROCESS_STATE: Mutex<()> = Mutex::new(());

struct CurrentDirectoryGuard(PathBuf);

impl CurrentDirectoryGuard {
    fn enter(path: &std::path::Path) -> Self {
        let original = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self(original)
    }
}

impl Drop for CurrentDirectoryGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.0);
    }
}

struct EnvironmentGuard {
    name: &'static str,
    original: Option<OsString>,
}

impl EnvironmentGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = env::var_os(name);
        env::set_var(name, value);
        Self { name, original }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => env::set_var(self.name, value),
            None => env::remove_var(self.name),
        }
    }
}

fn manifest_repository(
    repo_type: ManifestRepositoryType,
    connection: RepositoryConnection,
    auth: Option<AuthConfig>,
) -> ManifestRepository {
    ManifestRepository {
        name: "primary".to_string(),
        r#type: repo_type,
        priority: 7,
        connection,
        auth,
    }
}

#[test]
fn converts_every_repository_connection_without_losing_metadata() {
    let http = convert_repository_definition(manifest_repository(
        ManifestRepositoryType::HttpRegistry,
        RepositoryConnection::HttpRegistry {
            index_url: "https://registry.example/index.json".to_string(),
        },
        Some(AuthConfig {
            r#type: AuthType::Pat,
            env_var: Some("REGISTRY_TOKEN".to_string()),
        }),
    ));
    assert_eq!(http.name, "primary");
    assert_eq!(http.priority, 7);
    assert_eq!(http.repo_type, RepositoryType::HttpRegistry);
    assert!(matches!(
        http.config,
        RepositoryConfig::HttpRegistry { ref index_url }
            if index_url == "https://registry.example/index.json"
    ));
    assert!(matches!(
        http.auth,
        Some(RepositoryAuth::Pat { ref env_var }) if env_var == "REGISTRY_TOKEN"
    ));
    assert!(http.storage.is_none());

    let git = convert_repository_definition(manifest_repository(
        ManifestRepositoryType::GitMarketplace,
        RepositoryConnection::GitMarketplace {
            url: "https://github.com/example/skills".to_string(),
            branch: Some("stable".to_string()),
        },
        None,
    ));
    assert_eq!(git.repo_type, RepositoryType::GitMarketplace);
    assert!(matches!(
        git.config,
        RepositoryConfig::GitMarketplace {
            ref url,
            branch: Some(ref branch),
            tag: None,
        } if url == "https://github.com/example/skills" && branch == "stable"
    ));

    let zip = convert_repository_definition(manifest_repository(
        ManifestRepositoryType::ZipUrl,
        RepositoryConnection::ZipUrl {
            zip_url: "https://cdn.example/skills.zip".to_string(),
        },
        Some(AuthConfig {
            r#type: AuthType::Pat,
            env_var: None,
        }),
    ));
    assert_eq!(zip.repo_type, RepositoryType::ZipUrl);
    assert!(matches!(
        zip.config,
        RepositoryConfig::ZipUrl { ref base_url }
            if base_url == "https://cdn.example/skills.zip"
    ));
    assert!(matches!(
        zip.auth,
        Some(RepositoryAuth::Pat { ref env_var }) if env_var == "PAT_TOKEN"
    ));

    let local = convert_repository_definition(manifest_repository(
        ManifestRepositoryType::Local,
        RepositoryConnection::Local {
            path: "../shared-skills".to_string(),
        },
        None,
    ));
    assert_eq!(local.repo_type, RepositoryType::Local);
    assert!(matches!(
        local.config,
        RepositoryConfig::Local { ref path } if path == &PathBuf::from("../shared-skills")
    ));
}

#[test]
fn origin_validation_accepts_http_origins_and_rejects_incomplete_values() {
    assert!(is_valid_origin("http://localhost:8080"));
    assert!(is_valid_origin("https://fastskill.dev"));
    assert!(!is_valid_origin(""));
    assert!(!is_valid_origin("   "));
    assert!(!is_valid_origin("https://"));
    assert!(!is_valid_origin("ftp://fastskill.dev"));
}

#[test]
fn project_loaders_share_manifest_resolution_and_validate_server_origins() {
    let _process_state = CONFIG_PROCESS_STATE.lock().unwrap();
    let root = tempfile::TempDir::new().unwrap();
    let _cwd = CurrentDirectoryGuard::enter(root.path());

    assert!(load_repositories_from_project().unwrap().is_empty());
    assert!(load_server_config().unwrap().is_none());

    fs::write(
        root.path().join("skill-project.toml"),
        "[tool.fastskill]\n\
         skills_directory = \"managed-skills\"\n\
         [tool.fastskill.embedding]\n\
         openai_base_url = \"https://api.example/v1\"\n\
         embedding_model = \"example-embedding\"\n\
         index_path = \"custom-index\"\n\
         [tool.fastskill.server]\n\
         allowed_origins = [\"https://fastskill.dev\", \"invalid\"]\n\
         allowed_headers = [\"Content-Type\", \"X-FastSkill\"]\n\
         [[tool.fastskill.repositories]]\n\
         name = \"local\"\n\
         type = \"local\"\n\
         priority = 2\n\
         path = \"catalog\"\n",
    )
    .unwrap();

    let repositories = load_repositories_from_project().unwrap();
    assert_eq!(repositories.len(), 1);
    assert_eq!(repositories[0].repo_type, RepositoryType::Local);

    let expected_skills = root.path().join("managed-skills");
    assert_eq!(
        resolve_skills_storage_directory(false).unwrap(),
        expected_skills
    );
    assert_eq!(
        get_skill_search_locations_for_display(false).unwrap(),
        vec![(expected_skills.clone(), "project".to_string())]
    );

    let server = load_server_config().unwrap().unwrap();
    assert_eq!(server.allowed_origins, vec!["https://fastskill.dev"]);
    assert_eq!(
        server.allowed_headers,
        vec!["Content-Type".to_string(), "X-FastSkill".to_string()]
    );

    let config = create_service_config(false, None).unwrap();
    assert_eq!(config.skill_storage_path, expected_skills);
    let embedding = config.embedding.unwrap();
    assert_eq!(embedding.openai_base_url, "https://api.example/v1");
    assert_eq!(embedding.embedding_model, "example-embedding");
    assert_eq!(embedding.index_path, Some(PathBuf::from("custom-index")));
    assert_eq!(
        config.http_server.unwrap().allowed_origins,
        vec!["https://fastskill.dev"]
    );

    let missing = root.path().join("missing");
    assert!(create_service_config(false, Some(missing))
        .unwrap_err()
        .to_string()
        .contains("does not exist"));

    let explicit = root.path().join("explicit");
    fs::create_dir(&explicit).unwrap();
    assert_eq!(
        create_service_config(true, Some(explicit.clone()))
            .unwrap()
            .skill_storage_path,
        explicit
    );

    fs::write(root.path().join("skill-project.toml"), "invalid = [").unwrap();
    assert!(load_repositories_from_project()
        .unwrap_err()
        .to_string()
        .contains("Failed to load skill-project.toml"));
    assert!(load_server_config()
        .unwrap_err()
        .to_string()
        .contains("Failed to load skill-project.toml"));
}

#[test]
fn global_search_and_storage_use_the_lock_configuration_root() {
    let _process_state = CONFIG_PROCESS_STATE.lock().unwrap();
    let root = tempfile::TempDir::new().unwrap();
    let _xdg = EnvironmentGuard::set("XDG_CONFIG_HOME", root.path());
    let expected = root.path().join("fastskill/skills");

    assert_eq!(resolve_skills_storage_directory(true).unwrap(), expected);
    assert_eq!(
        get_skill_search_locations_for_display(true).unwrap(),
        vec![(expected, "global".to_string())]
    );
}

#[tokio::test]
async fn edge_injection_builds_the_configured_embedding_provider() {
    let root = tempfile::TempDir::new().unwrap();
    let skills = root.path().join("skills");
    fs::create_dir(&skills).unwrap();
    let service = FastSkillService::new(ServiceConfig {
        skill_storage_path: skills,
        skill_cache_root: Some(root.path().join("cache")),
        embedding: Some(fastskill_core::EmbeddingConfig {
            openai_base_url: "https://api.example/v1".to_string(),
            embedding_model: "example-embedding".to_string(),
            index_path: None,
        }),
        ..ServiceConfig::default()
    })
    .await
    .unwrap();
    let _process_state = CONFIG_PROCESS_STATE.lock().unwrap();
    let _cwd = CurrentDirectoryGuard::enter(root.path());
    let _api_key = EnvironmentGuard::set("OPENAI_API_KEY", "test-key");

    let service = inject_edge_services(service).unwrap();
    assert!(service.embedding_service().is_some());
}

//! An http-registry names its packages `scope/name`, while a skill's own files usually declare
//! only `name` (ADR-0014: identity comes from content). Adding `scope/name` from such a registry
//! installs the content as `name`; content naming another skill is still refused.
//!
//! The registry is a loopback `wiremock` server, as in `registry_content_cache_test.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::core::registry::client::IndexEntry;
use fastskill_core::core::repository::{
    RepositoryConfig, RepositoryDefinition, RepositoryManager, RepositoryType,
};
use fastskill_core::core::version::VersionConstraint;
use fastskill_core::core::Origin;
use fastskill_core::{FastSkillService, ServiceConfig};
use sha2::Digest;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPO_NAME: &str = "hub";

fn build_zip(name: &str) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file(format!("{name}/SKILL.md"), opts).unwrap();
        let skill_md = format!(
            "---\nname: {name}\nversion: \"1.0.0\"\ndescription: A registry skill\n---\nBody\n"
        );
        writer.write_all(skill_md.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buf
}

/// Publish `scoped` (`scope/name`) at 1.0.0 on `server`, its archive declaring `content_name`.
async fn publish(server: &MockServer, scoped: &str, content_name: &str) {
    let zip_bytes = build_zip(content_name);
    let entry = IndexEntry {
        name: scoped.to_string(),
        vers: "1.0.0".to_string(),
        deps: vec![],
        cksum: format!(
            "sha256:{}",
            fastskill_core::utils::to_hex_lower(&sha2::Sha256::digest(&zip_bytes))
        ),
        features: std::collections::HashMap::new(),
        yanked: false,
        links: None,
        download_url: format!("{}/dl/{scoped}", server.uri()),
        metadata: None,
    };
    Mock::given(method("GET"))
        .and(path(format!("/index/{scoped}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(serde_json::to_string(&entry).unwrap()),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/dl/{scoped}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_bytes))
        .mount(server)
        .await;
}

async fn service(server: &MockServer, root: &TempDir) -> FastSkillService {
    std::fs::write(
        root.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let manager = RepositoryManager::from_definitions(vec![RepositoryDefinition {
        name: REPO_NAME.to_string(),
        repo_type: RepositoryType::HttpRegistry,
        priority: 0,
        config: RepositoryConfig::HttpRegistry {
            index_url: format!("{}/index", server.uri()),
        },
        auth: None,
        storage: None,
    }]);
    let mut service = FastSkillService::new(ServiceConfig {
        skill_storage_path: root.path().join("storage"),
        skill_cache_root: Some(root.path().join("cache")),
        ..Default::default()
    })
    .await
    .unwrap();
    service.initialize().await.unwrap();
    service
        .with_project_root(root.path().to_path_buf())
        .with_repository_manager(Arc::new(manager))
}

fn origin(skill: &str) -> Origin {
    Origin::Repository {
        repo: REPO_NAME.to_string(),
        skill: skill.to_string(),
        version: Some(VersionConstraint::parse("1.0.0").unwrap()),
    }
}

#[tokio::test]
async fn a_scoped_registry_reference_installs_content_that_declares_the_bare_name() {
    let server = MockServer::start().await;
    publish(&server, "gofastskill/fastskill", "fastskill").await;
    let root = TempDir::new().unwrap();
    let service = service(&server, &root).await;

    let prepared = service
        .prepare_add(
            origin("gofastskill/fastskill"),
            Some("gofastskill/fastskill"),
        )
        .await
        .expect("the registry's scope is not part of the content's own id");
    assert_eq!(prepared.id(), "fastskill");
}

#[tokio::test]
async fn a_scoped_registry_reference_still_refuses_content_naming_another_skill() {
    let server = MockServer::start().await;
    publish(&server, "gofastskill/fastskill", "impostor").await;
    let root = TempDir::new().unwrap();
    let service = service(&server, &root).await;

    let error = service
        .prepare_add(
            origin("gofastskill/fastskill"),
            Some("gofastskill/fastskill"),
        )
        .await
        .expect_err("a different name is a different skill");
    assert!(
        error
            .to_string()
            .contains("refusing to install under the wrong identity"),
        "{error}"
    );
}

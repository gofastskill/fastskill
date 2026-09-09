use super::{RepositoryClientError, RepositoryConfig};
use crate::core::sources::SourcesManager;
use reqwest::Client;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

pub(super) async fn download(
    config: &RepositoryConfig,
    sources: &SourcesManager,
    source_name: &str,
    id: &str,
    version: &str,
) -> Result<Vec<u8>, RepositoryClientError> {
    match config {
        RepositoryConfig::Local { path } => {
            let skill = find_skill(path, id, version)?;
            archive_skill(&skill, id)
        }
        RepositoryConfig::GitMarketplace { url, branch, tag } => {
            let checkout =
                crate::storage::git::clone_repository(url, branch.as_deref(), tag.as_deref(), None)
                    .await?;
            let skill = find_skill(checkout.path(), id, version)?;
            archive_skill(&skill, id)
        }
        RepositoryConfig::ZipUrl { .. } => {
            let marketplace = sources
                .get_marketplace_json(source_name)
                .await
                .map_err(|error| RepositoryClientError::Client(error.to_string()))?;
            let entry = marketplace
                .skills
                .iter()
                .find(|entry| entry.id == id && entry.version == version)
                .ok_or_else(|| {
                    RepositoryClientError::Client(format!(
                        "skill '{id}@{version}' is absent from repository '{source_name}'"
                    ))
                })?;
            let url = entry.download_url.as_deref().ok_or_else(|| {
                RepositoryClientError::Client(format!(
                    "repository '{source_name}' does not publish a download URL for '{id}@{version}'"
                ))
            })?;
            let response = Client::new().get(url).send().await.map_err(|error| {
                RepositoryClientError::Client(format!("failed to download '{url}': {error}"))
            })?;
            if !response.status().is_success() {
                return Err(RepositoryClientError::Client(format!(
                    "failed to download '{url}': HTTP {}",
                    response.status()
                )));
            }
            response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|error| {
                    RepositoryClientError::Client(format!("failed to read '{url}': {error}"))
                })
        }
        RepositoryConfig::HttpRegistry { .. } => Err(RepositoryClientError::NotImplemented),
    }
}

fn find_skill(root: &Path, id: &str, version: &str) -> Result<PathBuf, RepositoryClientError> {
    let mut matches = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| RepositoryClientError::Client(error.to_string()))?;
        if !entry.file_type().is_file() || entry.file_name() != "SKILL.md" {
            continue;
        }
        let directory = entry.path().parent().unwrap_or(root);
        let (candidate_id, candidate_version) =
            crate::core::install::read_skill_identity(directory)
                .map_err(RepositoryClientError::Service)?;
        if candidate_id.as_str() == id && candidate_version == version {
            matches.push(directory.to_path_buf());
        }
    }
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(RepositoryClientError::Client(format!(
            "skill '{id}@{version}' was listed but its exact source folder was not found"
        ))),
        _ => Err(RepositoryClientError::Client(format!(
            "repository contains multiple source folders for '{id}@{version}'"
        ))),
    }
}

fn archive_skill(directory: &Path, id: &str) -> Result<Vec<u8>, RepositoryClientError> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for entry in WalkDir::new(directory).follow_links(false) {
        let entry = entry.map_err(|error| RepositoryClientError::Client(error.to_string()))?;
        if entry.file_type().is_symlink() {
            return Err(RepositoryClientError::Client(format!(
                "marketplace skill '{id}' contains a symbolic link"
            )));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(directory).map_err(|error| {
            RepositoryClientError::Client(format!("cannot archive '{id}': {error}"))
        })?;
        let name = relative.to_string_lossy().replace('\\', "/");
        archive
            .start_file(name, options)
            .map_err(|error| RepositoryClientError::Client(error.to_string()))?;
        archive
            .write_all(
                &std::fs::read(entry.path())
                    .map_err(|error| RepositoryClientError::Client(error.to_string()))?,
            )
            .map_err(|error| RepositoryClientError::Client(error.to_string()))?;
    }
    archive
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| RepositoryClientError::Client(error.to_string()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::sources::SourceConfig;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn skill(root: &Path, folder: &str, id: &str, name: &str, version: &str) -> PathBuf {
        let directory = root.join(folder);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: test\nversion: {version}\nmetadata:\n  id: {id}\n---\nBody\n"
            ),
        )
        .unwrap();
        directory
    }

    #[test]
    fn exact_identity_ignores_display_name_and_folder_name() {
        let root = tempfile::tempdir().unwrap();
        let expected = skill(
            root.path(),
            "unrelated",
            "team/reviewer",
            "Reviewer Display",
            "1.2.0",
        );
        skill(root.path(), "decoy", "other", "team/reviewer", "1.2.0");
        assert_eq!(
            find_skill(root.path(), "team/reviewer", "1.2.0").unwrap(),
            expected
        );
    }

    #[test]
    fn duplicate_canonical_identity_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "one", "shared", "One", "1.0.0");
        skill(root.path(), "two", "shared", "Two", "1.0.0");
        let error = find_skill(root.path(), "shared", "1.0.0").unwrap_err();
        assert!(error.to_string().contains("multiple source folders"));
        assert!(find_skill(root.path(), "missing", "1.0.0").is_err());
    }

    #[test]
    fn archive_has_skill_at_root_and_rejects_links() {
        let root = tempfile::tempdir().unwrap();
        let directory = skill(root.path(), "source", "demo", "Demo", "1.0.0");
        let bytes = archive_skill(&directory, "demo").unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name("SKILL.md").is_ok());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(directory.join("SKILL.md"), directory.join("linked"))
                .unwrap();
            assert!(archive_skill(&directory, "demo").is_err());
        }
    }

    #[tokio::test]
    async fn zip_url_catalog_entry_downloads_installable_archive() {
        let server = MockServer::start().await;
        let source = tempfile::tempdir().unwrap();
        let directory = skill(source.path(), "payload", "demo", "Demo", "1.0.0");
        let bytes = archive_skill(&directory, "demo").unwrap();
        let marketplace = serde_json::json!({
            "name": "test",
            "metadata": { "version": "1.0.0" },
            "plugins": [{
                "name": "demo-plugin",
                "description": "Demo",
                "source": ".",
                "skills": ["demo"]
            }]
        });
        Mock::given(method("GET"))
            .and(path("/.claude-plugin/marketplace.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(marketplace))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/demo"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&server)
            .await;

        let config_dir = tempfile::tempdir().unwrap();
        let mut sources = SourcesManager::new(config_dir.path().join("sources.toml"));
        sources.load().unwrap();
        sources
            .add_source(
                "zip".to_string(),
                SourceConfig::ZipUrl {
                    base_url: server.uri(),
                    auth: None,
                },
            )
            .unwrap();
        let result = download(
            &RepositoryConfig::ZipUrl {
                base_url: server.uri(),
            },
            &sources,
            "zip",
            "demo",
            "1.0.0",
        )
        .await
        .unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(result)).unwrap();
        assert!(archive.by_name("SKILL.md").is_ok());

        let absent = download(
            &RepositoryConfig::ZipUrl {
                base_url: server.uri(),
            },
            &sources,
            "zip",
            "absent",
            "1.0.0",
        )
        .await
        .unwrap_err();
        assert!(absent.to_string().contains("absent from repository"));
    }

    #[tokio::test]
    async fn local_adapter_archives_exact_skill_and_http_registry_is_not_duplicated() {
        let root = tempfile::tempdir().unwrap();
        skill(root.path(), "payload", "team/demo", "Demo", "1.2.3");
        let sources_dir = tempfile::tempdir().unwrap();
        let mut sources = SourcesManager::new(sources_dir.path().join("sources.toml"));
        sources.load().unwrap();

        let bytes = download(
            &RepositoryConfig::Local {
                path: root.path().to_path_buf(),
            },
            &sources,
            "local",
            "team/demo",
            "1.2.3",
        )
        .await
        .unwrap();
        assert!(zip::ZipArchive::new(Cursor::new(bytes))
            .unwrap()
            .by_name("SKILL.md")
            .is_ok());

        let error = download(
            &RepositoryConfig::HttpRegistry {
                index_url: "https://example.test".into(),
            },
            &sources,
            "http",
            "team/demo",
            "1.2.3",
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RepositoryClientError::NotImplemented));

        assert!(download(
            &RepositoryConfig::GitMarketplace {
                url: root.path().join("missing.git").display().to_string(),
                branch: None,
                tag: None,
            },
            &sources,
            "git",
            "team/demo",
            "1.2.3",
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn zip_adapter_reports_download_http_failure() {
        let server = MockServer::start().await;
        let marketplace = serde_json::json!({
            "name": "test",
            "metadata": { "version": "1.0.0" },
            "plugins": [{
                "name": "demo-plugin",
                "source": ".",
                "skills": ["demo"]
            }]
        });
        Mock::given(method("GET"))
            .and(path("/.claude-plugin/marketplace.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(marketplace))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/demo"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let config_dir = tempfile::tempdir().unwrap();
        let mut sources = SourcesManager::new(config_dir.path().join("sources.toml"));
        sources.load().unwrap();
        sources
            .add_source(
                "zip".to_string(),
                SourceConfig::ZipUrl {
                    base_url: server.uri(),
                    auth: None,
                },
            )
            .unwrap();

        let error = download(
            &RepositoryConfig::ZipUrl {
                base_url: server.uri(),
            },
            &sources,
            "zip",
            "demo",
            "1.0.0",
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("HTTP 503"));
    }
}

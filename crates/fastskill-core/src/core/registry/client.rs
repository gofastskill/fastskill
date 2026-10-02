//! Registry client for querying and downloading from skill registries

use crate::core::metadata::SkillMetadata;
use crate::core::registry::auth::Auth;
use crate::core::registry::config::RegistryConfig;
use crate::core::registry::credential::{redirect_decision, Credential, RedirectDecision};
use crate::core::registry_index::{Dependency as RegistryDependency, IndexMetadata};
use crate::core::service::ServiceError;
use reqwest::header::{HeaderValue, AUTHORIZATION, LOCATION};
use reqwest::{Client, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Registry client for querying and downloading skills
pub struct RegistryClient {
    config: RegistryConfig,
    client: Client,
    /// Legacy auth (`pat`, `ssh`, `api_key`): sent when configured, skipped
    /// silently when its variable is unset, redirects followed by reqwest.
    auth: Option<Box<dyn Auth>>,
    /// Bearer auth (`bearer`, `command`, ADR-0018): an error when the token
    /// cannot be had, sent only to the registry's origin, redirects followed
    /// by [`RegistryClient::send_get`] under [`redirect_decision`].
    credential: Option<Credential>,
}

/// The most redirects an authenticated request follows.
const MAX_REDIRECTS: usize = 10;

/// Index entry for a skill version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub vers: String,
    pub deps: Vec<RegistryDependency>,
    pub cksum: String,
    pub features: HashMap<String, Vec<String>>,
    pub yanked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    pub download_url: String,
    #[serde(default)]
    pub metadata: Option<IndexMetadata>,
}

// IndexMetadata is defined in registry_index.rs

// Dependency is defined in registry_index.rs

/// Number of times [`RegistryClient::download`] has actually performed an HTTP
/// GET for a package's bytes (PRD 006 "Local Skill Cache", US-003).
/// Instrumentation-only, kept for the same reason as
/// `storage::git::CLONE_INVOCATIONS`: the content cache should make a repeat
/// pinned-version install skip the network entirely, and counting real
/// downloads is the reliable way to prove that in an integration test.
/// Deliberately not gated behind `#[cfg(test)]` — integration tests are a
/// separate compilation unit and cannot see items scoped that way.
#[doc(hidden)] // test instrumentation, not supported public API
pub static DOWNLOAD_INVOCATIONS: AtomicUsize = AtomicUsize::new(0);

impl RegistryClient {
    /// Create a new registry client
    pub fn new(config: RegistryConfig) -> Result<Self, ServiceError> {
        use crate::core::registry::auth::{ApiKey, GitHubPat, SshKey};
        use crate::core::registry::config::AuthConfig;

        let credential = config.auth.as_ref().and_then(Credential::from_config);
        let mut builder = Client::builder().user_agent("fastskill/0.6.8");
        if credential.is_some() {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }
        let client = builder
            .build()
            .map_err(|e| ServiceError::Custom(format!("Failed to create HTTP client: {}", e)))?;

        let auth: Option<Box<dyn Auth>> = match &config.auth {
            Some(AuthConfig::Pat { env_var }) => Some(Box::new(GitHubPat::new(env_var.clone()))),
            Some(AuthConfig::Ssh { key_path }) => Some(Box::new(SshKey::new(key_path.clone()))),
            Some(AuthConfig::ApiKey { env_var }) => Some(Box::new(ApiKey::new(env_var.clone()))),
            Some(AuthConfig::Bearer { .. } | AuthConfig::Command { .. }) | None => None,
        };

        Ok(Self {
            config,
            client,
            auth,
            credential,
        })
    }

    /// GET `url`, the one place a registry request gets its `Authorization`
    /// header. Index, listing and download requests all come through here.
    pub async fn send_get(&self, url: &str) -> Result<Response, ServiceError> {
        match &self.credential {
            Some(credential) => self.send_get_with_credential(credential, url).await,
            None => {
                let mut request = self.client.get(url);
                if self.uses_registry_origin(url) {
                    if let Some(ref auth) = self.auth {
                        if auth.is_configured() {
                            if let Ok(header_value) = auth.get_auth_header() {
                                request = request.header(AUTHORIZATION, header_value);
                            }
                        }
                    }
                }
                request
                    .send()
                    .await
                    .map_err(|e| ServiceError::Custom(e.to_string()))
            }
        }
    }

    /// Follow redirects by hand so the token reaches only the registry's
    /// origin and never travels over a downgraded connection.
    async fn send_get_with_credential(
        &self,
        credential: &Credential,
        url: &str,
    ) -> Result<Response, ServiceError> {
        let invalid = |what: &str, e: url::ParseError| {
            ServiceError::Custom(format!("Invalid {what} URL: {e}"))
        };
        let registry_origin = Url::parse(&self.config.index_url)
            .map_err(|e| invalid("registry", e))?
            .origin();
        let mut current = Url::parse(url).map_err(|e| invalid("request", e))?;
        let mut send_token = current.origin() == registry_origin;
        let mut header: Option<HeaderValue> = None;
        for _ in 0..=MAX_REDIRECTS {
            let mut request = self.client.get(current.clone());
            if send_token {
                if header.is_none() {
                    let value = credential
                        .header(&self.config.name, &self.config.index_url)
                        .await?;
                    header = Some(value);
                }
                if let Some(value) = &header {
                    request = request.header(AUTHORIZATION, value.clone());
                }
            }
            let response = request
                .send()
                .await
                .map_err(|e| ServiceError::Custom(e.to_string()))?;
            let followed = matches!(
                response.status(),
                StatusCode::MOVED_PERMANENTLY
                    | StatusCode::FOUND
                    | StatusCode::SEE_OTHER
                    | StatusCode::TEMPORARY_REDIRECT
                    | StatusCode::PERMANENT_REDIRECT
            );
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok());
            let Some(location) = location.filter(|_| followed) else {
                return Ok(response);
            };
            let next = current.join(location).map_err(|e| invalid("redirect", e))?;
            match redirect_decision(&current, &next, &registry_origin) {
                RedirectDecision::Refuse(reason) => return Err(ServiceError::Custom(reason)),
                RedirectDecision::Follow {
                    send_token: next_send,
                } => send_token = next_send,
            }
            current = next;
        }
        Err(ServiceError::Custom(format!(
            "stopped after {MAX_REDIRECTS} redirects"
        )))
    }

    /// Get the index URL for a skill (flat layout: scope/skill-name)
    fn get_index_url(&self, skill_id: &str) -> String {
        // Flat layout: use skill_id directly (e.g., "dev-user/test-skill")
        // index_url should be the base URL for the index (e.g., http://159.69.182.11:8080/index)
        format!(
            "{}/{}",
            self.config.index_url.trim_end_matches('/'),
            skill_id
        )
    }

    fn uses_registry_origin(&self, download_url: &str) -> bool {
        let Ok(registry) = reqwest::Url::parse(&self.config.index_url) else {
            return false;
        };
        let Ok(download) = reqwest::Url::parse(download_url) else {
            return false;
        };
        registry.origin() == download.origin()
    }

    /// Get skill information from registry
    /// Returns all versions for the skill (reads single file with newline-delimited JSON)
    pub async fn get_skill(&self, name: &str) -> Result<Vec<IndexEntry>, ServiceError> {
        let url = self.get_index_url(name);

        let response = self
            .send_get(&url)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to fetch skill index: {}", e)))?;

        if !response.status().is_success() {
            if response.status() == 404 {
                return Ok(Vec::new()); // Skill not found
            }
            return Err(ServiceError::Custom(format!(
                "Failed to fetch skill index: HTTP {}",
                response.status()
            )));
        }

        let content = response
            .text()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read index file: {}", e)))?;

        // Parse line-by-line (newline-delimited JSON)
        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<IndexEntry>(line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    // Log error but continue parsing other lines
                    eprintln!(
                        "Warning: Failed to parse index entry: {} (line: {})",
                        e, line
                    );
                }
            }
        }

        Ok(entries)
    }

    /// Get index entry for a specific skill version
    async fn get_index_entry(
        &self,
        name: &str,
        version: &str,
    ) -> Result<Option<IndexEntry>, ServiceError> {
        // Get all versions and find the matching one
        let entries = self.get_skill(name).await?;
        Ok(entries.into_iter().find(|e| e.vers == version))
    }

    /// Get available versions for a skill
    pub async fn get_versions(&self, name: &str) -> Result<Vec<String>, ServiceError> {
        let entries = self.get_skill(name).await?;
        let mut versions: Vec<String> = entries.iter().map(|e| e.vers.clone()).collect();
        crate::core::version::sort_versions_desc(&mut versions);
        Ok(versions)
    }

    /// Get latest version for a skill (excluding pre-releases by default)
    pub async fn get_latest_version(
        &self,
        name: &str,
        include_pre_release: bool,
    ) -> Result<Option<String>, ServiceError> {
        let versions = self.get_versions(name).await?;

        if versions.is_empty() {
            return Ok(None);
        }

        // Filter out pre-releases if not requested
        let candidates: Vec<&String> = if include_pre_release {
            versions.iter().collect()
        } else {
            versions
                .iter()
                .filter(|v| !v.contains('-')) // Simple pre-release detection
                .collect()
        };

        if candidates.is_empty() {
            // If all are pre-releases and not requested, return None
            return Ok(None);
        }

        // Use semver crate for proper version comparison
        use semver::Version;
        let mut parsed_versions: Vec<(Version, &String)> = candidates
            .iter()
            .filter_map(|v| {
                // Try to parse as semver, skip if invalid
                Version::parse(v).ok().map(|ver| (ver, *v))
            })
            .collect();

        if parsed_versions.is_empty() {
            // Fallback to first version if none can be parsed as semver
            return Ok(Some(candidates[0].clone()));
        }

        // Sort by version (highest first)
        parsed_versions.sort_by(|a, b| b.0.cmp(&a.0));

        Ok(Some(parsed_versions[0].1.clone()))
    }

    /// Get specific version of a skill
    pub async fn get_version(
        &self,
        name: &str,
        version: &str,
    ) -> Result<Option<IndexEntry>, ServiceError> {
        self.get_index_entry(name, version).await
    }

    /// Download skill package
    pub async fn download(&self, name: &str, version: &str) -> Result<Vec<u8>, ServiceError> {
        DOWNLOAD_INVOCATIONS.fetch_add(1, Ordering::SeqCst);
        let entry = self.get_version(name, version).await?.ok_or_else(|| {
            ServiceError::Custom(format!(
                "Skill {} version {} not found in registry",
                name, version
            ))
        })?;

        if entry.yanked {
            return Err(ServiceError::Custom(format!(
                "Skill {} version {} has been yanked",
                name, version
            )));
        }

        // Auth goes only to the registry's own origin (`send_get`).
        let response = self
            .send_get(&entry.download_url)
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to download package: {}", e)))?;

        if !response.status().is_success() {
            return Err(ServiceError::Custom(format!(
                "Failed to download package: HTTP {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ServiceError::Custom(format!("Failed to read package data: {}", e)))?;

        // Verify checksum
        let calculated = format!(
            "sha256:{}",
            crate::utils::to_hex_lower(&sha2::Sha256::digest(&bytes))
        );
        if calculated != entry.cksum {
            return Err(ServiceError::Custom(format!(
                "Checksum mismatch: expected {}, got {}",
                entry.cksum, calculated
            )));
        }

        Ok(bytes.to_vec())
    }

    /// Search skills in registry (basic implementation - scans index)
    pub async fn search(&self, _query: &str) -> Result<Vec<SkillMetadata>, ServiceError> {
        // For now, return empty - full search requires scanning entire index
        // This would be implemented by fetching a search endpoint or scanning index
        // For GitHub-based registries, we'd need to use GitHub API or clone the repo
        Ok(Vec::new())
    }
}

#[cfg(test)]
#[path = "client_auth_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod auth_tests;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // ── HTTP-backed tests (mock server) ───────────────────────────────────────

    use crate::core::registry::config::{AuthConfig, RegistryConfig};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn config_for(index_url: &str, auth: Option<AuthConfig>) -> RegistryConfig {
        RegistryConfig {
            name: "test-registry".to_string(),
            registry_type: "git".to_string(),
            index_url: index_url.to_string(),
            auth,
            storage: None,
        }
    }

    fn make_entry(name: &str, vers: &str, download_url: &str, cksum: &str) -> IndexEntry {
        IndexEntry {
            name: name.to_string(),
            vers: vers.to_string(),
            deps: Vec::new(),
            cksum: cksum.to_string(),
            features: HashMap::new(),
            yanked: false,
            links: None,
            download_url: download_url.to_string(),
            metadata: None,
        }
    }

    /// Newline-delimited JSON index body from a slice of entries.
    fn index_body(entries: &[IndexEntry]) -> String {
        entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_new_without_auth() {
        assert!(RegistryClient::new(config_for("http://example.com/index", None)).is_ok());
    }

    #[test]
    fn test_new_with_each_auth_variant() {
        for auth in [
            AuthConfig::Pat {
                env_var: "GITHUB_TOKEN".to_string(),
            },
            AuthConfig::Ssh {
                key_path: "/tmp/key".into(),
            },
            AuthConfig::ApiKey {
                env_var: "API_KEY".to_string(),
            },
            AuthConfig::Bearer {
                env_var: "REGISTRY_TOKEN".to_string(),
            },
            AuthConfig::Command {
                command: vec!["helper".to_string()],
            },
        ] {
            assert!(
                RegistryClient::new(config_for("http://example.com/index", Some(auth))).is_ok()
            );
        }
    }

    #[test]
    fn test_get_index_url_trims_trailing_slash() {
        let client = RegistryClient::new(config_for("http://example.com/index/", None)).unwrap();
        assert_eq!(
            client.get_index_url("scope/skill"),
            "http://example.com/index/scope/skill"
        );
    }

    #[tokio::test]
    async fn test_get_skill_success_multiple_versions() {
        let server = MockServer::start().await;
        let entries = vec![
            make_entry("myskill", "1.0.0", "http://x/dl", "sha256:aa"),
            make_entry("myskill", "1.2.0", "http://x/dl", "sha256:bb"),
        ];
        Mock::given(method("GET"))
            .and(path("/myskill"))
            .respond_with(ResponseTemplate::new(200).set_body_string(index_body(&entries)))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let result = client.get_skill("myskill").await.unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_get_skill_404_returns_empty() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let result = client.get_skill("missing").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_get_skill_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/boom"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        assert!(client.get_skill("boom").await.is_err());
    }

    #[tokio::test]
    async fn test_get_skill_skips_malformed_lines() {
        let server = MockServer::start().await;
        let good =
            serde_json::to_string(&make_entry("s", "1.0.0", "http://x/dl", "sha256:aa")).unwrap();
        let body = format!("{}\n\nthis is not json\n{}", good, good);
        Mock::given(method("GET"))
            .and(path("/s"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let result = client.get_skill("s").await.unwrap();
        // Two good lines parse; the blank and malformed lines are skipped.
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_get_versions_sorted_desc() {
        let server = MockServer::start().await;
        let entries = vec![
            make_entry("v", "1.9.0", "http://x/dl", "sha256:aa"),
            make_entry("v", "1.10.0", "http://x/dl", "sha256:bb"),
            make_entry("v", "1.2.0", "http://x/dl", "sha256:cc"),
        ];
        Mock::given(method("GET"))
            .and(path("/v"))
            .respond_with(ResponseTemplate::new(200).set_body_string(index_body(&entries)))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let versions = client.get_versions("v").await.unwrap();
        assert_eq!(versions, vec!["1.10.0", "1.9.0", "1.2.0"]);
    }

    async fn mount_index(server: &MockServer, name: &str, entries: &[IndexEntry]) {
        Mock::given(method("GET"))
            .and(path(format!("/{}", name)))
            .respond_with(ResponseTemplate::new(200).set_body_string(index_body(entries)))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn test_get_latest_version_excludes_prerelease() {
        let server = MockServer::start().await;
        let entries = vec![
            make_entry("p", "1.0.0", "http://x/dl", "sha256:aa"),
            make_entry("p", "2.0.0-rc1", "http://x/dl", "sha256:bb"),
        ];
        mount_index(&server, "p", &entries).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let latest = client.get_latest_version("p", false).await.unwrap();
        assert_eq!(latest, Some("1.0.0".to_string()));
    }

    #[tokio::test]
    async fn test_get_latest_version_includes_prerelease() {
        let server = MockServer::start().await;
        let entries = vec![
            make_entry("p", "1.0.0", "http://x/dl", "sha256:aa"),
            make_entry("p", "2.0.0-rc1", "http://x/dl", "sha256:bb"),
        ];
        mount_index(&server, "p", &entries).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let latest = client.get_latest_version("p", true).await.unwrap();
        assert_eq!(latest, Some("2.0.0-rc1".to_string()));
    }

    #[tokio::test]
    async fn test_get_latest_version_empty_is_none() {
        let server = MockServer::start().await;
        mount_index(&server, "none", &[]).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        assert_eq!(
            client.get_latest_version("none", false).await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn test_get_latest_version_all_prerelease_is_none() {
        let server = MockServer::start().await;
        let entries = vec![make_entry("p", "1.0.0-alpha", "http://x/dl", "sha256:aa")];
        mount_index(&server, "p", &entries).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        assert_eq!(client.get_latest_version("p", false).await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_get_latest_version_unparseable_fallback() {
        let server = MockServer::start().await;
        // Not a dash-containing string (so it passes the pre-release filter) but
        // not valid semver either: exercises the "no semver parsed" fallback.
        let entries = vec![make_entry("p", "notsemver", "http://x/dl", "sha256:aa")];
        mount_index(&server, "p", &entries).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        assert_eq!(
            client.get_latest_version("p", false).await.unwrap(),
            Some("notsemver".to_string())
        );
    }

    #[tokio::test]
    async fn test_get_version_found_and_not_found() {
        let server = MockServer::start().await;
        let entries = vec![make_entry("g", "1.0.0", "http://x/dl", "sha256:aa")];
        mount_index(&server, "g", &entries).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        assert!(client.get_version("g", "1.0.0").await.unwrap().is_some());
        assert!(client.get_version("g", "9.9.9").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_download_success_verifies_checksum() {
        let server = MockServer::start().await;
        let payload = b"skill-package-bytes";
        let cksum = format!(
            "sha256:{}",
            crate::utils::to_hex_lower(&sha2::Sha256::digest(payload))
        );
        let dl_url = format!("{}/dl", server.uri());
        let entries = vec![make_entry("d", "1.0.0", &dl_url, &cksum)];
        mount_index(&server, "d", &entries).await;
        Mock::given(method("GET"))
            .and(path("/dl"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let bytes = client.download("d", "1.0.0").await.unwrap();
        assert_eq!(bytes, payload);
    }

    #[tokio::test]
    async fn download_scopes_registry_auth_to_the_registry_origin() {
        std::env::set_var("FASTSKILL_CROSS_ORIGIN_PAT", "test-token");
        let registry = MockServer::start().await;
        let artifact = MockServer::start().await;
        let payload = b"skill-package-bytes";
        let cksum = format!(
            "sha256:{}",
            crate::utils::to_hex_lower(&sha2::Sha256::digest(payload))
        );
        let entries = vec![make_entry(
            "d",
            "1.0.0",
            &format!("{}/dl", artifact.uri()),
            &cksum,
        )];
        mount_index(&registry, "d", &entries).await;
        Mock::given(method("GET"))
            .and(path("/dl"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&artifact)
            .await;
        let client = RegistryClient::new(config_for(
            &registry.uri(),
            Some(AuthConfig::Pat {
                env_var: "FASTSKILL_CROSS_ORIGIN_PAT".to_string(),
            }),
        ))
        .unwrap();

        assert_eq!(client.download("d", "1.0.0").await.unwrap(), payload);
        let requests = artifact.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0]
            .headers
            .keys()
            .any(|name| name.as_str().eq_ignore_ascii_case("authorization")));

        let same_origin_entries = vec![make_entry(
            "same",
            "1.0.0",
            &format!("{}/same-dl", registry.uri()),
            &cksum,
        )];
        mount_index(&registry, "same", &same_origin_entries).await;
        Mock::given(method("GET"))
            .and(path("/same-dl"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&registry)
            .await;
        assert_eq!(client.download("same", "1.0.0").await.unwrap(), payload);
        let requests = registry.received_requests().await.unwrap();
        let artifact_request = requests
            .iter()
            .find(|request| request.url.path() == "/same-dl")
            .unwrap();
        assert!(artifact_request
            .headers
            .keys()
            .any(|name| name.as_str().eq_ignore_ascii_case("authorization")));
        std::env::remove_var("FASTSKILL_CROSS_ORIGIN_PAT");
    }

    #[tokio::test]
    async fn test_download_checksum_mismatch() {
        let server = MockServer::start().await;
        let dl_url = format!("{}/dl", server.uri());
        let entries = vec![make_entry("d", "1.0.0", &dl_url, "sha256:deadbeef")];
        mount_index(&server, "d", &entries).await;
        Mock::given(method("GET"))
            .and(path("/dl"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"actual".to_vec()))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let result = client.download("d", "1.0.0").await;
        assert!(matches!(result, Err(ServiceError::Custom(m)) if m.contains("Checksum mismatch")));
    }

    #[tokio::test]
    async fn test_download_yanked_rejected() {
        let server = MockServer::start().await;
        let mut entry = make_entry("y", "1.0.0", "http://x/dl", "sha256:aa");
        entry.yanked = true;
        mount_index(&server, "y", &[entry]).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        let result = client.download("y", "1.0.0").await;
        assert!(matches!(result, Err(ServiceError::Custom(m)) if m.contains("yanked")));
    }

    #[tokio::test]
    async fn test_download_version_not_found() {
        let server = MockServer::start().await;
        mount_index(&server, "n", &[]).await;
        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        assert!(client.download("n", "1.0.0").await.is_err());
    }

    #[tokio::test]
    async fn test_download_http_error() {
        let server = MockServer::start().await;
        let dl_url = format!("{}/dl", server.uri());
        let entries = vec![make_entry("e", "1.0.0", &dl_url, "sha256:aa")];
        mount_index(&server, "e", &entries).await;
        Mock::given(method("GET"))
            .and(path("/dl"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(&server.uri(), None)).unwrap();
        assert!(client.download("e", "1.0.0").await.is_err());
    }

    #[tokio::test]
    async fn test_search_returns_empty() {
        let client = RegistryClient::new(config_for("http://example.com/index", None)).unwrap();
        assert!(client.search("anything").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_skill_sends_auth_header_when_configured() {
        use wiremock::matchers::header_exists;
        // Set env so the PAT auth reports configured and emits an Authorization header.
        std::env::set_var("FASTSKILL_TEST_PAT", "secret-token");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth-skill"))
            .and(header_exists("Authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;

        let client = RegistryClient::new(config_for(
            &server.uri(),
            Some(AuthConfig::Pat {
                env_var: "FASTSKILL_TEST_PAT".to_string(),
            }),
        ))
        .unwrap();
        // Succeeds only if the Authorization header matcher matched.
        let result = client.get_skill("auth-skill").await.unwrap();
        assert!(result.is_empty());
        std::env::remove_var("FASTSKILL_TEST_PAT");
    }
}

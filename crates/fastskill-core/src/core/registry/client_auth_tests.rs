//! Bearer and credential-command auth through `RegistryClient` (ADR-0018).

use super::*;
use crate::core::registry::config::AuthConfig;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PAYLOAD: &[u8] = b"skill-package-bytes";

fn cksum() -> String {
    format!(
        "sha256:{}",
        crate::utils::to_hex_lower(&sha2::Sha256::digest(PAYLOAD))
    )
}

fn client(name: &str, index_url: &str, auth: AuthConfig) -> RegistryClient {
    RegistryClient::new(RegistryConfig {
        name: name.to_string(),
        registry_type: "git".to_string(),
        index_url: index_url.to_string(),
        auth: Some(auth),
        storage: None,
    })
    .unwrap()
}

fn bearer(env_var: &str) -> AuthConfig {
    AuthConfig::Bearer {
        env_var: env_var.to_string(),
    }
}

async fn mount_index(server: &MockServer, name: &str, download_url: &str) {
    let entry = IndexEntry {
        name: name.to_string(),
        vers: "1.0.0".to_string(),
        deps: Vec::new(),
        cksum: cksum(),
        features: HashMap::new(),
        yanked: false,
        links: None,
        download_url: download_url.to_string(),
        metadata: None,
    };
    Mock::given(method("GET"))
        .and(path(format!("/{name}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(serde_json::to_string(&entry).unwrap()),
        )
        .mount(server)
        .await;
}

async fn mount_payload(server: &MockServer, at: &str) {
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PAYLOAD.to_vec()))
        .mount(server)
        .await;
}

fn authorization(request: &Request) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(name, _)| name.as_str().eq_ignore_ascii_case("authorization"))
        .map(|(_, values)| values.as_str().to_string())
}

async fn requests_to(server: &MockServer, at: &str) -> Vec<Request> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path() == at)
        .collect()
}

#[tokio::test]
async fn bearer_env_var_is_sent_on_index_and_same_origin_download() {
    std::env::set_var("FASTSKILL_TEST_CLIENT_BEARER", "env-token");
    let registry = MockServer::start().await;
    mount_index(&registry, "s", &format!("{}/dl", registry.uri())).await;
    mount_payload(&registry, "/dl").await;
    let client = client(
        "bearer-env",
        &registry.uri(),
        bearer("FASTSKILL_TEST_CLIENT_BEARER"),
    );

    assert_eq!(client.download("s", "1.0.0").await.unwrap(), PAYLOAD);
    for at in ["/s", "/dl"] {
        let requests = requests_to(&registry, at).await;
        assert_eq!(
            authorization(&requests[0]).as_deref(),
            Some("Bearer env-token"),
            "{at}"
        );
    }
}

#[tokio::test]
async fn a_missing_bearer_env_var_fails_the_request() {
    let registry = MockServer::start().await;
    mount_index(&registry, "s", &format!("{}/dl", registry.uri())).await;
    let client = client(
        "bearer-missing",
        &registry.uri(),
        bearer("FASTSKILL_TEST_CLIENT_UNSET"),
    );

    let error = client.get_skill("s").await.unwrap_err().to_string();
    assert!(error.contains("FASTSKILL_TEST_CLIENT_UNSET"), "{error}");
    // Nothing was sent without the token.
    assert!(registry.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn no_token_goes_to_a_download_on_another_origin() {
    std::env::set_var("FASTSKILL_TEST_CLIENT_OTHER_ORIGIN", "env-token");
    let registry = MockServer::start().await;
    let artifact = MockServer::start().await;
    mount_index(&registry, "s", &format!("{}/dl", artifact.uri())).await;
    mount_payload(&artifact, "/dl").await;
    let client = client(
        "bearer-other-origin",
        &registry.uri(),
        bearer("FASTSKILL_TEST_CLIENT_OTHER_ORIGIN"),
    );

    assert_eq!(client.download("s", "1.0.0").await.unwrap(), PAYLOAD);
    let downloads = requests_to(&artifact, "/dl").await;
    assert_eq!(downloads.len(), 1);
    assert_eq!(authorization(&downloads[0]), None);
}

#[tokio::test]
async fn a_cross_origin_redirect_drops_the_token() {
    std::env::set_var("FASTSKILL_TEST_CLIENT_REDIRECT", "env-token");
    let registry = MockServer::start().await;
    let elsewhere = MockServer::start().await;
    mount_index(&registry, "s", &format!("{}/dl", registry.uri())).await;
    Mock::given(method("GET"))
        .and(path("/dl"))
        .and(header("authorization", "Bearer env-token"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/moved", registry.uri()).as_str()),
        )
        .mount(&registry)
        .await;
    Mock::given(method("GET"))
        .and(path("/moved"))
        .and(header("authorization", "Bearer env-token"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/final", elsewhere.uri()).as_str()),
        )
        .mount(&registry)
        .await;
    mount_payload(&elsewhere, "/final").await;
    let client = client(
        "bearer-redirect",
        &registry.uri(),
        bearer("FASTSKILL_TEST_CLIENT_REDIRECT"),
    );

    assert_eq!(client.download("s", "1.0.0").await.unwrap(), PAYLOAD);
    // The same-origin hop kept the token (the mocks above require it); the
    // cross-origin hop did not get it.
    let landed = requests_to(&elsewhere, "/final").await;
    assert_eq!(landed.len(), 1);
    assert_eq!(authorization(&landed[0]), None);
}

#[tokio::test]
async fn too_many_redirects_is_an_error() {
    std::env::set_var("FASTSKILL_TEST_CLIENT_LOOP", "env-token");
    let registry = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/s"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/s", registry.uri()).as_str()),
        )
        .mount(&registry)
        .await;
    let client = client(
        "bearer-loop",
        &registry.uri(),
        bearer("FASTSKILL_TEST_CLIENT_LOOP"),
    );
    let error = client.get_skill("s").await.unwrap_err().to_string();
    assert!(error.contains("redirects"), "{error}");
}

#[tokio::test]
async fn legacy_pat_is_unchanged_when_its_variable_is_missing() {
    let registry = MockServer::start().await;
    mount_index(&registry, "s", &format!("{}/dl", registry.uri())).await;
    let client = client(
        "pat-missing",
        &registry.uri(),
        AuthConfig::Pat {
            env_var: "FASTSKILL_TEST_CLIENT_PAT_UNSET".to_string(),
        },
    );
    // Skipped silently, as before: the request goes out without a header.
    assert_eq!(client.get_skill("s").await.unwrap().len(), 1);
    let requests = requests_to(&registry, "/s").await;
    assert_eq!(authorization(&requests[0]), None);
}

/// Point FastSkill's config dir at a temp dir for a command-auth test.
struct ConfigDir {
    _mutex: std::sync::MutexGuard<'static, ()>,
    previous: Option<std::ffi::OsString>,
    dir: tempfile::TempDir,
}

impl ConfigDir {
    fn new() -> Self {
        let mutex = crate::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        Self {
            _mutex: mutex,
            previous,
            dir,
        }
    }
}

impl Drop for ConfigDir {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var("XDG_CONFIG_HOME", previous),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn credential_command_runs_once_and_its_token_is_sent() {
    let config = ConfigDir::new();
    let counter = config.dir.path().join("runs");
    let script = format!("echo run >> '{}'; echo cmd-token", counter.display());
    let registry = MockServer::start().await;
    mount_index(&registry, "s", &format!("{}/dl", registry.uri())).await;
    mount_payload(&registry, "/dl").await;
    let auth = AuthConfig::Command {
        command: vec!["sh".to_string(), "-c".to_string(), script],
    };
    let client = client("command-once", &registry.uri(), auth);

    assert_eq!(client.download("s", "1.0.0").await.unwrap(), PAYLOAD);
    assert_eq!(client.get_versions("s").await.unwrap(), vec!["1.0.0"]);
    let runs = std::fs::read_to_string(&counter).unwrap();
    assert_eq!(runs.lines().count(), 1, "{runs}");
    for request in registry.received_requests().await.unwrap() {
        assert_eq!(authorization(&request).as_deref(), Some("Bearer cmd-token"));
    }
    // The command ran from FastSkill's config dir, which it created.
    assert!(config.dir.path().join("fastskill").is_dir());
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn a_failing_command_fails_the_request_without_showing_the_token() {
    let _config = ConfigDir::new();
    let registry = MockServer::start().await;
    mount_index(&registry, "s", &format!("{}/dl", registry.uri())).await;
    let auth = AuthConfig::Command {
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            "echo half-printed-secret; exit 7".to_string(),
        ],
    };
    let client = client("command-fails", &registry.uri(), auth);

    let error = client.get_skill("s").await.unwrap_err();
    let shown = format!("{error} {error:?}");
    assert!(shown.contains("exit status: 7"), "{shown}");
    assert!(!shown.contains("half-printed-secret"), "{shown}");
    assert!(registry.received_requests().await.unwrap().is_empty());
}

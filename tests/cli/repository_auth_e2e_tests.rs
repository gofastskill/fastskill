//! E2E tests for http-registry bearer and credential-command auth (ADR-0018).
//!
//! The registry is a loopback `wiremock` server that answers 401 unless the
//! request carries `Authorization: Bearer <expected>`. FastSkill's user
//! config directory is a temporary directory (`XDG_CONFIG_HOME`), so the
//! user `repositories.toml` and the credential command live there.

#![cfg(unix)]
#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::snapshot_helpers::{run_fastskill_command_with_env, CommandResult};
use sha2::Digest;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SKILL_ID: &str = "widget";
const VERSION: &str = "1.0.0";
const TOKEN: &str = "e2e-registry-token-7f3a";

fn build_zip() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file(format!("{SKILL_ID}/SKILL.md"), opts)
            .unwrap();
        let skill_md = format!(
            "---\nname: {SKILL_ID}\nversion: \"{VERSION}\"\ndescription: A registry skill\n---\nBody\n"
        );
        writer.write_all(skill_md.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buf
}

/// A registry whose index entry and download both need the bearer token.
async fn registry_requiring(token: &str) -> MockServer {
    let server = MockServer::start().await;
    let zip_bytes = build_zip();
    let digest = sha2::Sha256::digest(&zip_bytes);
    let cksum = format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let entry = serde_json::json!({
        "name": SKILL_ID,
        "vers": VERSION,
        "deps": [],
        "cksum": cksum,
        "features": {},
        "yanked": false,
        "links": null,
        "download_url": format!("{}/dl/{VERSION}", server.uri()),
    });
    let authorized = format!("Bearer {token}");
    Mock::given(method("GET"))
        .and(path(format!("/index/{SKILL_ID}")))
        .and(header("Authorization", authorized.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_string(entry.to_string()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/dl/{VERSION}")))
        .and(header("Authorization", authorized.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_bytes))
        .mount(&server)
        .await;
    // Mounted last, so it only answers requests the mocks above did not.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    server
}

/// A project directory, a user config directory and a cache directory.
struct Machine {
    root: TempDir,
}

impl Machine {
    fn new(project_repositories: &str) -> Self {
        let root = TempDir::new().unwrap();
        let project = root.path().join("project");
        fs::create_dir_all(project.join(".claude/skills")).unwrap();
        fs::write(
            project.join("skill-project.toml"),
            format!(
                "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n{project_repositories}"
            ),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("config/fastskill")).unwrap();
        Self { root }
    }

    fn project(&self) -> PathBuf {
        self.root.path().join("project")
    }

    fn config_dir(&self) -> PathBuf {
        self.root.path().join("config/fastskill")
    }

    /// Write an executable credential helper into the user config dir.
    fn helper(&self, name: &str, script: &str) -> PathBuf {
        let path = self.config_dir().join(name);
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn user_repositories(&self, content: &str) {
        fs::write(self.config_dir().join("repositories.toml"), content).unwrap();
    }

    fn run(&self, args: &[&str], extra_env: &[(&str, &str)]) -> CommandResult {
        let config_home = self.root.path().join("config");
        let cache = self.root.path().join("cache");
        let mut env = vec![
            ("XDG_CONFIG_HOME", config_home.to_str().unwrap()),
            ("FASTSKILL_CACHE_DIR", cache.to_str().unwrap()),
        ];
        env.extend_from_slice(extra_env);
        run_fastskill_command_with_env(args, &env, Some(&self.project()))
    }

    fn installed(&self) -> bool {
        self.project()
            .join(".claude/skills")
            .join(SKILL_ID)
            .join("SKILL.md")
            .exists()
    }
}

fn user_command_repository(index_url: &str, helper: &Path) -> String {
    format!(
        "[[repositories]]\nname = \"private\"\ntype = \"http-registry\"\npriority = 0\n\
         index_url = \"{index_url}\"\nauth = {{ type = \"command\", command = [\"{}\", \"token\"] }}\n",
        helper.display()
    )
}

fn install(machine: &Machine, extra_env: &[(&str, &str)]) -> CommandResult {
    let reference = format!("{SKILL_ID}@{VERSION}");
    machine.run(
        &["skill", "add", &reference, "--repository", "private"],
        extra_env,
    )
}

/// The acceptance case: the token comes from a credential command named in
/// the user config, runs once from the config dir without a terminal, and
/// reaches both the index and the download.
#[tokio::test]
async fn credential_command_in_user_config_installs_from_a_registry_that_requires_a_token() {
    let server = registry_requiring(TOKEN).await;
    let machine = Machine::new("");
    let helper = machine.helper(
        "token-helper",
        &format!(
            "echo run >> runs.log\n\
             [ \"$1\" = token ] || exit 9\n\
             [ \"$FASTSKILL_INTERACTIVE\" = 0 ] || exit 8\n\
             printf '{TOKEN}\\r\\n'"
        ),
    );
    machine.user_repositories(&user_command_repository(
        &format!("{}/index", server.uri()),
        &helper,
    ));

    let result = install(&machine, &[]);
    assert!(
        result.success,
        "install failed:\nstdout: {}\nstderr: {}",
        result.stdout, result.stderr
    );
    assert!(machine.installed());
    let runs = fs::read_to_string(machine.config_dir().join("runs.log")).unwrap();
    assert_eq!(runs.lines().count(), 1, "the command runs once per process");
    assert!(!result.stdout.contains(TOKEN) && !result.stderr.contains(TOKEN));

    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().any(|r| r.url.path() == "/index/widget"));
    assert!(requests.iter().any(|r| r.url.path() == "/dl/1.0.0"));

    // Listing shows the auth type and never the token.
    let listed = machine.run(&["repo", "info", "private"], &[]);
    assert!(listed.success, "{}", listed.stderr);
    assert!(listed.stdout.contains("command"), "{}", listed.stdout);
    assert!(!listed.stdout.contains(TOKEN));
}

#[tokio::test]
async fn a_failing_credential_command_fails_the_install_without_showing_the_token() {
    let server = registry_requiring(TOKEN).await;
    let machine = Machine::new("");
    let helper = machine.helper(
        "token-helper",
        &format!("echo {TOKEN}\necho {TOKEN} >&2\nexit 4"),
    );
    machine.user_repositories(&user_command_repository(
        &format!("{}/index", server.uri()),
        &helper,
    ));

    let result = install(&machine, &[]);
    assert!(!result.success);
    assert!(!machine.installed());
    let output = format!("{}{}", result.stdout, result.stderr);
    assert!(output.contains("token-helper"), "{output}");
    assert!(output.contains("exit status: 4"), "{output}");
    assert!(!output.contains(TOKEN), "{output}");
}

#[tokio::test]
async fn bearer_env_var_in_the_project_file_installs_and_a_missing_one_is_an_error() {
    let server = registry_requiring(TOKEN).await;
    let machine = Machine::new(&format!(
        "\n[[tool.fastskill.repositories]]\nname = \"private\"\ntype = \"http-registry\"\n\
         priority = 0\nindex_url = \"{}/index\"\n\
         auth = {{ type = \"bearer\", env_var = \"FASTSKILL_E2E_REGISTRY_TOKEN\" }}\n",
        server.uri()
    ));

    let missing = install(&machine, &[]);
    assert!(!missing.success);
    assert!(
        missing.stderr.contains("FASTSKILL_E2E_REGISTRY_TOKEN"),
        "{}",
        missing.stderr
    );
    assert!(!machine.installed());

    let result = install(&machine, &[("FASTSKILL_E2E_REGISTRY_TOKEN", TOKEN)]);
    assert!(
        result.success,
        "install failed:\nstdout: {}\nstderr: {}",
        result.stdout, result.stderr
    );
    assert!(machine.installed());
    assert!(!result.stdout.contains(TOKEN) && !result.stderr.contains(TOKEN));
}

#[test]
fn a_project_file_that_names_a_credential_command_is_refused() {
    let machine = Machine::new(
        "\n[[tool.fastskill.repositories]]\nname = \"private\"\ntype = \"http-registry\"\n\
         priority = 0\nindex_url = \"https://registry.example/index\"\n\
         auth = { type = \"command\", command = [\"touch\", \"ran\"] }\n",
    );
    let result = machine.run(&["repo", "list"], &[]);
    assert!(!result.success);
    assert!(
        result.stderr.contains("cannot name a command"),
        "{}",
        result.stderr
    );
    assert!(!machine.project().join("ran").exists());
    assert!(!machine.config_dir().join("ran").exists());
}

#[test]
fn repo_add_saves_command_auth_to_the_user_file_only() {
    let machine = Machine::new("");
    let refused = machine.run(
        &[
            "repo",
            "add",
            "private",
            "https://registry.example/index",
            "--repo-type",
            "http-registry",
            "--auth-type",
            "command",
            "--credential-command",
            "my-login",
        ],
        &[],
    );
    assert!(!refused.success);
    assert!(refused.stderr.contains("--user"), "{}", refused.stderr);

    let added = machine.run(
        &[
            "repo",
            "add",
            "private",
            "https://registry.example/index",
            "--repo-type",
            "http-registry",
            "--user",
            "--auth-type",
            "command",
            "--credential-command",
            "my-login",
            "--credential-arg",
            "token",
            // A value that starts with a dash needs the `=` form.
            "--credential-arg=--quiet",
        ],
        &[],
    );
    assert!(added.success, "{}", added.stderr);
    let saved = fs::read_to_string(machine.config_dir().join("repositories.toml")).unwrap();
    assert!(saved.contains("my-login"), "{saved}");
    assert!(saved.contains("--quiet"), "{saved}");
    let project = fs::read_to_string(machine.project().join("skill-project.toml")).unwrap();
    assert!(!project.contains("private"), "{project}");

    let listed = machine.run(&["repo", "list"], &[]);
    assert!(listed.success, "{}", listed.stderr);
    assert!(listed.stdout.contains("auth: command"), "{}", listed.stdout);
}

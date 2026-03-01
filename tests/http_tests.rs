//! Integration tests for HTTP/registry endpoints

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_borrows_for_generic_args,
    clippy::single_char_add_str,
    clippy::to_string_in_format_args,
    clippy::useless_vec
)]

use reqwest::multipart;
use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::net::TcpListener;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zip::write::FileOptions;

fn wait_for_port(port: u16, timeout_secs: u64) -> bool {
    let start = Instant::now();
    while start.elapsed().as_secs() < timeout_secs {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn get_free_port_or_skip() -> Option<u16> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            let port = listener.local_addr().expect("local addr").port();
            drop(listener);
            Some(port)
        }
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            eprintln!("Skipping test: unable to bind localhost socket ({err})");
            None
        }
        Err(err) => panic!("failed to bind localhost socket for test setup: {err}"),
    }
}

fn create_test_publish_zip() -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("SKILL.md", options).expect("start SKILL.md");
        zip.write_all(b"---\nname: demo-skill\ndescription: Demo\n---\n\n# Demo")
            .expect("write SKILL.md");

        zip.start_file("skill-project.toml", options)
            .expect("start skill-project.toml");
        zip.write_all(
            br#"[metadata]
id = "demo-skill"
version = "1.0.0"
name = "Demo Skill"
"#,
        )
        .expect("write skill-project.toml");

        zip.finish().expect("finish zip");
    }
    cursor.into_inner()
}

fn start_test_server(temp_dir: &TempDir, port: u16) -> Child {
    let project_toml = r#"[tool.fastskill]
skills_directory = ".skills"
"#;
    fs::create_dir_all(temp_dir.path().join(".skills")).expect("create skills dir");
    fs::write(temp_dir.path().join("skill-project.toml"), project_toml).expect("write project");

    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .current_dir(temp_dir.path())
        .spawn()
        .expect("start fastskill serve")
}

#[tokio::test]
async fn test_http_endpoints_are_accessible_without_authentication() {
    let temp_dir = TempDir::new().expect("temp dir");
    let Some(port) = get_free_port_or_skip() else {
        return;
    };
    let mut child = start_test_server(&temp_dir, port);

    assert!(
        wait_for_port(port, 10),
        "Server failed to start on port {}",
        port
    );

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let project_resp = client
        .get(format!("{base}/api/project"))
        .send()
        .await
        .expect("GET /api/project");
    assert_eq!(project_resp.status(), reqwest::StatusCode::OK);

    let skills_resp = client
        .get(format!("{base}/api/skills"))
        .send()
        .await
        .expect("GET /api/skills");
    assert_eq!(skills_resp.status(), reqwest::StatusCode::OK);

    let package_bytes = create_test_publish_zip();
    let form = multipart::Form::new().part(
        "package",
        multipart::Part::bytes(package_bytes)
            .file_name("demo-skill.zip")
            .mime_str("application/zip")
            .expect("zip mime"),
    );

    let publish_resp = client
        .post(format!("{base}/api/registry/publish"))
        .multipart(form)
        .send()
        .await
        .expect("POST /api/registry/publish");
    assert_ne!(
        publish_resp.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "publish should not require auth headers"
    );

    child.kill().expect("kill server");
    let _ = child.wait();
}

#[test]
fn test_marketplace_json_structure() {
    use fastskill::core::sources::{MarketplaceJson, MarketplaceSkill};

    let skill = MarketplaceSkill {
        id: "test-skill".to_string(),
        name: "Test Skill".to_string(),
        description: "A test skill".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Test Author".to_string()),
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
        download_url: None,
    };

    assert!(!valid.id.is_empty());
    assert!(!valid.name.is_empty());
    assert!(!valid.description.is_empty());
}

//! `fastskill managed` run end to end against an isolated home.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn without_settings_status_says_so_and_apply_fails() {
    let root = TempDir::new().unwrap();
    let status = run(root.path(), &["managed", "status"]);
    assert!(status.status.success(), "{}", text(&status.stderr));
    assert!(text(&status.stdout).contains("No managed source is configured."));

    let apply = run(root.path(), &["managed", "apply"]);
    assert!(!apply.status.success());
    let output = text(&apply.stdout) + &text(&apply.stderr);
    assert!(
        output.contains("no managed source is configured"),
        "{output}"
    );
}

#[test]
fn with_user_settings_enroll_status_and_unenroll() {
    let root = TempDir::new().unwrap();
    let settings = root
        .path()
        .join("config")
        .join("fastskill")
        .join("managed.toml");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let state = root.path().join("source").join("state.dsse");
    std::fs::write(
        &settings,
        format!(
            "source = {:?}\n\n[[keys]]\nid = \"k1\"\npublic_key = \
             \"BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=\"\n",
            state.display().to_string()
        ),
    )
    .unwrap();

    let status = run(root.path(), &["managed", "status"]);
    assert!(status.status.success(), "{}", text(&status.stderr));
    assert!(
        text(&status.stdout).contains("Not enrolled"),
        "{}",
        text(&status.stdout)
    );

    let enroll = run(root.path(), &["managed", "enroll"]);
    assert!(!enroll.status.success());
    let output = text(&enroll.stdout) + &text(&enroll.stderr);
    assert!(output.contains("can't read the managed state"), "{output}");

    let unenroll = run(root.path(), &["managed", "unenroll", "--json"]);
    assert!(unenroll.status.success(), "{}", text(&unenroll.stderr));
    let value: serde_json::Value = serde_json::from_slice(&unenroll.stdout).unwrap();
    assert!(value["removed_settings"].as_str().is_some(), "{value}");
    assert!(!settings.exists());
}

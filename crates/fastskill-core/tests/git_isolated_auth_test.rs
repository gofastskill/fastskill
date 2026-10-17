//! `clone_isolated` sends the caller's authorization with git's HTTP requests,
//! and nothing when it has none. A loopback listener records each request's
//! head and answers 404, so git stops after its first request.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fastskill_core::storage::git_isolated::{
    clone_isolated, CloneAuthorization, IsolatedCloneOptions,
};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// A listener on a loopback port that keeps every request head it is sent.
fn recorder() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let heads = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&heads);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            kept.lock().unwrap().push(head);
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    (base, heads)
}

async fn clone_heads(authorization: Option<CloneAuthorization>) -> Vec<String> {
    let (base, heads) = recorder();
    let parent = TempDir::new().unwrap();
    let result = clone_isolated(
        &format!("{base}/acme/skills.git"),
        None,
        &IsolatedCloneOptions {
            allowed_protocols: vec!["http".to_string()],
            parent_dir: Some(parent.path().to_path_buf()),
            authorization,
            ..IsolatedCloneOptions::default()
        },
    )
    .await;
    let error = result.expect_err("the listener serves no repository");
    assert!(!error.to_string().contains("c2VjcmV0"), "{error}");
    let heads = heads.lock().unwrap().clone();
    assert!(!heads.is_empty(), "git reached the listener");
    heads
}

#[tokio::test]
async fn the_authorization_is_sent_with_every_request() {
    let authorization = CloneAuthorization::new("Basic c2VjcmV0").unwrap();
    for head in clone_heads(Some(authorization)).await {
        assert!(head.starts_with("GET /acme/skills.git/info/refs"), "{head}");
        assert!(
            head.lines()
                .any(|line| line.eq_ignore_ascii_case("authorization: Basic c2VjcmV0")),
            "{head}"
        );
    }
}

#[tokio::test]
async fn without_an_authorization_none_is_sent() {
    for head in clone_heads(None).await {
        assert!(
            !head.to_ascii_lowercase().contains("authorization:"),
            "{head}"
        );
    }
}

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::core::managed::test_https::{Route, TestCa, TestServer};

fn source(server: &TestServer) -> ManagedSource {
    ManagedSource::parse(&server.url("/state")).unwrap()
}

#[cfg(unix)]
fn shell(script: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), script.to_string()]
}

#[cfg(unix)]
fn token(script: &str, interactive: bool) -> Result<Token, RemoteError> {
    run_credential_command(&shell(script), &std::env::temp_dir(), interactive)
}

#[test]
#[cfg(unix)]
fn the_token_is_the_first_line_and_goes_only_to_the_source_origin() {
    let ca = TestCa::trusted();
    let origin = TestServer::start(&ca);
    let other = TestServer::start(&ca);
    origin.route("/state", Route::Body(b"envelope".to_vec()));
    origin.route("/a.zip", Route::Body(b"same origin".to_vec()));
    other.route("/b.zip", Route::Body(b"other origin".to_vec()));
    let token = token("printf 'secret-token\\r\\nignored\\n'", true).unwrap();
    assert_eq!(format!("{token:?}"), "Token(..)");
    let remote = Remote::new(&source(&origin), Some(token)).unwrap();
    assert!(remote.has_token());

    assert_eq!(remote.get_state().unwrap(), b"envelope");
    let dir = tempfile::tempdir().unwrap();
    remote
        .download(&origin.url("/a.zip"), &dir.path().join("a"))
        .unwrap();
    remote
        .download(&other.url("/b.zip"), &dir.path().join("b"))
        .unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("b")).unwrap(),
        b"other origin"
    );

    let bearer = Some("Bearer secret-token".to_string());
    assert_eq!(origin.seen_at("/state")[0].authorization, bearer);
    assert_eq!(origin.seen_at("/a.zip")[0].authorization, bearer);
    assert_eq!(other.seen_at("/b.zip")[0].authorization, None);
}

#[test]
#[cfg(unix)]
fn a_redirect_to_another_origin_drops_the_token() {
    let ca = TestCa::trusted();
    let origin = TestServer::start(&ca);
    let other = TestServer::start(&ca);
    origin.route("/state", Route::Redirect(other.url("/moved")));
    other.route("/moved", Route::Body(b"envelope".to_vec()));
    let remote = Remote::new(&source(&origin), Some(token("echo t", false).unwrap())).unwrap();
    assert_eq!(remote.get_state().unwrap(), b"envelope");
    assert_eq!(
        origin.seen_at("/state")[0].authorization.as_deref(),
        Some("Bearer t")
    );
    assert_eq!(other.seen_at("/moved")[0].authorization, None);
}

#[test]
fn a_redirect_to_http_is_refused() {
    let ca = TestCa::trusted();
    let origin = TestServer::start(&ca);
    origin.route(
        "/state",
        Route::Redirect(origin.url("/plain").replace("https://", "http://")),
    );
    let error = Remote::new(&source(&origin), None)
        .unwrap()
        .get_state()
        .unwrap_err();
    assert!(!error.sign_in);
    assert!(
        error.message.contains("only https:// is followed"),
        "{error}"
    );
    assert!(origin.seen_at("/plain").is_empty());
}

#[test]
fn too_many_redirects_stop() {
    let ca = TestCa::trusted();
    let origin = TestServer::start(&ca);
    origin.route("/state", Route::Redirect(origin.url("/state")));
    let error = Remote::new(&source(&origin), None)
        .unwrap()
        .get_state()
        .unwrap_err();
    assert!(error.message.contains("too many redirects"), "{error}");
}

#[test]
fn a_401_means_sign_in_and_other_failures_do_not() {
    let ca = TestCa::trusted();
    let origin = TestServer::start(&ca);
    origin.route("/state", Route::Status(401));
    origin.route("/report", Route::Status(500));
    let remote = Remote::new(&source(&origin), None).unwrap();
    assert!(!remote.has_token());
    let error = remote.get_state().unwrap_err();
    assert!(error.sign_in);
    assert!(error.message.contains("401"), "{error}");
    let error = remote
        .post_json(&origin.url("/report"), b"{}".to_vec())
        .unwrap_err();
    assert!(!error.sign_in);
    assert!(error.message.contains("500"), "{error}");
    let seen = origin.seen_at("/report");
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].body, b"{}");
    let missing = remote
        .download(
            &origin.url("/missing.zip"),
            &std::env::temp_dir().join("never"),
        )
        .unwrap_err();
    assert!(missing.message.contains("404"), "{missing}");
}

#[test]
fn bodies_over_the_limit_are_refused() {
    let ca = TestCa::trusted();
    let origin = TestServer::start(&ca);
    origin.route("/big", Route::Body(vec![b'x'; 64]));
    let remote = Remote::new(&source(&origin), None).unwrap();
    let fetch = || {
        remote
            .send(
                remote.request(reqwest::Method::GET, &origin.url("/big")),
                "it",
            )
            .unwrap()
    };
    let error = read_limited(fetch(), 16, &mut Vec::new(), "the body").unwrap_err();
    assert!(error.message.contains("larger than"), "{error}");
    let mut body = Vec::new();
    read_limited(fetch(), 64, &mut body, "the body").unwrap();
    assert_eq!(body.len(), 64);
}

#[test]
fn unreachable_sources_and_file_sources_fail_without_sign_in() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let closed = ManagedSource::parse(&format!("https://localhost:{port}/state")).unwrap();
    let error = Remote::new(&closed, None).unwrap().get_state().unwrap_err();
    assert!(!error.sign_in);
    assert!(
        error.message.starts_with("can't fetch the managed state"),
        "{error}"
    );

    let file = ManagedSource::File(std::env::temp_dir().join("state.dsse"));
    let error = Remote::new(&file, None).unwrap().get_state().unwrap_err();
    assert!(error.message.contains("isn't https://"), "{error}");
    let service: crate::core::service::ServiceError = error.into();
    assert!(matches!(
        service,
        crate::core::service::ServiceError::InvalidOperation(_)
    ));
}

#[test]
fn a_credential_command_that_cannot_start_needs_sign_in() {
    let error = run_credential_command(&[], &std::env::temp_dir(), false).unwrap_err();
    assert!(error.sign_in);
    assert!(error.message.contains("empty"), "{error}");
    let missing = vec!["fastskill-no-such-credential-helper".to_string()];
    let error = run_credential_command(&missing, Path::new("/no/such/folder"), false).unwrap_err();
    assert!(error.sign_in);
    assert!(error.message.contains("couldn't start"), "{error}");
}

#[test]
#[cfg(unix)]
fn credential_command_failures_never_show_what_it_printed() {
    for (script, expected) in [
        ("echo leaked; exit 3", "exited with"),
        ("printf ''", "printed no token"),
        ("printf '\\n'", "printed no token"),
        ("head -c 20000 /dev/zero | tr '\\0' 'a'", "more than 16 KiB"),
        ("printf 'bad\\001token'", "can't be a token"),
    ] {
        let error = token(script, false).unwrap_err();
        assert!(error.sign_in, "{script}");
        assert!(error.message.contains(expected), "{script}: {error}");
        assert!(!error.message.contains("leaked"), "{error}");
    }
}

#[test]
#[cfg(unix)]
fn credential_command_output_over_the_limit_stops_a_command_that_keeps_running() {
    let started = Instant::now();
    let error = token("yes leaked", false).unwrap_err();
    assert!(error.message.contains("more than 16 KiB"), "{error}");
    assert!(started.elapsed() < CREDENTIAL_TIMEOUT);
}

#[test]
#[cfg(unix)]
fn the_credential_command_runs_in_the_configuration_folder_and_knows_when_no_one_can_answer() {
    let ca = TestCa::trusted();
    let origin = TestServer::start(&ca);
    origin.route("/state", Route::Body(Vec::new()));
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("marker"), "here").unwrap();
    let script = format!("cat marker; echo \"-${{{INTERACTIVE_ENV}:-unset}}\"");
    for (interactive, expected) in [(false, "Bearer here-0"), (true, "Bearer here-unset")] {
        let command = shell(&format!("printf '%s' \"$({script})\" | tr -d '\\n'"));
        let token = run_credential_command(&command, folder.path(), interactive).unwrap();
        let remote = Remote::new(&source(&origin), Some(token)).unwrap();
        remote.get_state().unwrap();
        let seen = origin.seen_at("/state");
        assert_eq!(
            seen.last().unwrap().authorization.as_deref(),
            Some(expected)
        );
    }
}

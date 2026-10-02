use super::*;
use std::time::Instant;

fn settings(dir: &std::path::Path) -> CommandSettings {
    CommandSettings {
        timeout: Duration::from_secs(10),
        working_dir: dir.to_path_buf(),
        stdin_is_terminal: false,
        stderr_is_terminal: false,
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

#[cfg(unix)]
fn sh(script: &str) -> Vec<String> {
    argv(&["sh", "-c", script])
}

#[cfg(unix)]
async fn run_sh(script: &str, settings: &CommandSettings) -> Result<SecretToken, ServiceError> {
    run_credential_command(&sh(script), settings).await
}

#[cfg(unix)]
#[tokio::test]
async fn token_is_the_first_line_with_trailing_cr_lf_trimmed() {
    let dir = tempfile::tempdir().unwrap();
    let token = run_sh(
        "printf 'tok-123\\r\\nsecond line\\n'",
        &settings(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(token.expose(), "tok-123");
    assert_eq!(format!("{token:?}"), "SecretToken(<redacted>)");
}

#[cfg(unix)]
#[tokio::test]
async fn non_zero_exit_names_program_and_status_but_never_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let error = run_sh(
        "echo leaked-secret-value; echo also-secret >&2; exit 3",
        &settings(dir.path()),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("'sh'"), "{error}");
    assert!(error.contains("exit status: 3"), "{error}");
    assert!(!error.contains("secret"), "{error}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_command_that_runs_too_long_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let mut short = settings(dir.path());
    short.timeout = Duration::from_millis(300);
    let started = Instant::now();
    let error = run_sh("sleep 20", &short).await.unwrap_err().to_string();
    assert!(error.contains("did not finish within"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[cfg(unix)]
#[tokio::test]
async fn empty_output_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    for script in ["true", "printf '\\n'", "printf '   \\r\\n'"] {
        let error = run_sh(script, &settings(dir.path()))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("printed no token"), "{script}: {error}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn more_than_16_kib_of_output_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let error = run_sh(
        "head -c 20000 /dev/zero | tr '\\0' a; sleep 5",
        &settings(dir.path()),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("more than 16 KiB"), "{error}");
    assert!(!error.contains("aaaa"), "{error}");
}

#[cfg(unix)]
#[tokio::test]
async fn exactly_16_kib_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let token = run_sh(
        "head -c 16384 /dev/zero | tr '\\0' a",
        &settings(dir.path()),
    )
    .await
    .unwrap();
    assert_eq!(token.expose().len(), MAX_COMMAND_OUTPUT);
}

#[cfg(unix)]
#[tokio::test]
async fn interactive_is_zero_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let script = "printf '%s' \"${FASTSKILL_INTERACTIVE:-unset}\"";
    let token = run_sh(script, &settings(dir.path())).await.unwrap();
    assert_eq!(token.expose(), "0");

    // A terminal on stdin alone is not enough: stderr must be one too.
    let mut stdin_only = settings(dir.path());
    stdin_only.stdin_is_terminal = true;
    let token = run_sh(script, &stdin_only).await.unwrap();
    assert_eq!(token.expose(), "0");

    if std::env::var_os(INTERACTIVE_ENV).is_none() {
        let mut both = settings(dir.path());
        both.stdin_is_terminal = true;
        both.stderr_is_terminal = true;
        let token = run_sh(script, &both).await.unwrap();
        assert_eq!(token.expose(), "unset");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn runs_without_a_shell_from_the_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("fastskill");
    // `pwd` runs directly; the directory is created when missing.
    let token = run_credential_command(&argv(&["pwd"]), &settings(&config_dir))
        .await
        .unwrap();
    assert_eq!(
        std::fs::canonicalize(token.expose()).unwrap(),
        std::fs::canonicalize(&config_dir).unwrap()
    );
    // No shell: a metacharacter reaches the program as a plain argument.
    let token = run_credential_command(&argv(&["echo", "a;b", "$HOME"]), &settings(&config_dir))
        .await
        .unwrap();
    assert_eq!(token.expose(), "a;b $HOME");
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_output_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let error = run_sh("printf '\\377\\376'", &settings(dir.path()))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("not valid UTF-8"), "{error}");
}

#[tokio::test]
async fn a_missing_program_or_empty_command_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let error = run_credential_command(
        &argv(&["fastskill-no-such-credential-helper"]),
        &settings(dir.path()),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("could not be started"), "{error}");
    let error = run_credential_command(&[], &settings(dir.path()))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("empty"), "{error}");
}

#[cfg(windows)]
#[tokio::test]
async fn windows_command_prints_a_token() {
    let dir = tempfile::tempdir().unwrap();
    let token = run_credential_command(&argv(&["cmd", "/C", "echo tok"]), &settings(dir.path()))
        .await
        .unwrap();
    assert_eq!(token.expose(), "tok");
}

#[test]
fn detect_uses_the_config_dir_and_a_30_second_timeout() {
    let _mutex = crate::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let previous = std::env::var_os("XDG_CONFIG_HOME");
    std::env::set_var("XDG_CONFIG_HOME", temp.path());
    let detected = CommandSettings::detect().unwrap();
    match previous {
        Some(previous) => std::env::set_var("XDG_CONFIG_HOME", previous),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }
    assert_eq!(detected.working_dir, temp.path().join("fastskill"));
    assert_eq!(detected.timeout, COMMAND_TIMEOUT);
}

use super::*;

fn url(text: &str) -> url::Url {
    url::Url::parse(text).unwrap()
}

#[test]
fn secret_token_debug_is_redacted() {
    let token = SecretToken::new("very-secret".to_string());
    let shown = format!("{token:?} {:?}", Some(token.clone()));
    assert!(!shown.contains("very-secret"), "{shown}");
    assert_eq!(token.expose(), "very-secret");
}

#[test]
fn bearer_header_is_sensitive_and_uses_the_bearer_scheme() {
    let header = bearer_header(&SecretToken::new("abc".to_string())).unwrap();
    assert!(header.is_sensitive());
    assert_eq!(header.to_str().unwrap(), "Bearer abc");
    assert!(!format!("{header:?}").contains("abc"));
    assert!(bearer_header(&SecretToken::new("bad\nvalue".to_string())).is_none());
}

#[test]
fn from_config_picks_only_the_bearer_types() {
    assert_eq!(
        Credential::from_config(&AuthConfig::Bearer {
            env_var: "T".to_string()
        }),
        Some(Credential::Env {
            env_var: "T".to_string()
        })
    );
    assert_eq!(
        Credential::from_config(&AuthConfig::Command {
            command: vec!["helper".to_string()]
        }),
        Some(Credential::Command {
            argv: vec!["helper".to_string()]
        })
    );
    assert_eq!(
        Credential::from_config(&AuthConfig::Pat {
            env_var: "T".to_string()
        }),
        None
    );
}

#[tokio::test]
async fn bearer_env_var_supplies_the_token() {
    std::env::set_var("FASTSKILL_TEST_BEARER_PRESENT", "env-token");
    let credential = Credential::Env {
        env_var: "FASTSKILL_TEST_BEARER_PRESENT".to_string(),
    };
    let header = credential
        .header("private", "https://r.example")
        .await
        .unwrap();
    assert_eq!(header.to_str().unwrap(), "Bearer env-token");
}

#[tokio::test]
async fn a_missing_or_empty_bearer_env_var_is_an_error() {
    let credential = Credential::Env {
        env_var: "FASTSKILL_TEST_BEARER_MISSING".to_string(),
    };
    let error = credential
        .header("private", "https://r.example")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("FASTSKILL_TEST_BEARER_MISSING"), "{error}");
    assert!(error.contains("'private'"), "{error}");

    std::env::set_var("FASTSKILL_TEST_BEARER_EMPTY", "  ");
    let empty = Credential::Env {
        env_var: "FASTSKILL_TEST_BEARER_EMPTY".to_string(),
    };
    assert!(empty.header("private", "https://r.example").await.is_err());
}

#[test]
fn https_to_http_redirect_is_refused() {
    let registry = url("https://registry.example/index").origin();
    let decision = redirect_decision(
        &url("https://registry.example/dl/a"),
        &url("http://registry.example/dl/a"),
        &registry,
    );
    assert!(matches!(decision, RedirectDecision::Refuse(ref m) if m.contains("https to http")));
    // Even toward another origin, where no token would be sent.
    let decision = redirect_decision(
        &url("https://registry.example/dl/a"),
        &url("http://cdn.example/a"),
        &registry,
    );
    assert!(matches!(decision, RedirectDecision::Refuse(_)));
}

#[test]
fn cross_origin_redirect_drops_the_token() {
    let registry = url("https://registry.example/index").origin();
    let from = url("https://registry.example/dl/a");
    assert_eq!(
        redirect_decision(&from, &url("https://cdn.example/a"), &registry),
        RedirectDecision::Follow { send_token: false }
    );
    // Another port is another origin.
    assert_eq!(
        redirect_decision(&from, &url("https://registry.example:8443/a"), &registry),
        RedirectDecision::Follow { send_token: false }
    );
    assert_eq!(
        redirect_decision(&from, &url("https://registry.example/other"), &registry),
        RedirectDecision::Follow { send_token: true }
    );
    // http to https on the registry host is a different origin too.
    let plain = url("http://registry.example/index").origin();
    assert_eq!(
        redirect_decision(
            &url("http://registry.example/a"),
            &url("https://registry.example/a"),
            &plain
        ),
        RedirectDecision::Follow { send_token: false }
    );
}

#[test]
fn redirect_to_another_scheme_is_refused() {
    let registry = url("https://registry.example/index").origin();
    let decision = redirect_decision(
        &url("https://registry.example/a"),
        &url("file:///etc/passwd"),
        &registry,
    );
    assert!(matches!(decision, RedirectDecision::Refuse(_)));
}

#[test]
fn one_cell_per_repository_and_command() {
    let key = |repo: &str| {
        (
            repo.to_string(),
            "https://r.example".to_string(),
            vec!["helper".to_string()],
        )
    };
    let first = token_cell(key("cell-a"));
    let again = token_cell(key("cell-a"));
    let other = token_cell(key("cell-b"));
    assert!(Arc::ptr_eq(&first, &again));
    assert!(!Arc::ptr_eq(&first, &other));
}

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::config::{ManagedSettings, ManagedSource, PinnedKey};
use super::envelope::{pae, verify_envelope, PAYLOAD_TYPE};
use super::state::{Editable, ManagedState, Recorded};
use super::{open, ManagedError};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use std::path::Path;

const SOURCE: &str = "https://skills.example.com/state";

fn digest(fill: char) -> String {
    format!("sha256-tree-v2:{}", fill.to_string().repeat(64))
}

fn key_pair(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

fn pinned(id: &str, seed: u8) -> PinnedKey {
    PinnedKey {
        id: id.to_string(),
        public_key: key_pair(seed).public_key().as_ref().try_into().unwrap(),
    }
}

fn now() -> DateTime<Utc> {
    "2026-10-09T12:00:00Z".parse().unwrap()
}

fn state_json() -> Value {
    json!({
        "format_version": 1,
        "issued_at": "2026-10-09T11:00:00Z",
        "expires_at": "2026-10-16T11:00:00Z",
        "source": SOURCE,
        "subject": "team-a",
        "skills": [
            { "id": "pdf", "digest": digest('a'), "artifact": "https://skills.example.com/a.zip" }
        ],
        "allowed": "listed",
        "also_allowed": [digest('b')],
        "blocked": [{ "digest": digest('c'), "message": "withdrawn" }],
        "a_future_field": true
    })
}

fn sign(payload: &[u8], signers: &[(&str, u8)]) -> Vec<u8> {
    let message = pae(PAYLOAD_TYPE, payload);
    let signatures: Vec<Value> = signers
        .iter()
        .map(|(id, seed)| {
            json!({ "keyid": id, "sig": STANDARD.encode(key_pair(*seed).sign(&message)) })
        })
        .collect();
    serde_json::to_vec(&json!({
        "payloadType": PAYLOAD_TYPE,
        "payload": STANDARD.encode(payload),
        "signatures": signatures,
    }))
    .unwrap()
}

fn settings() -> ManagedSettings {
    ManagedSettings {
        source: Some(ManagedSource::parse(SOURCE).unwrap()),
        keys: vec![pinned("k1", 1)],
        ..ManagedSettings::default()
    }
}

fn open_state(state: &Value, recorded: &Recorded) -> Result<ManagedState, ManagedError> {
    let envelope = sign(&serde_json::to_vec(state).unwrap(), &[("k1", 1)]);
    open(&envelope, &settings(), recorded, now()).map(|opened| opened.state)
}

fn refused(state: Value) -> ManagedError {
    open_state(&state, &Recorded::default()).unwrap_err()
}

#[test]
fn pae_matches_the_dsse_example() {
    assert_eq!(
        pae("http://example.com/HelloWorld", b"hello world"),
        b"DSSEv1 29 http://example.com/HelloWorld 11 hello world".to_vec()
    );
}

#[test]
fn a_signed_state_opens_and_unknown_fields_are_ignored() {
    let state = open_state(&state_json(), &Recorded::default()).unwrap();
    assert_eq!(state.subject, "team-a");
    assert!(state.allows(&digest('a')));
    assert!(state.allows(&digest('b')));
    assert!(!state.allows(&digest('c')));
    assert!(!state.allows(&digest('d')));
    assert_eq!(
        state.blocked(&digest('c')).unwrap().message.as_deref(),
        Some("withdrawn")
    );
    assert!(!state.is_expired(now()));
    assert!(state.is_expired("2026-10-16T11:00:00Z".parse().unwrap()));
}

#[test]
fn a_signature_by_an_unpinned_key_is_skipped_and_a_pinned_one_accepted() {
    let payload = serde_json::to_vec(&state_json()).unwrap();
    let envelope = sign(&payload, &[("new", 9), ("k1", 1)]);
    let verified = verify_envelope(&envelope, &[pinned("k1", 1)]).unwrap();
    assert_eq!(verified.key_id, "k1");
    assert_eq!(verified.payload, payload);
}

#[test]
fn url_safe_unpadded_base64_is_read() {
    let payload = serde_json::to_vec(&state_json()).unwrap();
    let message = pae(PAYLOAD_TYPE, &payload);
    let envelope = serde_json::to_vec(&json!({
        "payloadType": PAYLOAD_TYPE,
        "payload": URL_SAFE_NO_PAD.encode(&payload),
        "signatures": [{ "keyid": "k1", "sig": URL_SAFE_NO_PAD.encode(key_pair(1).sign(&message)) }],
    }))
    .unwrap();
    verify_envelope(&envelope, &[pinned("k1", 1)]).unwrap();
}

#[test]
fn envelopes_that_dont_verify_are_untrusted() {
    let payload = serde_json::to_vec(&state_json()).unwrap();
    // Signed by another key under the pinned id.
    let forged = sign(&payload, &[("k1", 2)]);
    let error = verify_envelope(&forged, &[pinned("k1", 1)]).unwrap_err();
    assert!(
        matches!(&error, ManagedError::Untrusted(m) if m.contains("doesn't verify")),
        "{error}"
    );
    // Signed only by unpinned keys.
    let other = sign(&payload, &[("k9", 1)]);
    let error = verify_envelope(&other, &[pinned("k1", 1)]).unwrap_err();
    assert!(
        matches!(&error, ManagedError::Untrusted(m) if m.contains("no signature")),
        "{error}"
    );
    // No keys pinned at all.
    let signed = sign(&payload, &[("k1", 1)]);
    assert!(matches!(
        verify_envelope(&signed, &[]),
        Err(ManagedError::Untrusted(_))
    ));
    // A payload changed after signing.
    let mut envelope: Value = serde_json::from_slice(&signed).unwrap();
    let mut changed = state_json();
    changed["subject"] = json!("team-b");
    envelope["payload"] = json!(STANDARD.encode(serde_json::to_vec(&changed).unwrap()));
    let tampered = serde_json::to_vec(&envelope).unwrap();
    assert!(matches!(
        verify_envelope(&tampered, &[pinned("k1", 1)]),
        Err(ManagedError::Untrusted(_))
    ));
}

#[test]
fn malformed_envelopes_are_refused() {
    let keys = [pinned("k1", 1)];
    assert!(matches!(
        verify_envelope(b"not json", &keys),
        Err(ManagedError::Envelope(_))
    ));
    let wrong_type = serde_json::to_vec(&json!({
        "payloadType": "application/json", "payload": "e30", "signatures": []
    }))
    .unwrap();
    assert!(matches!(
        verify_envelope(&wrong_type, &keys),
        Err(ManagedError::Envelope(_))
    ));
    let bad_payload = serde_json::to_vec(&json!({
        "payloadType": PAYLOAD_TYPE, "payload": "%%%", "signatures": []
    }))
    .unwrap();
    assert!(matches!(
        verify_envelope(&bad_payload, &keys),
        Err(ManagedError::Envelope(_))
    ));
    let huge = vec![b' '; super::envelope::MAX_ENVELOPE_BYTES + 1];
    assert!(matches!(
        verify_envelope(&huge, &keys),
        Err(ManagedError::Envelope(_))
    ));
}

#[test]
fn an_unknown_format_version_is_refused_first() {
    let error = refused(json!({ "format_version": 2, "anything": "else" }));
    assert!(
        matches!(&error, ManagedError::Invalid(m) if m.contains("format version 2")),
        "{error}"
    );
}

type Change = Box<dyn Fn(&mut Value)>;

#[test]
fn whole_state_rules_refuse_it() {
    let cases: Vec<(&str, Change)> = vec![
        (
            "scoped id",
            Box::new(|s| s["skills"][0]["id"] = json!("team/pdf")),
        ),
        (
            "traversal id",
            Box::new(|s| s["skills"][0]["id"] = json!("..")),
        ),
        (
            "duplicate id",
            Box::new(|s| {
                let skill = s["skills"][0].clone();
                s["skills"].as_array_mut().unwrap().push(skill);
            }),
        ),
        (
            "legacy digest",
            Box::new(|s| s["skills"][0]["digest"] = json!("a".repeat(64))),
        ),
        (
            "bad also_allowed",
            Box::new(|s| s["also_allowed"][0] = json!("sha256:xyz")),
        ),
        (
            "blocked and listed",
            Box::new(|s| s["blocked"][0]["digest"] = json!(digest('a'))),
        ),
        (
            "http artifact",
            Box::new(|s| s["skills"][0]["artifact"] = json!("http://x.example/a.zip")),
        ),
        (
            "local artifact",
            Box::new(|s| s["skills"][0]["artifact"] = json!("/srv/a.zip")),
        ),
        (
            "http report_url",
            Box::new(|s| s["report_url"] = json!("http://skills.example.com/r")),
        ),
        (
            "no placeholder",
            Box::new(|s| s["request_url"] = json!("https://skills.example.com/r")),
        ),
        (
            "two placeholders",
            Box::new(|s| s["request_url"] = json!("https://skills.example.com/{digest}/{digest}")),
        ),
        (
            "unknown editable",
            Box::new(|s| s["editable"] = json!("sometimes")),
        ),
        (
            "unknown allowed",
            Box::new(|s| s["allowed"] = json!("some")),
        ),
    ];
    for (name, change) in cases {
        let mut state = state_json();
        change(&mut state);
        let error = refused(state);
        assert!(matches!(error, ManagedError::Invalid(_)), "{name}: {error}");
    }
}

#[test]
fn a_file_source_may_name_local_artifacts_but_no_other_scheme() {
    let source = ManagedSource::parse("/srv/managed/state.json").unwrap();
    let mut state: ManagedState = serde_json::from_value(state_json()).unwrap();
    state.skills[0].artifact = "/srv/managed/a.zip".to_string();
    state.validate(&source).unwrap();
    state.skills[0].artifact = "file:///srv/managed/a.zip".to_string();
    assert!(state.validate(&source).is_err());
}

#[test]
fn binding_refuses_another_source_subject_rollback_and_the_future() {
    let mut other_source = state_json();
    other_source["source"] = json!("https://elsewhere.example.com/state");
    assert!(matches!(refused(other_source), ManagedError::Binding(_)));

    let recorded = Recorded {
        subject: Some("team-b".to_string()),
        highest_issued_at: None,
    };
    let error = open_state(&state_json(), &recorded).unwrap_err();
    assert!(
        matches!(&error, ManagedError::Binding(m) if m.contains("enroll")),
        "{error}"
    );

    let recorded = Recorded {
        subject: Some("team-a".to_string()),
        highest_issued_at: Some("2026-10-09T11:30:00Z".parse().unwrap()),
    };
    assert!(matches!(
        open_state(&state_json(), &recorded),
        Err(ManagedError::Binding(_))
    ));

    let mut ahead = state_json();
    ahead["issued_at"] = json!((now() + Duration::minutes(6)).to_rfc3339());
    assert!(matches!(refused(ahead), ManagedError::Binding(_)));
    let mut within_skew = state_json();
    within_skew["issued_at"] = json!((now() + Duration::minutes(4)).to_rfc3339());
    open_state(&within_skew, &Recorded::default()).unwrap();
}

#[test]
fn the_same_issued_at_is_accepted_again_and_recorded() {
    let state = open_state(&state_json(), &Recorded::default()).unwrap();
    let recorded = Recorded::default().accept(&state);
    assert_eq!(recorded.subject.as_deref(), Some("team-a"));
    open_state(&state_json(), &recorded).unwrap();
}

#[test]
fn https_only_features_need_an_https_source_and_its_origin() {
    let mut value = state_json();
    value["editable"] = json!("refused");
    value["report_url"] = json!("https://skills.example.com/report");
    value["request_url"] = json!("https://skills.example.com/request?digest={digest}");
    let state = open_state(&value, &Recorded::default()).unwrap();
    let https = ManagedSource::parse(SOURCE).unwrap();
    assert_eq!(state.editable(&https), Editable::Refused);
    assert_eq!(
        state.report_url(&https),
        Some("https://skills.example.com/report")
    );
    assert_eq!(
        state.request_link(&https, &digest('d')).unwrap(),
        format!(
            "https://skills.example.com/request?digest=sha256-tree-v2%3A{}",
            "d".repeat(64)
        )
    );
    let file = ManagedSource::parse("/srv/state.json").unwrap();
    assert_eq!(state.editable(&file), Editable::BlockedOnly);
    assert_eq!(state.report_url(&file), None);
    assert_eq!(state.request_link(&file, &digest('d')), None);

    let mut cross = state.clone();
    cross.report_url = Some("https://other.example.com/report".to_string());
    cross.request_url = Some("https://other.example.com/{digest}".to_string());
    assert_eq!(cross.report_url(&https), None);
    assert_eq!(cross.request_link(&https, &digest('d')), None);
}

#[test]
fn sources_parse_https_or_absolute_paths_only() {
    assert!(matches!(
        ManagedSource::parse(SOURCE),
        Ok(ManagedSource::Https(_))
    ));
    assert!(matches!(
        ManagedSource::parse("/srv/state.json"),
        Ok(ManagedSource::File(_))
    ));
    for bad in [
        "http://skills.example.com/state",
        "file:///srv/state.json",
        "state.json",
        "",
    ] {
        assert!(ManagedSource::parse(bad).is_err(), "{bad}");
    }
    let https = ManagedSource::parse(SOURCE).unwrap();
    assert!(https.matches("https://SKILLS.example.com:443/state"));
    assert!(!https.matches("https://skills.example.com/other"));
}

fn write(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn key_toml(id: &str, seed: u8) -> String {
    format!(
        "[[keys]]\nid = \"{id}\"\npublic_key = \"{}\"\n",
        STANDARD.encode(pinned(id, seed).public_key)
    )
}

#[test]
fn the_system_file_overrides_the_user_file_setting_by_setting() {
    let dir = tempfile::tempdir().unwrap();
    let system = write(
        dir.path(),
        "system.toml",
        &format!(
            "source = \"{SOURCE}\"\nrequired = true\n{}",
            key_toml("sys", 1)
        ),
    );
    let user = write(
        dir.path(),
        "user.toml",
        &format!(
            "source = \"/srv/state.json\"\nrequired = false\ntargets = [\"claude\"]\n\
             credential_command = [\"login\", \"token\"]\n{}",
            key_toml("user", 2)
        ),
    );
    let settings = ManagedSettings::load_from(Some(&system), Some(&user), |_| Ok(())).unwrap();
    assert_eq!(settings.source, Some(ManagedSource::parse(SOURCE).unwrap()));
    assert!(settings.required && settings.required_by_system);
    assert_eq!(settings.keys, vec![pinned("sys", 1)]);
    assert_eq!(settings.targets, Some(vec!["claude".to_string()]));
    assert_eq!(
        settings.credential_command,
        Some(vec!["login".to_string(), "token".to_string()])
    );
    assert_eq!(settings.files, vec![system, user]);
}

#[test]
fn an_untrusted_system_file_is_refused_not_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let system = write(dir.path(), "system.toml", "required = false\n");
    let error = ManagedSettings::load_from(Some(&system), None, |_| Err("not root".to_string()))
        .unwrap_err();
    assert_eq!(error, ManagedError::Config("not root".to_string()));
}

#[test]
fn missing_files_mean_no_managed_source() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent.toml");
    let settings =
        ManagedSettings::load_from(Some(&absent), Some(&absent), |_| panic!("not checked"))
            .unwrap();
    assert!(!settings.is_configured());
}

#[test]
fn settings_files_that_cant_be_used_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        format!("source = \"{SOURCE}\"\n"),
        format!(
            "source = \"{SOURCE}\"\n{}{}",
            key_toml("k", 1),
            key_toml("k", 2)
        ),
        format!("source = \"{SOURCE}\"\n[[keys]]\nid = \"k\"\npublic_key = \"c2hvcnQ=\"\n"),
        format!("source = \"{SOURCE}\"\n{}unknown = 1\n", key_toml("k", 1)),
        format!(
            "source = \"{SOURCE}\"\ncredential_command = []\n{}",
            key_toml("k", 1)
        ),
        "required = true\n".to_string(),
        format!("source = \"http://x.example/state\"\n{}", key_toml("k", 1)),
    ];
    for content in cases {
        let user = write(dir.path(), "user.toml", &content);
        let error = ManagedSettings::load_from(None, Some(&user), |_| Ok(())).unwrap_err();
        assert!(
            matches!(error, ManagedError::Config(_)),
            "{content}: {error}"
        );
    }
}

#[cfg(unix)]
#[test]
fn the_system_file_check_refuses_files_others_can_change() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "managed.toml", "");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    // A file owned by the test user isn't owned by root (tests don't run as root in CI).
    if nix_is_root() {
        return;
    }
    let error = super::config::check_system_file(&file).unwrap_err();
    assert!(error.contains("owned by root"), "{error}");
    let link = dir.path().join("link.toml");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let error = super::config::check_system_file(&link).unwrap_err();
    assert!(error.contains("regular file"), "{error}");
}

#[cfg(unix)]
fn nix_is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
}

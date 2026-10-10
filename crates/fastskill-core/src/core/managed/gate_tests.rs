#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::core::managed::records::Enrollment;
use crate::core::managed::tests::{pinned, sign};
use serde_json::{json, Value};
use std::path::PathBuf;

fn now() -> DateTime<Utc> {
    "2026-10-09T12:00:00Z".parse().unwrap()
}

/// A skill folder with a body, and its digest.
fn skill(dir: &Path, id: &str, body: &str) -> (PathBuf, String) {
    let folder = dir.join(id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: test\n---\n{body}\n"),
    )
    .unwrap();
    let digest = content_digest(&folder).unwrap();
    (folder, digest)
}

fn other(fill: char) -> String {
    format!("sha256-tree-v2:{}", fill.to_string().repeat(64))
}

fn state_json(source: &str, listed: &str, blocked: &str) -> Value {
    json!({
        "format_version": 1,
        "issued_at": "2026-10-09T11:00:00Z",
        "expires_at": "2026-10-16T11:00:00Z",
        "source": source,
        "subject": "team-a",
        "skills": [{ "id": "pdf", "digest": listed, "artifact": "pdf.zip" }],
        "allowed": "listed",
        "blocked": [
            { "digest": blocked, "message": "withdrawn" },
            { "digest": other('e') }
        ],
    })
}

fn situation(value: Value, source: ManagedSource, expired: bool) -> Situation {
    Situation::State {
        state: Box::new(ManagedState::parse(&serde_json::to_vec(&value).unwrap()).unwrap()),
        source,
        expired,
    }
}

fn file_source() -> ManagedSource {
    ManagedSource::File(std::env::temp_dir().join("state.dsse"))
}

fn https() -> ManagedSource {
    ManagedSource::parse("https://skills.example.com/state").unwrap()
}

fn error(result: Result<(), ServiceError>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn without_a_source_everything_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, _) = skill(dir.path(), "pdf", "one");
    let situation = Situation::NotConfigured;
    assert!(situation.check(Candidate::digest("pdf", "x")).is_ok());
    assert_eq!(situation.status_of(&folder, false).unwrap(), None);
    assert_eq!(situation.command_policy(), CommandPolicy::Run);
}

#[test]
fn listed_blocked_and_unlisted_content() {
    let dir = tempfile::tempdir().unwrap();
    let (listed, listed_digest) = skill(dir.path(), "pdf", "one");
    let (blocked, blocked_digest) = skill(dir.path(), "bad", "two");
    let (unlisted, unlisted_digest) = skill(dir.path(), "docx", "three");
    let source = file_source();
    let value = state_json(source.as_str(), &listed_digest, &blocked_digest);
    let situation = situation(value, source, false);

    assert!(situation
        .check(Candidate::digest("pdf", &listed_digest))
        .is_ok());
    let message = error(situation.check(Candidate::digest("bad", &blocked_digest)));
    assert!(
        message.contains("is blocked by the managed state: withdrawn"),
        "{message}"
    );
    let message = error(situation.check(Candidate::digest("e", &other('e'))));
    assert!(
        message.ends_with("is blocked by the managed state"),
        "{message}"
    );
    let message = error(situation.check(Candidate::digest("docx", &unlisted_digest)));
    assert!(
        message.contains("isn't allowed by the managed state"),
        "{message}"
    );
    assert!(!message.contains("request"), "{message}");

    assert_eq!(situation.status_of(&listed, false).unwrap(), None);
    assert_eq!(
        situation.status_of(&blocked, false).unwrap(),
        Some(ManagedSkillStatus::Blocked)
    );
    assert_eq!(
        situation.status_of(&unlisted, false).unwrap(),
        Some(ManagedSkillStatus::NotAllowed)
    );
    assert_eq!(situation.command_policy(), CommandPolicy::Run);
}

#[test]
fn a_candidate_digest_is_computed_from_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (blocked, blocked_digest) = skill(dir.path(), "bad", "two");
    let source = file_source();
    let value = state_json(source.as_str(), &other('a'), &blocked_digest);
    let situation = situation(value, source, false);
    let candidate = Candidate {
        id: "bad",
        digest: None,
        path: Some(&blocked),
        editable: false,
    };
    assert!(error(situation.check(candidate)).contains("blocked"));
    let nothing = Candidate {
        path: None,
        ..candidate
    };
    assert!(error(situation.check(nothing)).contains("no content digest"));
    let gone = Candidate {
        path: Some(&dir.path().join("gone")),
        ..candidate
    };
    assert!(situation.check(gone).is_err());
}

#[test]
fn editable_skills_skip_the_list_but_never_the_block() {
    let dir = tempfile::tempdir().unwrap();
    let (editable, editable_digest) = skill(dir.path(), "mine", "one");
    let (blocked, blocked_digest) = skill(dir.path(), "bad", "two");
    let source = file_source();
    let mut value = state_json(source.as_str(), &other('a'), &blocked_digest);
    // A file source ignores `refused`.
    value["editable"] = json!("refused");
    let situation = situation(value, source, false);
    let candidate = |id, path| Candidate {
        id,
        digest: None,
        path: Some(path),
        editable: true,
    };
    assert!(situation.check(candidate("mine", &editable)).is_ok());
    assert!(error(situation.check(candidate("bad", &blocked))).contains("blocked"));
    assert_eq!(situation.status_of(&editable, true).unwrap(), None);
    assert_eq!(
        situation.status_of(&blocked, true).unwrap(),
        Some(ManagedSkillStatus::Blocked)
    );
    assert!(!editable_digest.is_empty());
}

#[test]
fn an_https_source_may_refuse_editable_skills_and_link_requests() {
    let dir = tempfile::tempdir().unwrap();
    let (editable, _) = skill(dir.path(), "mine", "one");
    let (_, unlisted_digest) = skill(dir.path(), "docx", "three");
    let source = https();
    let mut value = state_json(source.as_str(), &other('a'), &other('c'));
    value["skills"][0]["artifact"] = json!("https://skills.example.com/pdf.zip");
    value["editable"] = json!("refused");
    value["request_url"] = json!("https://skills.example.com/request?d={digest}");
    let situation = situation(value, source, false);

    let candidate = Candidate {
        id: "mine",
        digest: None,
        path: Some(&editable),
        editable: true,
    };
    assert!(error(situation.check(candidate)).contains("refuses those"));
    assert_eq!(
        situation.status_of(&editable, true).unwrap(),
        Some(ManagedSkillStatus::NotAllowed)
    );
    let message = error(situation.check(Candidate::digest("docx", &unlisted_digest)));
    assert!(
        message.contains("request it at https://skills.example.com/request?d=sha256-tree-v2"),
        "{message}"
    );
}

#[test]
fn allowing_any_content_refuses_only_the_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let (unlisted, unlisted_digest) = skill(dir.path(), "docx", "three");
    let source = file_source();
    let mut value = state_json(source.as_str(), &other('a'), &other('c'));
    value["allowed"] = json!("any");
    let situation = situation(value, source, false);
    assert!(situation
        .check(Candidate::digest("docx", &unlisted_digest))
        .is_ok());
    assert_eq!(situation.status_of(&unlisted, false).unwrap(), None);
}

#[test]
fn an_expired_state_refuses_changes_and_warns_readers() {
    let source = file_source();
    let value = state_json(source.as_str(), &other('a'), &other('c'));
    let situation = situation(value, source, true);
    let message = error(situation.check(Candidate::digest("pdf", &other('a'))));
    assert!(message.contains("expired at"), "{message}");
    let CommandPolicy::Warn(warning) = situation.command_policy() else {
        panic!("expected a warning");
    };
    assert!(warning.contains("expired"), "{warning}");
}

#[test]
fn without_a_valid_state_required_refuses_and_otherwise_warns() {
    let required = Situation::NoState {
        required: true,
        problem: "nothing cached".to_string(),
    };
    assert!(error(required.check(Candidate::digest("pdf", "x"))).contains("is required"));
    assert!(
        matches!(required.command_policy(), CommandPolicy::Refuse(text) if text.contains("nothing cached"))
    );

    let optional = Situation::NoState {
        required: false,
        problem: "nothing cached".to_string(),
    };
    assert!(optional.check(Candidate::digest("pdf", "x")).is_ok());
    assert!(
        matches!(optional.command_policy(), CommandPolicy::Warn(text) if text.contains("nothing cached"))
    );

    let unusable = Situation::Unusable("owned by someone else".to_string());
    assert!(error(unusable.check(Candidate::digest("pdf", "x"))).contains("can't be used"));
    assert!(matches!(
        unusable.command_policy(),
        CommandPolicy::Refuse(_)
    ));
}

struct Store {
    dir: tempfile::TempDir,
    layout: ManagedLayout,
    state_file: PathBuf,
}

impl Store {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let layout = ManagedLayout::new(dir.path().join("data"));
        let state_file = dir.path().join("state.dsse");
        Self {
            dir,
            layout,
            state_file,
        }
    }

    fn settings(&self, from_system: bool) -> ManagedSettings {
        ManagedSettings {
            source: Some(ManagedSource::File(self.state_file.clone())),
            keys: vec![pinned("k1", 1)],
            source_from_system: from_system,
            ..ManagedSettings::default()
        }
    }

    fn enroll(&self) {
        std::fs::create_dir_all(self.layout.managed()).unwrap();
        let enrollment = Enrollment::new(&self.state_file.display().to_string(), now());
        std::fs::write(
            self.layout.enrollment_file(),
            serde_json::to_vec(&enrollment).unwrap(),
        )
        .unwrap();
    }

    fn cache(&self, expires_at: &str) {
        let mut value = state_json(
            &self.state_file.display().to_string(),
            &other('a'),
            &other('c'),
        );
        value["expires_at"] = json!(expires_at);
        let envelope = sign(&serde_json::to_vec(&value).unwrap(), &[("k1", 1)]);
        std::fs::write(self.layout.cached_state(), envelope).unwrap();
    }

    fn load(&self, settings: ManagedSettings) -> Situation {
        Situation::load(&self.layout, Ok(settings), now())
    }
}

#[test]
fn loading_reads_the_enrollment_and_the_cached_state() {
    let store = Store::new();
    assert_eq!(
        Situation::load(&store.layout, Ok(ManagedSettings::default()), now()),
        Situation::NotConfigured
    );
    let unusable = Situation::load(
        &store.layout,
        Err(ManagedError::Config("bad".to_string())),
        now(),
    );
    assert!(matches!(unusable, Situation::Unusable(text) if text.contains("bad")));

    let Situation::NoState { problem, .. } = store.load(store.settings(false)) else {
        panic!("expected no state");
    };
    assert!(problem.contains("managed enroll"), "{problem}");
    let Situation::NoState { problem, .. } = store.load(store.settings(true)) else {
        panic!("expected no state");
    };
    assert!(problem.contains("managed apply"), "{problem}");

    store.enroll();
    let mut required = store.settings(true);
    required.required = true;
    let Situation::NoState { required, problem } = store.load(required) else {
        panic!("expected no state");
    };
    assert!(required);
    assert!(
        problem.contains("no state has been accepted yet"),
        "{problem}"
    );

    store.cache("2026-10-16T11:00:00Z");
    assert!(matches!(
        store.load(store.settings(true)),
        Situation::State { expired: false, .. }
    ));
    store.cache("2026-10-09T11:30:00Z");
    assert!(matches!(
        store.load(store.settings(true)),
        Situation::State { expired: true, .. }
    ));

    std::fs::write(store.layout.enrollment_file(), "not json").unwrap();
    assert!(matches!(
        store.load(store.settings(true)),
        Situation::NoState { .. }
    ));
    drop(store.dir);
}

#[test]
fn a_fixed_gate_checks_against_its_situation() {
    let gate = ManagedGate::fixed(Situation::Unusable("broken".to_string()));
    assert!(gate.check(Candidate::digest("pdf", "x")).is_err());
    assert!(gate.check_all([Candidate::digest("pdf", "x")]).is_err());
    assert!(ManagedGate::fixed(Situation::NotConfigured)
        .check_all([Candidate::digest("pdf", "x")])
        .is_ok());
    // The default gate reads this user's situation; a test machine has no managed source.
    let current = ManagedGate::default().situation();
    assert!(matches!(
        *current,
        Situation::NotConfigured | Situation::NoState { .. } | Situation::State { .. }
    ));
}

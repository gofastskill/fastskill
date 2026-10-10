#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::core::managed::apply::ApplyContext;
use crate::core::managed::apply_tests::{now, write_skill, Fixture};
use crate::core::managed::enrollment::status;
use crate::core::managed::gate::{Candidate, Situation};
use crate::core::managed::state::StateSkill;
use crate::core::managed::test_https::{Route, TestCa, TestServer};
use crate::core::managed::tests::sign;
use serde_json::{json, Value};

/// A managed source served over https by a local test server.
struct Https {
    fixture: Fixture,
    server: TestServer,
    _ca: TestCa,
}

impl Https {
    fn new() -> Self {
        let ca = TestCa::trusted();
        let server = TestServer::start(&ca);
        server.route("/report", Route::Body(Vec::new()));
        Self {
            fixture: Fixture::new(),
            server,
            _ca: ca,
        }
    }

    fn source(&self) -> ManagedSource {
        ManagedSource::parse(&self.server.url("/state")).unwrap()
    }

    fn context(&self) -> ApplyContext {
        let mut context = self.fixture.context();
        context.settings.source = Some(self.source());
        context
    }

    /// A listed skill whose archive the server serves.
    fn skill(&mut self, id: &str, body: &str) -> StateSkill {
        let mut skill = self.fixture.skill(id, body);
        let archive =
            std::fs::read(self.fixture.root.join("source").join(&skill.artifact)).unwrap();
        let path = format!("/{}", skill.artifact);
        self.server.route(&path, Route::Body(archive));
        skill.artifact = self.server.url(&path);
        skill
    }

    fn publish(&self, skills: &[StateSkill], extra: Value) {
        let mut state = json!({
            "format_version": 1,
            "issued_at": "2026-10-09T11:00:00Z",
            "expires_at": "2026-10-16T11:00:00Z",
            "source": self.server.url("/state"),
            "subject": "team-a",
            "skills": skills,
            "allowed": "any",
            "report_url": self.server.url("/report"),
        });
        for (key, value) in extra.as_object().unwrap() {
            state[key] = value.clone();
        }
        let envelope = sign(&serde_json::to_vec(&state).unwrap(), &[("k1", 1)]);
        self.server.route("/state", Route::Body(envelope));
    }

    fn apply(&self) -> ApplyOutcome {
        self.fixture.apply_with(&self.context())
    }

    fn reports(&self) -> Vec<Report> {
        self.server
            .seen_at("/report")
            .iter()
            .map(|seen| serde_json::from_slice(&seen.body).unwrap())
            .collect()
    }
}

#[test]
fn an_https_apply_deploys_from_the_source_and_reports_with_a_rising_sequence() {
    let mut https = Https::new();
    let pdf = https.skill("pdf", "one");
    https.publish(std::slice::from_ref(&pdf), json!({}));
    write_skill(&https.fixture.project.join("notes"), "notes", "mine");

    let first = https.apply();
    assert!(first.completed, "{first:?}");
    assert!(!first.sign_in_needed && first.warnings.is_empty());
    assert_eq!(first.deployed.len(), 1);
    let delivery = first.report.unwrap();
    assert_eq!((delivery.sequence, delivery.accepted), (1, true));

    let second = https.apply();
    assert_eq!(second.report.unwrap().sequence, 2);
    let reports = https.reports();
    assert_eq!(reports.len(), 2);
    let report = &reports[0];
    assert_eq!(report.format_version, REPORT_FORMAT_VERSION);
    assert_eq!(report.sequence, 1);
    assert!(report.completed);
    assert_eq!(report.agents[0].name, "claude");
    assert!(!report.agents[0].hook);
    let skill = report
        .skills
        .iter()
        .find(|skill| skill.id == "pdf")
        .unwrap();
    assert_eq!(skill.outcome, SkillOutcome::Deployed);
    assert_eq!(skill.origin_kind.as_deref(), Some("managed"));
    assert_eq!(skill.digest.as_deref(), Some(pdf.digest.as_str()));
    assert_eq!(report.project_skills.len(), 1);
    assert_eq!(report.project_skills[0].id, "notes");
    assert_eq!(reports[1].sequence, 2);

    let body = String::from_utf8(https.server.seen_at("/report")[0].body.clone()).unwrap();
    assert!(
        !body.contains(https.fixture.root.to_str().unwrap()),
        "{body}"
    );
    let enrollment: Enrollment = read_record(&https.fixture.layout.enrollment_file()).unwrap();
    assert_eq!(enrollment.report_sequence, 2);

    let shown = status(&https.fixture.layout, &https.context().settings, now()).unwrap();
    assert!(shown.ignored.is_empty());
    assert_eq!(
        shown.report_note,
        Some(format!("sent to {}", https.server.url("/report")))
    );
    assert_eq!(shown.report.unwrap().sequence, 3);
}

#[test]
fn an_accepted_report_clears_the_refusals_it_carried_and_a_failed_one_keeps_them() {
    let mut https = Https::new();
    let pdf = https.skill("pdf", "one");
    https.publish(&[pdf], json!({}));
    let layout = https.fixture.layout.clone();
    record_refusal(&layout, "sha256-tree-v2:before").unwrap();
    assert!(refusals(&layout).unwrap().entries.is_empty());
    https.apply();

    set_current_command("skill add");
    let digest = format!("sha256-tree-v2:{}", "a".repeat(64));
    record_refusal(&layout, &digest).unwrap();
    record_refusal(&layout, &digest).unwrap();
    assert_eq!(refusals(&layout).unwrap().entries.len(), 1);
    https.apply();
    let carried = &https.reports()[1].refusals;
    assert_eq!(
        carried,
        &vec![Refusal {
            digest: digest.clone(),
            command: "skill add".to_string()
        }]
    );
    assert!(refusals(&layout).unwrap().entries.is_empty());

    https.server.route("/report", Route::Status(500));
    record_refusal(&layout, &digest).unwrap();
    let failed = https.apply();
    let delivery = failed.report.unwrap();
    assert!(!delivery.accepted && !failed.sign_in_needed);
    assert!(delivery.problem.unwrap().contains("500"));
    assert_eq!(refusals(&layout).unwrap().entries.len(), 1);

    https.server.route("/report", Route::Status(401));
    assert!(https.apply().sign_in_needed);
}

#[test]
fn a_failed_sign_in_applies_the_cached_state_and_sends_no_report() {
    let mut https = Https::new();
    let pdf = https.skill("pdf", "one");
    https.publish(&[pdf], json!({}));
    https.apply();

    let mut context = https.context();
    context.settings.credential_command = Some(vec!["fastskill-no-such-helper".to_string()]);
    let outcome = https.fixture.apply_with(&context);
    assert!(outcome.sign_in_needed);
    assert!(outcome.completed);
    assert!(
        outcome
            .state_problem
            .as_deref()
            .unwrap()
            .contains("couldn't start"),
        "{outcome:?}"
    );
    let delivery = outcome.report.unwrap();
    assert!(!delivery.accepted);
    assert_eq!(
        delivery.problem.as_deref(),
        Some("not sent: sign-in is needed")
    );
    assert_eq!(https.server.seen_at("/state").len(), 1);
    assert_eq!(https.reports().len(), 1);
    assert!(
        status(&https.fixture.layout, &context.settings, now())
            .unwrap()
            .sign_in_needed
    );

    https.server.route("/state", Route::Status(401));
    let outcome = https.apply();
    assert!(outcome.sign_in_needed && outcome.completed);
    assert!(outcome.state_problem.unwrap().contains("401"));
}

#[test]
#[cfg(unix)]
fn the_token_reaches_the_state_the_artifacts_and_the_report() {
    let mut https = Https::new();
    let pdf = https.skill("pdf", "one");
    https.publish(std::slice::from_ref(&pdf), json!({}));
    let mut context = https.context();
    context.settings.credential_command = Some(vec![
        "sh".to_string(),
        "-c".to_string(),
        "echo signed-in".to_string(),
    ]);
    assert!(https.fixture.apply_with(&context).report.unwrap().accepted);
    let seen = https.server.seen();
    assert_eq!(seen.len(), 3);
    for request in seen {
        assert_eq!(request.authorization.as_deref(), Some("Bearer signed-in"));
    }
}

#[test]
fn urls_on_another_origin_get_a_warning_and_no_report() {
    let mut https = Https::new();
    let pdf = https.skill("pdf", "one");
    https.publish(
        &[pdf],
        json!({
            "report_url": "https://elsewhere.example.com/report",
            "request_url": "https://elsewhere.example.com/request/{digest}",
        }),
    );
    let outcome = https.apply();
    assert!(outcome.report.is_none());
    assert_eq!(outcome.warnings.len(), 2, "{:?}", outcome.warnings);
    assert!(outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("report_url")));
    assert!(outcome
        .warnings
        .iter()
        .any(|warning| warning.contains("request_url")));
    assert!(https.reports().is_empty());
}

#[test]
fn a_file_source_ignores_the_https_fields_and_is_never_reported_to() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(
        &[pdf],
        json!({
            "editable": "refused",
            "request_url": "https://skills.example.com/request/{digest}",
            "report_url": "https://skills.example.com/report",
        }),
    );
    let outcome = fixture.apply();
    assert!(outcome.report.is_none() && outcome.warnings.is_empty());
    let layout = fixture.layout.clone();
    record_refusal(&layout, "sha256-tree-v2:x").unwrap();

    let shown = status(&layout, &fixture.settings(), now()).unwrap();
    assert_eq!(shown.ignored.len(), 3, "{:?}", shown.ignored);
    assert!(shown.ignored[0].starts_with("editable"));
    assert_eq!(
        shown.report_note.as_deref(),
        Some("none: a file source never gets a report")
    );
    let report = shown.report.unwrap();
    assert!(report.refusals.is_empty() && report.project_skills.is_empty());
    assert_eq!(report.sequence, 1);
}

#[test]
fn destinations() {
    let https = ManagedSource::parse("https://skills.example.com/state").unwrap();
    let file = ManagedSource::File(std::env::temp_dir().join("state.dsse"));
    assert_eq!(destination(None, &https), Ok(None));
    assert_eq!(
        destination(Some("https://skills.example.com/r"), &https),
        Ok(Some("https://skills.example.com/r".to_string()))
    );
    assert!(destination(Some("https://other.example.com/r"), &https)
        .unwrap_err()
        .contains("isn't on the managed source's origin"));
    assert!(destination(Some("https://skills.example.com/r"), &file).is_err());
}

#[test]
fn the_snapshot_names_adopted_collided_and_unmanaged_entries() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    let docs = fixture.skill("docs", "two");
    fixture.publish(&[pdf, docs], json!({}));
    let target = fixture.target();
    write_skill(&target.join("pdf"), "pdf", "one");
    write_skill(&target.join("docs"), "docs", "mine");
    #[cfg(unix)]
    {
        let editable = fixture.folder("mine", "local");
        std::os::unix::fs::symlink(&editable, target.join("mine")).unwrap();
    }
    let outcome = fixture.apply();
    let outcomes: Vec<(String, SkillOutcome)> = outcome
        .snapshot
        .skills
        .iter()
        .map(|skill| (skill.id.clone(), skill.outcome))
        .collect();
    assert!(outcomes.contains(&("pdf".to_string(), SkillOutcome::Adopted)));
    assert!(outcomes.contains(&("docs".to_string(), SkillOutcome::Collision)));
    #[cfg(unix)]
    {
        let mine = outcome
            .snapshot
            .skills
            .iter()
            .find(|skill| skill.id == "mine")
            .unwrap();
        assert_eq!(mine.outcome, SkillOutcome::Unmanaged);
        assert!(mine.editable && mine.origin_kind.is_none());
    }
}

#[test]
fn quarantined_entries_appear_in_the_report() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    fixture.apply();
    let target = fixture.target();
    std::fs::write(target.join("pdf/SKILL.md"), "changed").unwrap();
    fixture.publish(&[], json!({}));
    let outcome = fixture.apply();
    assert_eq!(outcome.snapshot.quarantined_now.len(), 1);
    assert_eq!(outcome.snapshot.quarantined_now[0].id, "pdf");
    let quarantined = outcome
        .snapshot
        .skills
        .iter()
        .find(|skill| skill.outcome == SkillOutcome::Quarantined)
        .unwrap();
    assert_eq!(quarantined.id, "pdf");
    let shown = status(&fixture.layout, &fixture.settings(), now()).unwrap();
    assert_eq!(shown.report.unwrap().quarantine.len(), 1);
}

#[test]
fn only_an_unexpired_https_state_keeps_refusals() {
    let fixture = Fixture::new();
    let layout = fixture.layout.clone();
    std::fs::create_dir_all(layout.managed()).unwrap();
    write_record(&layout.enrollment_file(), &Enrollment::default()).unwrap();
    let blocked = format!("sha256-tree-v2:{}", "b".repeat(64));
    let situation = |source: ManagedSource, expired: bool| {
        let state = json!({
            "format_version": 1,
            "issued_at": "2026-10-09T11:00:00Z",
            "expires_at": "2026-10-16T11:00:00Z",
            "source": source.as_str(),
            "subject": "team-a",
            "skills": [],
            "allowed": "any",
            "blocked": [{ "digest": blocked }],
        });
        Situation::State {
            state: Box::new(ManagedState::parse(&serde_json::to_vec(&state).unwrap()).unwrap()),
            source,
            expired,
        }
    };
    let https = ManagedSource::parse("https://skills.example.com/state").unwrap();
    let file = ManagedSource::File(std::env::temp_dir().join("state.dsse"));
    let candidate = Candidate::digest("bad", &blocked);

    for (source, expired) in [(file, false), (https.clone(), true)] {
        let checked = situation(source, expired).check_recording(candidate, &layout);
        assert!(checked.is_err(), "expired: {expired}");
        assert!(refusals(&layout).unwrap().entries.is_empty());
    }
    let error = situation(https.clone(), false)
        .check_recording(candidate, &layout)
        .unwrap_err();
    assert!(error.to_string().contains("blocked"), "{error}");
    assert_eq!(refusals(&layout).unwrap().entries[0].digest, blocked);
    let fine = format!("sha256-tree-v2:{}", "c".repeat(64));
    situation(https, false)
        .check_recording(Candidate::digest("fine", &fine), &layout)
        .unwrap();
    assert_eq!(refusals(&layout).unwrap().entries.len(), 1);

    clear_refusals(&layout, &refusals(&layout).unwrap().entries).unwrap();
    assert!(refusals(&layout).unwrap().entries.is_empty());
}

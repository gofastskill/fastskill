#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::apply::{apply, entry_digest, fetch, targets, ApplyContext, ApplyOutcome, ApplyResult};
use super::config::{ManagedSettings, ManagedSource};
use super::enrollment::{enroll, status, unenroll};
use super::layout::{read_record, write_record, ManagedLayout};
use super::quarantine::{self, QuarantineReason};
use super::records::{Enrollment, EntryMode, OwnedEntry, Ownership};
use super::state::StateSkill;
use super::store::{artifact_path, ManagedStore};
use super::tests::{pinned, sign};
use crate::core::content_digest::content_digest;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn now() -> DateTime<Utc> {
    "2026-10-09T12:00:00Z".parse().unwrap()
}

pub(super) struct Fixture {
    pub(super) _dir: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) state_file: PathBuf,
    pub(super) layout: ManagedLayout,
    pub(super) project: PathBuf,
    pub(super) count: usize,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("home/.claude")).unwrap();
        std::fs::create_dir_all(root.join("project/.claude/skills")).unwrap();
        std::fs::create_dir_all(root.join("source")).unwrap();
        Self {
            state_file: root.join("source/state.dsse"),
            layout: ManagedLayout::new(root.join("data")),
            project: root.join("project/.claude/skills"),
            root,
            _dir: dir,
            count: 0,
        }
    }

    pub(super) fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    pub(super) fn settings(&self) -> ManagedSettings {
        ManagedSettings {
            source: Some(ManagedSource::File(self.state_file.clone())),
            keys: vec![pinned("k1", 1)],
            targets: Some(vec!["claude".to_string()]),
            source_from_system: true,
            ..ManagedSettings::default()
        }
    }

    pub(super) fn context(&self) -> ApplyContext {
        ApplyContext {
            layout: self.layout.clone(),
            settings: self.settings(),
            home: self.home(),
            project_skills: Some(self.project.clone()),
            may_enroll: true,
            now: now(),
            interactive: false,
            config_dir: self.root.clone(),
            hooks: None,
            timer: None,
        }
    }

    pub(super) fn target(&self) -> PathBuf {
        targets(&self.context()).remove(0)
    }

    /// A skill folder named `id` whose body is `body`, outside any target.
    pub(super) fn folder(&mut self, id: &str, body: &str) -> PathBuf {
        self.count += 1;
        let folder = self.root.join(format!("src/{}/{id}", self.count));
        write_skill(&folder, id, body);
        folder
    }

    /// A listed skill with an archive in the source folder, relative to the state file.
    pub(super) fn skill(&mut self, id: &str, body: &str) -> StateSkill {
        let folder = self.folder(id, body);
        let name = format!("{id}-{}.zip", self.count);
        zip_folder(&folder, &self.root.join("source").join(&name));
        StateSkill {
            id: id.to_string(),
            digest: content_digest(&folder).unwrap(),
            artifact: name,
        }
    }

    pub(super) fn publish(&self, skills: &[StateSkill], extra: Value) {
        let mut state = json!({
            "format_version": 1,
            "issued_at": "2026-10-09T11:00:00Z",
            "expires_at": "2026-10-16T11:00:00Z",
            "source": self.state_file.display().to_string(),
            "subject": "team-a",
            "skills": skills,
            "allowed": "any",
        });
        for (key, value) in extra.as_object().unwrap() {
            state[key] = value.clone();
        }
        let envelope = sign(&serde_json::to_vec(&state).unwrap(), &[("k1", 1)]);
        std::fs::write(&self.state_file, envelope).unwrap();
    }

    pub(super) fn apply(&self) -> ApplyOutcome {
        self.apply_with(&self.context())
    }

    pub(super) fn apply_with(&self, context: &ApplyContext) -> ApplyOutcome {
        match apply(context).unwrap() {
            ApplyResult::Done(outcome) => *outcome,
            ApplyResult::Busy => panic!("busy"),
        }
    }

    pub(super) fn ownership(&self) -> Ownership {
        read_record(&self.layout.ownership_file()).unwrap()
    }
}

pub(super) fn write_skill(folder: &Path, id: &str, body: &str) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: A test skill.\nversion: 1.0.0\n---\n{body}\n"),
    )
    .unwrap();
}

pub(super) fn zip_folder(folder: &Path, archive: &Path) {
    let file = std::fs::File::create(archive).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let name = folder.file_name().unwrap().to_str().unwrap();
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(format!("{name}/SKILL.md"), options).unwrap();
    zip.write_all(&std::fs::read(folder.join("SKILL.md")).unwrap())
        .unwrap();
    zip.finish().unwrap();
}

#[test]
fn a_first_apply_enrolls_deploys_links_and_records_ownership() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));

    let outcome = fixture.apply();
    assert!(outcome.completed, "{outcome:?}");
    assert!(outcome.enrolled_now);
    assert_eq!(outcome.subject, "team-a");
    assert_eq!(outcome.deployed.len(), 1);
    let entry = fixture.target().join("pdf");
    assert!(entry.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(entry_digest(&entry).as_deref(), Some(pdf.digest.as_str()));
    let ownership = fixture.ownership();
    assert_eq!(ownership.entries.len(), 1);
    assert_eq!(ownership.entries[0].mode, EntryMode::Link);
    assert!(fixture.layout.cached_state().is_file());
    let enrollment: Enrollment = read_record(&fixture.layout.enrollment_file()).unwrap();
    assert_eq!(enrollment.recorded.subject.as_deref(), Some("team-a"));
    let last: ApplyOutcome = read_record(&fixture.layout.last_apply_file()).unwrap();
    assert_eq!(last, outcome);

    // Nothing changes the second time.
    let again = fixture.apply();
    assert!(again.completed && !again.enrolled_now);
    assert!(again.deployed.is_empty() && again.removed.is_empty());
}

#[test]
fn an_unenrolled_user_is_enrolled_only_when_allowed() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(&[pdf], json!({}));
    let mut context = fixture.context();
    context.may_enroll = false;
    let error = apply(&context).unwrap_err();
    assert!(error.to_string().contains("managed enroll"), "{error}");
}

#[test]
fn a_second_apply_waits_for_none_while_one_runs() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(&[pdf], json!({}));
    let _held = fixture.layout.try_lock().unwrap().unwrap();
    assert_eq!(apply(&fixture.context()).unwrap(), ApplyResult::Busy);
    assert_eq!(enroll(&fixture.context()).unwrap(), ApplyResult::Busy);
    let error = unenroll(
        &fixture.layout,
        &fixture.settings(),
        None,
        &fixture.home(),
        None,
        now(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("apply is running"), "{error}");
}

#[test]
fn a_new_version_replaces_the_entry_and_the_old_one_leaves_the_store() {
    let mut fixture = Fixture::new();
    let old = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&old), json!({}));
    fixture.apply();
    let new = fixture.skill("pdf", "two");
    fixture.publish(std::slice::from_ref(&new), json!({}));

    let outcome = fixture.apply();
    assert!(outcome.completed, "{outcome:?}");
    assert_eq!(outcome.deployed[0].digest, new.digest);
    let entry = fixture.target().join("pdf");
    assert_eq!(entry_digest(&entry).as_deref(), Some(new.digest.as_str()));
    let store = ManagedStore::new(&fixture.layout);
    assert!(!store.path(&old.digest).exists());
    assert!(store.path(&new.digest).is_dir());
    assert_eq!(fixture.ownership().entries[0].digest, new.digest);
}

#[test]
fn an_unlisted_skill_is_removed_and_a_changed_copy_is_quarantined() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    let docx = fixture.skill("docx", "one");
    fixture.publish(&[pdf, docx], json!({}));
    fixture.apply();
    let target = fixture.target();
    // Someone replaced the docx link with their own copy.
    quarantine::remove_entry(&target.join("docx")).unwrap();
    write_skill(&target.join("docx"), "docx", "edited");
    fixture.publish(&[], json!({}));

    let outcome = fixture.apply();
    assert!(outcome.completed, "{outcome:?}");
    assert_eq!(outcome.removed.len(), 1);
    assert_eq!(outcome.removed[0].id, "pdf");
    assert_eq!(outcome.quarantined.len(), 1);
    assert_eq!(outcome.quarantined[0].reason, QuarantineReason::Modified);
    assert!(target.join("pdf").symlink_metadata().is_err());
    assert!(target.join("docx").symlink_metadata().is_err());
    assert!(fixture.ownership().entries.is_empty());
    assert_eq!(
        std::fs::read_dir(fixture.layout.store()).unwrap().count(),
        0
    );
    assert_eq!(quarantine::list(&fixture.layout).unwrap().len(), 1);
}

#[test]
fn a_changed_owned_entry_is_quarantined_before_the_listed_content_is_deployed() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    fixture.apply();
    let entry = fixture.target().join("pdf");
    quarantine::remove_entry(&entry).unwrap();
    write_skill(&entry, "pdf", "edited");

    let outcome = fixture.apply();
    assert_eq!(outcome.quarantined.len(), 1);
    assert_eq!(outcome.deployed.len(), 1);
    assert_eq!(entry_digest(&entry).as_deref(), Some(pdf.digest.as_str()));
}

#[test]
fn a_missing_owned_entry_is_deployed_again() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(&[pdf], json!({}));
    fixture.apply();
    quarantine::remove_entry(&fixture.target().join("pdf")).unwrap();
    assert_eq!(fixture.apply().deployed.len(), 1);
}

#[test]
fn identical_unowned_content_is_adopted() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    write_skill(&fixture.target().join("pdf"), "pdf", "one");

    let outcome = fixture.apply();
    assert_eq!(outcome.adopted.len(), 1);
    assert!(outcome.deployed.is_empty());
    assert_eq!(fixture.ownership().entries[0].mode, EntryMode::Adopted);
}

#[test]
fn other_unowned_content_is_a_collision_unless_it_must_be_quarantined() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    let entry = fixture.target().join("pdf");
    write_skill(&entry, "pdf", "mine");

    let outcome = fixture.apply();
    assert_eq!(outcome.collisions, vec![entry.clone()]);
    assert!(outcome.completed);
    assert_eq!(entry_digest(&entry), Some(content_digest(&entry).unwrap()));

    let mine = content_digest(&entry).unwrap();
    fixture.publish(
        std::slice::from_ref(&pdf),
        json!({ "blocked": [{ "digest": mine }] }),
    );
    let outcome = fixture.apply();
    assert!(outcome.collisions.is_empty());
    assert_eq!(outcome.quarantined[0].reason, QuarantineReason::Blocked);
    assert_eq!(entry_digest(&entry).as_deref(), Some(pdf.digest.as_str()));
}

#[test]
fn exclusive_targets_quarantine_content_that_is_not_allowed() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    let other = fixture.folder("notes", "one");
    let allowed = fixture.folder("todo", "one");
    let target = fixture.target();
    write_skill(&target.join("notes"), "notes", "one");
    write_skill(&target.join("todo"), "todo", "one");
    write_skill(&target.join(".hidden"), "hidden", "one");
    fixture.publish(
        &[pdf],
        json!({
            "allowed": "listed",
            "exclusive_targets": true,
            "also_allowed": [content_digest(&allowed).unwrap()],
        }),
    );

    let outcome = fixture.apply();
    assert_eq!(outcome.quarantined.len(), 1, "{outcome:?}");
    assert_eq!(outcome.quarantined[0].reason, QuarantineReason::NotAllowed);
    assert_eq!(
        outcome.quarantined[0].digest,
        Some(content_digest(&other).unwrap())
    );
    assert!(target.join("todo").is_dir());
    assert!(target.join(".hidden").is_dir());
}

#[test]
fn blocked_project_skills_are_quarantined_and_clashes_reported() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    write_skill(&fixture.project.join("pdf"), "pdf", "local");
    write_skill(&fixture.project.join("bad"), "bad", "one");
    let bad = content_digest(&fixture.project.join("bad")).unwrap();
    fixture.publish(&[pdf], json!({ "blocked": [{ "digest": bad }] }));

    let outcome = fixture.apply();
    assert_eq!(outcome.project_clashes, vec![fixture.project.join("pdf")]);
    assert_eq!(outcome.quarantined.len(), 1);
    assert!(!fixture.project.join("bad").exists());
    let listed = quarantine::list(&fixture.layout).unwrap();
    assert_eq!(listed[0].former_path, fixture.project.join("bad"));
    assert!(listed[0].path.join("SKILL.md").is_file());
}

#[test]
fn an_expired_state_changes_no_content_but_still_quarantines_blocked_content() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    fixture.apply();
    write_skill(&fixture.target().join("bad"), "bad", "one");
    let bad = content_digest(&fixture.target().join("bad")).unwrap();
    let docx = fixture.skill("docx", "one");
    fixture.publish(
        &[docx],
        json!({
            "issued_at": "2026-10-01T11:00:00Z",
            "expires_at": "2026-10-02T11:00:00Z",
            "blocked": [{ "digest": bad }],
        }),
    );
    // Accepting an older state needs a fresh enrollment.
    std::fs::remove_file(fixture.layout.enrollment_file()).unwrap();

    let outcome = fixture.apply();
    assert!(outcome.expired);
    assert!(outcome.deployed.is_empty() && outcome.removed.is_empty());
    assert!(fixture.target().join("pdf").exists());
    assert!(!fixture.target().join("docx").exists());
    assert_eq!(outcome.quarantined.len(), 1);
}

#[test]
fn an_unreadable_source_falls_back_to_the_cached_state() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(&[pdf], json!({}));
    fixture.apply();
    std::fs::remove_file(&fixture.state_file).unwrap();

    let outcome = fixture.apply();
    assert!(outcome.completed);
    let problem = outcome.state_problem.unwrap();
    assert!(
        problem.contains("can't read the managed state"),
        "{problem}"
    );

    // Without a cached state the problem is the error.
    std::fs::remove_file(fixture.layout.cached_state()).unwrap();
    assert!(apply(&fixture.context()).is_err());
}

#[test]
fn a_refused_state_falls_back_and_a_refused_cache_is_an_error() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    fixture.apply();
    fixture.publish(&[pdf], json!({ "subject": "team-b" }));
    let outcome = fixture.apply();
    assert!(outcome.state_problem.unwrap().contains("team-b"));

    // The cache no longer opens under other keys either.
    let mut context = fixture.context();
    context.settings.keys = vec![pinned("k2", 2)];
    let error = apply(&context).unwrap_err();
    assert!(error.to_string().contains("pinned key"), "{error}");
}

#[test]
fn an_artifact_with_the_wrong_digest_or_identity_fails_that_skill() {
    let mut fixture = Fixture::new();
    let mut wrong_digest = fixture.skill("pdf", "one");
    wrong_digest.digest = fixture.skill("pdf", "two").digest;
    let mut wrong_id = fixture.skill("docx", "one");
    wrong_id.id = "xlsx".to_string();
    let missing = StateSkill {
        artifact: "missing.zip".to_string(),
        ..fixture.skill("csv", "one")
    };
    fixture.publish(&[wrong_digest, wrong_id, missing], json!({}));

    let outcome = fixture.apply();
    assert!(!outcome.completed);
    let failures: Vec<&str> = outcome
        .failures
        .iter()
        .map(|(_, why)| why.as_str())
        .collect();
    assert_eq!(failures.len(), 3, "{failures:?}");
    assert!(failures[0].contains("has digest"), "{failures:?}");
    assert!(failures[1].contains("is the skill"), "{failures:?}");
    assert!(outcome.deployed.is_empty());
}

#[test]
fn a_changed_source_or_no_source_is_refused() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(&[pdf], json!({}));
    fixture.apply();
    let mut context = fixture.context();
    context.settings.source = Some(ManagedSource::File(fixture.root.join("elsewhere.dsse")));
    let error = apply(&context).unwrap_err();
    assert!(error.to_string().contains("enrolled with"), "{error}");
    context.settings.source = None;
    assert!(apply(&context).is_err());
    assert!(enroll(&context).is_err());
}

#[test]
fn enrolling_again_keeps_the_machine_id_and_follows_a_new_subject() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    let ApplyResult::Done(first) = enroll(&fixture.context()).unwrap() else {
        panic!("busy");
    };
    assert!(first.completed);
    let before: Enrollment = read_record(&fixture.layout.enrollment_file()).unwrap();
    fixture.publish(
        &[pdf],
        json!({ "subject": "team-b", "issued_at": "2026-10-09T11:30:00Z" }),
    );
    enroll(&fixture.context()).unwrap();
    let after: Enrollment = read_record(&fixture.layout.enrollment_file()).unwrap();
    assert_eq!(after.machine_id, before.machine_id);
    assert_eq!(after.recorded.subject.as_deref(), Some("team-b"));
}

#[test]
fn unenrolling_removes_everything_but_the_quarantine() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    let docx = fixture.skill("docx", "one");
    fixture.publish(&[pdf, docx], json!({}));
    fixture.apply();
    let target = fixture.target();
    quarantine::remove_entry(&target.join("docx")).unwrap();
    write_skill(&target.join("docx"), "docx", "edited");
    let user_file = fixture.root.join("managed.toml");
    std::fs::write(&user_file, "").unwrap();
    // An owned entry that's already gone is skipped.
    let mut ownership = fixture.ownership();
    ownership.record(OwnedEntry {
        target: target.clone(),
        id: "gone".to_string(),
        digest: "x".to_string(),
        mode: EntryMode::Copy,
    });
    write_record(&fixture.layout.ownership_file(), &ownership).unwrap();

    let outcome = unenroll(
        &fixture.layout,
        &fixture.settings(),
        Some(&user_file),
        &fixture.home(),
        None,
        now(),
    )
    .unwrap();
    assert_eq!(outcome.removed, vec![target.join("pdf")]);
    assert_eq!(outcome.quarantined.len(), 1);
    assert_eq!(outcome.removed_settings, Some(user_file.clone()));
    assert!(!user_file.exists());
    assert!(!fixture.layout.managed().exists());
    assert_eq!(quarantine::list(&fixture.layout).unwrap().len(), 1);

    let mut required = fixture.settings();
    required.required_by_system = true;
    let error = unenroll(
        &fixture.layout,
        &required,
        None,
        &fixture.home(),
        None,
        now(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("can't unenroll"), "{error}");
}

#[test]
fn status_reads_the_records_and_the_cached_state() {
    let mut fixture = Fixture::new();
    let empty = status(&fixture.layout, &fixture.settings(), now()).unwrap();
    assert!(empty.enrollment.is_none() && empty.last_apply.is_none());

    let pdf = fixture.skill("pdf", "one");
    fixture.publish(&[pdf], json!({}));
    fixture.apply();
    let shown = status(&fixture.layout, &fixture.settings(), now()).unwrap();
    assert_eq!(shown.subject.as_deref(), Some("team-a"));
    assert!(!shown.expired && shown.state_problem.is_none());
    assert_eq!(shown.owned.len(), 1);
    assert!(shown.last_apply.unwrap().completed);
    let later = status(
        &fixture.layout,
        &fixture.settings(),
        now() + Duration::days(30),
    )
    .unwrap();
    assert!(later.expired);

    let mut other = fixture.settings();
    other.keys = vec![pinned("k2", 2)];
    let refused = status(&fixture.layout, &other, now()).unwrap();
    assert!(refused.state_problem.is_some());
    std::fs::remove_file(fixture.layout.cached_state()).unwrap();
    let uncached = status(&fixture.layout, &fixture.settings(), now()).unwrap();
    assert_eq!(
        uncached.state_problem.as_deref(),
        Some("no state has been accepted yet")
    );
}

#[test]
fn a_corrupt_store_entry_is_fetched_again() {
    let mut fixture = Fixture::new();
    let pdf = fixture.skill("pdf", "one");
    fixture.publish(std::slice::from_ref(&pdf), json!({}));
    fixture.apply();
    let store = ManagedStore::new(&fixture.layout);
    std::fs::write(store.path(&pdf.digest).join("SKILL.md"), "tampered").unwrap();
    assert!(store.verified(&pdf.digest).unwrap().is_none());
    assert!(!store.path(&pdf.digest).exists());
    let source = ManagedSource::File(fixture.state_file.clone());
    let path = store.ensure(&pdf, &source, None).unwrap();
    assert_eq!(content_digest(&path).unwrap(), pdf.digest);
}

#[test]
fn artifact_paths_and_https_sources() {
    let file = ManagedSource::File(PathBuf::from("/srv/state/state.dsse"));
    assert_eq!(
        artifact_path("a.zip", &file).unwrap(),
        PathBuf::from("/srv/state/a.zip")
    );
    let absolute = std::env::temp_dir().join("a.zip");
    assert_eq!(
        artifact_path(absolute.to_str().unwrap(), &file).unwrap(),
        absolute
    );
    assert!(artifact_path("https://example.com/a.zip", &file).is_err());
    let https = ManagedSource::parse("https://skills.example.com/state").unwrap();
    assert!(artifact_path("https://skills.example.com/a.zip", &https).is_err());
    let error = fetch(&https, None).unwrap_err();
    assert!(error.message.contains("https isn't available"), "{error}");
    assert!(!error.sign_in);
    let listed = StateSkill {
        id: "pdf".to_string(),
        digest: format!("sha256-tree-v2:{}", "0".repeat(64)),
        artifact: "https://skills.example.com/a.zip".to_string(),
    };
    let fixture = Fixture::new();
    let error = ManagedStore::new(&fixture.layout)
        .ensure(&listed, &https, None)
        .unwrap_err();
    assert!(
        error.to_string().contains("https isn't available"),
        "{error}"
    );
}

#[test]
fn records_and_quarantine_listing_tolerate_what_they_can() {
    let fixture = Fixture::new();
    assert!(ManagedLayout::for_current_user().is_ok());
    std::fs::create_dir_all(fixture.layout.managed()).unwrap();
    std::fs::write(fixture.layout.ownership_file(), "not json").unwrap();
    let damaged = read_record::<Ownership>(&fixture.layout.ownership_file()).unwrap_err();
    assert!(damaged.to_string().contains("damaged"), "{damaged}");

    assert!(quarantine::list(&fixture.layout).unwrap().is_empty());
    let skill = fixture.root.join("loose");
    write_skill(&skill, "loose", "one");
    let file = fixture.root.join("loose.txt");
    std::fs::write(&file, "x").unwrap();
    let first = quarantine::quarantine(
        &fixture.layout,
        &skill,
        None,
        QuarantineReason::Blocked,
        now(),
    )
    .unwrap();
    quarantine::quarantine(
        &fixture.layout,
        &file,
        None,
        QuarantineReason::Blocked,
        now(),
    )
    .unwrap();
    std::fs::create_dir_all(fixture.layout.quarantine().join("stray")).unwrap();
    let listed = quarantine::list(&fixture.layout).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0], first);
    assert!(!skill.exists() && !file.exists());
}

#[test]
fn quarantine_reports_what_it_cant_move_and_lists_only_complete_records() {
    let fixture = Fixture::new();
    let missing = fixture.root.join("missing");
    let error = quarantine::quarantine(
        &fixture.layout,
        &missing,
        None,
        QuarantineReason::Modified,
        now(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("into the quarantine"), "{error}");

    let blank = fixture.layout.quarantine().join("blank");
    std::fs::create_dir_all(&blank).unwrap();
    std::fs::write(blank.join(quarantine::RECORD_FILE), "{}").unwrap();
    assert!(quarantine::list(&fixture.layout).unwrap().is_empty());

    let stamp = now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
    for n in 0..1000 {
        std::fs::create_dir_all(fixture.layout.quarantine().join(format!("{stamp}-{n}"))).unwrap();
    }
    let error = quarantine::quarantine(
        &fixture.layout,
        &missing,
        None,
        QuarantineReason::Modified,
        now(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("no free quarantine folder"),
        "{error}"
    );
}

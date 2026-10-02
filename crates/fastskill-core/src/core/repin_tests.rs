use super::*;
use tempfile::TempDir;

const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn document(text: &str) -> DocumentMut {
    text.parse().unwrap()
}

#[test]
fn set_string_rewrites_values_in_tables_and_inline_tables_and_keeps_comments() {
    let mut doc = document(&format!(
        "[[skills]]\nid = \"alpha\"\n\n[skills.resolved]\nchecksum = \"{OLD}\" # pinned\n\n\
         [[bundles]]\nid = \"team\"\nmembers = [{{ id = \"member\", digest = \"{OLD}\" }}]\n"
    ));
    let checksum = [
        Step::Entry("skills", "alpha"),
        Step::Key("resolved"),
        Step::Key("checksum"),
    ];
    assert!(set_string(
        doc.as_table_mut(),
        &checksum,
        OLD,
        "new-checksum"
    ));
    let member = [
        Step::Entry("bundles", "team"),
        Step::Entry("members", "member"),
        Step::Key("digest"),
    ];
    assert!(set_string(doc.as_table_mut(), &member, OLD, "new-member"));

    let text = doc.to_string();
    assert!(
        text.contains("checksum = \"new-checksum\" # pinned"),
        "{text}"
    );
    assert!(text.contains("digest = \"new-member\" }"), "{text}");

    // A value that is no longer the expected one, or a path that does not lead to a
    // string, is left alone.
    assert!(!set_string(doc.as_table_mut(), &checksum, OLD, "other"));
    assert!(!set_string(
        doc.as_table_mut(),
        &[Step::Entry("skills", "missing"), Step::Key("id")],
        "x",
        "y"
    ));
    assert!(!set_string(
        doc.as_table_mut(),
        &[Step::Entry("metadata", "alpha"), Step::Key("id")],
        "x",
        "y"
    ));
    assert!(!set_string(
        doc.as_table_mut(),
        &[Step::Key("skills")],
        "x",
        "y"
    ));
    assert!(!set_string(doc.as_table_mut(), &[], "x", "y"));
}

#[test]
fn write_refuses_a_record_that_changed_after_it_was_planned() {
    let root = TempDir::new().unwrap();
    let file = root.path().join("skills.lock");
    fs::write(
        &file,
        "[[skills]]\nid = \"alpha\"\n[skills.resolved]\nchecksum = \"changed\"\n",
    )
    .unwrap();
    let planned = entry(
        RepinRecord::Skill,
        "alpha",
        &file,
        OLD,
        Ok("current".to_string()),
    );
    let report = RepinReport {
        entries: vec![planned],
        ..RepinReport::default()
    };

    let error = write(report).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("changed while its digests were re-pinned"),
        "{error}"
    );
    assert!(fs::read_to_string(&file).unwrap().contains("\"changed\""));
}

#[test]
fn check_installed_refuses_ids_that_leave_the_skills_directory() {
    let root = TempDir::new().unwrap();

    let reason = check_installed(OLD, root.path(), "../outside").unwrap_err();

    assert!(reason.contains("is not a valid skill id"), "{reason}");
}

#[test]
fn history_releases_that_cannot_be_classified_are_not_reported() {
    let root = TempDir::new().unwrap();
    let history = root.path().join(BUNDLE_HISTORY_FILE);
    fs::create_dir_all(history.parent().unwrap()).unwrap();
    fs::write(
        &history,
        format!(
            "[releases]\n\"no-version\" = \"{OLD}\"\n\"team@not-semver\" = \"{OLD}\"\n\
             \"../team@1.0.0\" = \"{OLD}\"\n\"team@1.0.0\" = \"{OLD}\"\n"
        ),
    )
    .unwrap();

    assert!(plan_history(root.path(), &[]).unwrap().is_empty());

    fs::write(&history, "releases = 1\n").unwrap();
    assert!(plan_history(root.path(), &[]).is_err());
}

#[test]
fn unreadable_and_repeated_artifacts_are_not_reported() {
    let root = TempDir::new().unwrap();
    let cache = root.path().join(BUNDLE_STATE_DIRECTORY);
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("broken-1.0.0.zip"), b"not a zip").unwrap();
    fs::write(cache.join("notes.txt"), b"not an artifact").unwrap();
    fs::write(
        root.path().join(PROJECT_FILE),
        "[bundles.broken]\nversion = \"1.0.0\"\nartifact = \".fastskill/bundles/broken-1.0.0.zip\"\n\n\
         [bundles.absent]\nversion = \"1.0.0\"\nartifact = \"dist/absent-1.0.0.zip\"\n",
    )
    .unwrap();

    assert!(legacy_artifacts(root.path(), None).unwrap().is_empty());

    fs::write(root.path().join(PROJECT_FILE), "[bundles]\nbroken = 1\n").unwrap();
    assert!(legacy_artifacts(root.path(), None).is_err());
}

#[test]
fn nothing_is_written_when_no_legacy_value_is_recorded() {
    let root = TempDir::new().unwrap();
    let skills = root.path().join("skills");

    let project = repin_project(root.path(), &skills, false).unwrap();
    let global = repin_global(&root.path().join("global-skills.lock"), &skills, false).unwrap();

    assert_eq!(project, RepinReport::default());
    assert_eq!(global, RepinReport::default());
    assert_eq!(project.legacy_remaining(), 0);
    assert!(!root.path().join(".fastskill").exists());
    assert!(!skills.exists());
}

#[test]
fn record_labels_name_each_kind_of_record() {
    let labels: Vec<&str> = [
        RepinRecord::Skill,
        RepinRecord::Bundle,
        RepinRecord::Override,
        RepinRecord::BundleHistory,
    ]
    .into_iter()
    .map(RepinRecord::label)
    .collect();
    assert_eq!(labels, ["skill", "bundle", "override", "bundle history"]);
}

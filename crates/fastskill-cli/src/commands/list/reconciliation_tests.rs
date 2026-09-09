use super::*;
use fastskill_core::core::lock::{
    ProjectLockedBundleEntry, ProjectLockedBundleMember, ProjectLockedPersonalOverride,
};
use serde_json::Value;

struct ProjectFixture {
    original_directory: PathBuf,
    directory: TempDir,
    storage: PathBuf,
}

impl ProjectFixture {
    fn new(dependencies: &str) -> Self {
        let original_directory = env::current_dir().unwrap();
        let directory = TempDir::new().unwrap();
        let storage = directory.path().join("skills");
        fs::create_dir_all(&storage).unwrap();
        fs::write(
            directory.path().join("skill-project.toml"),
            format!(
                "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n{dependencies}\n"
            ),
        )
        .unwrap();
        env::set_current_dir(directory.path()).unwrap();
        Self {
            original_directory,
            directory,
            storage,
        }
    }

    fn skill(&self, id: &str, version: &str) -> String {
        let path = self.storage.join(id);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {id}\nversion: {version}\ndescription: fixture\n---\n# {id}\n"),
        )
        .unwrap();
        managed_tree_digest(&path).unwrap()
    }

    async fn service(&self, lock: &ProjectSkillsLock) -> FastSkillService {
        lock.save_to_file(&self.directory.path().join("skills.lock"))
            .unwrap();
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: self.storage.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();
        service
    }
}

impl Drop for ProjectFixture {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original_directory);
    }
}

fn json_args() -> ListArgs {
    ListArgs {
        format: Some(OutputFormat::Json),
        json: false,
        details: true,
        bundles: false,
        check: true,
        only: None,
        without: None,
        skills_dir: None,
    }
}

fn row<'a>(rows: &'a [Value], id: &str) -> &'a Value {
    rows.iter().find(|row| row["id"] == id).unwrap()
}

fn bundle(id: &str, members: &[(&str, &str)]) -> ProjectLockedBundleEntry {
    ProjectLockedBundleEntry {
        id: id.to_string(),
        version: "1.0.0".to_string(),
        artifact: format!("{id}.zip"),
        digest: "bundle-digest".to_string(),
        members: members
            .iter()
            .map(|(id, digest)| ProjectLockedBundleMember {
                id: (*id).to_string(),
                digest: (*digest).to_string(),
                overridable: true,
            })
            .collect(),
    }
}

#[tokio::test]
async fn bundle_members_report_shared_ownership_overrides_and_integrity_failures() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = ProjectFixture::new("");
    let shared_digest = fixture.skill("shared", "1.0.0");
    let override_digest = fixture.skill("custom", "1.0.0");
    fixture.skill("edited", "1.0.0");
    fixture.skill("conflict", "1.0.0");
    fixture.skill("unreadable", "1.0.0");
    let mut lock = ProjectSkillsLock::new_empty();
    lock.bundles = vec![
        bundle(
            "first",
            &[
                ("shared", &shared_digest),
                ("custom", "original-custom"),
                ("edited", "original-edited"),
                ("conflict", "first-content"),
                ("unreadable", "unreadable-content"),
                ("missing", "missing-content"),
            ],
        ),
        bundle(
            "second",
            &[
                ("shared", &shared_digest),
                ("custom", "other-custom"),
                ("conflict", "other-content"),
            ],
        ),
    ];
    lock.overrides.push(ProjectLockedPersonalOverride {
        id: "custom".to_string(),
        origin: "custom.zip".to_string(),
        digest: override_digest,
    });
    let service = fixture.service(&lock).await;
    // Simulate a directory disappearing after discovery. The integrity probe
    // must report an error instead of treating the cached skill as verified.
    fs::remove_dir_all(fixture.storage.join("unreadable")).unwrap();
    let (result, output) = crate::output::capture(execute_list(&service, json_args(), false)).await;
    let error = result.unwrap_err().to_string();
    let rows: Vec<Value> = serde_json::from_str(&output).unwrap();
    for (id, expected) in [
        ("shared", "ok"),
        ("custom", "ok"),
        ("edited", "content-mismatch"),
        ("conflict", "ownership-conflict"),
        ("unreadable", "integrity-error"),
        ("missing", "missing-content"),
    ] {
        assert_eq!(row(&rows, id)["reconciliation"], expected, "{id}");
        assert_eq!(row(&rows, id)["missing_from_manifest"], false, "{id}");
        if expected != "ok" {
            assert!(error.contains(&format!("{id}: {expected}")), "{error}");
        }
    }
    assert_eq!(
        row(&rows, "shared")["owners"],
        serde_json::json!(["bundle:first", "bundle:second"])
    );
    assert_eq!(row(&rows, "custom")["override_active"], true);
    assert_eq!(
        row(&rows, "custom")["owners"],
        serde_json::json!(["bundle:first", "bundle:second", "personal-override"])
    );

    let mut args = json_args();
    args.bundles = true;
    args.check = false;
    args.format = Some(OutputFormat::Table);
    let (result, output) = crate::output::capture(execute_list(&service, args, false)).await;
    result.unwrap();
    assert!(output.contains("first 1.0.0 [shared, custom, edited, conflict, unreadable, missing]"));
    assert!(output.contains("second 1.0.0 [shared, custom, conflict]"));
}

fn locked_child(id: &str, checksum: Option<String>) -> ProjectLockedSkillEntry {
    ProjectLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin: Origin::Local {
            path: PathBuf::from(format!("skills/{id}")),
            editable: id == "editable" || id == "root",
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum,
        },
        dependencies: Vec::new(),
        groups: vec!["dev".to_string()],
        depth: 1,
        parent_skill: Some("root".to_string()),
        required_by: vec!["root".to_string()],
    }
}

#[tokio::test]
async fn selected_roots_check_transitive_content_and_exclude_the_same_closure_by_group() {
    let _lock = fastskill_core::test_utils::DIR_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = ProjectFixture::new(
        "root = { origin = { type = \"local\", path = \"skills/root\", editable = true }, groups = [\"dev\"] }\nlatest = { origin = { type = \"repository\", repo = \"team\", skill = \"latest\" }, groups = [\"prod\"] }",
    );
    fixture.skill("root", "1.0.0");
    let good = fixture.skill("good", "1.0.0");
    fixture.skill("edited", "1.0.0");
    fixture.skill("revision", "2.0.0");
    fixture.skill("editable", "1.0.0");
    fixture.skill("insufficient", "1.0.0");
    fixture.skill("unreadable", "1.0.0");
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = vec!["root".to_string()];
    lock.skills = vec![
        locked_child("good", Some(good)),
        locked_child("edited", Some("original".to_string())),
        locked_child("revision", None),
        locked_child("editable", None),
        locked_child("insufficient", None),
        locked_child("unreadable", Some("original".to_string())),
    ];
    let mut root = locked_child("root", None);
    root.depth = 0;
    root.parent_skill = None;
    root.required_by.clear();
    root.dependencies = lock.skills.iter().map(|entry| entry.id.clone()).collect();
    lock.skills.push(root);
    let service = fixture.service(&lock).await;
    fs::remove_dir_all(fixture.storage.join("unreadable")).unwrap();
    let mut args = json_args();
    args.only = Some(vec!["dev".to_string()]);
    let (result, output) = crate::output::capture(execute_list(&service, args, false)).await;
    let error = result.unwrap_err().to_string();
    let rows: Vec<Value> = serde_json::from_str(&output).unwrap();
    for (id, expected) in [
        ("root", "ok"),
        ("good", "ok"),
        ("editable", "ok"),
        ("edited", "content-mismatch"),
        ("revision", "revision-mismatch"),
        ("insufficient", "insufficient-integrity"),
        ("unreadable", "integrity-error"),
        ("latest", "excluded"),
    ] {
        assert_eq!(row(&rows, id)["reconciliation"], expected, "{id}");
        if !matches!(expected, "ok" | "excluded") {
            assert!(error.contains(&format!("{id}: {expected}")), "{error}");
        }
    }
    assert_eq!(row(&rows, "good")["owners"], serde_json::json!(["root"]));
    assert_eq!(row(&rows, "good")["groups"], serde_json::json!(["dev"]));
    assert_eq!(row(&rows, "good")["missing_from_manifest"], false);
    assert_eq!(row(&rows, "editable")["mutable"], true);
    assert_eq!(row(&rows, "latest")["desired_constraint"], "latest");

    let mut args = json_args();
    args.without = Some(vec!["dev".to_string()]);
    let (result, output) = crate::output::capture(execute_list(&service, args, false)).await;
    let error = result.unwrap_err().to_string();
    assert!(error.contains("latest: missing-lock"));
    assert!(!error.contains("edited:"));
    let rows: Vec<Value> = serde_json::from_str(&output).unwrap();
    assert_eq!(row(&rows, "edited")["reconciliation"], "excluded");
}

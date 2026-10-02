#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! `project repin` rewrites legacy content digests from the installed content (ADR-0017).

use chrono::Utc;
use fastskill_core::core::content_digest::{
    content_digest, legacy_content_digest, CONTENT_DIGEST_PREFIX,
};
use fastskill_core::core::lock::{
    GlobalLockedSkillEntry, GlobalSkillsLock, ProjectLockedSkillEntry, ProjectSkillsLock,
};
use fastskill_core::core::origin::{Origin, Resolved};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const BUNDLE_FORMAT: &str = "fastskill-bundle-v1";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("HOME", root.join("home"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_exit(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
}

fn write_skill(directory: &Path, id: &str, body: &str) {
    fs::create_dir_all(directory.join("scripts")).unwrap();
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {id}\nversion: 1.0.0\ndescription: test\n---\n{body}\n"),
    )
    .unwrap();
    fs::write(directory.join("scripts/run.sh"), format!("echo {id}\n")).unwrap();
}

fn project_entry(id: &str, checksum: &str) -> ProjectLockedSkillEntry {
    ProjectLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin: Origin::Local {
            path: PathBuf::from(format!("sources/{id}")),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(checksum.to_string()),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        depth: 0,
        parent_skill: None,
        required_by: Vec::new(),
    }
}

/// A project whose `skills.lock` records each skill with the legacy digest of `skills/<id>`.
/// The skills named in `installed` are written first; `modified` ones are changed after
/// their digest was taken; the rest are recorded but never installed.
fn legacy_project(root: &Path, installed: &[&str], modified: &[&str], missing: &[&str]) {
    let skills = root.join("skills");
    let mut dependencies = String::new();
    let mut entries = Vec::new();
    for id in installed.iter().chain(modified) {
        write_skill(&skills.join(id), id, "packaged");
        entries.push(project_entry(
            id,
            &legacy_content_digest(&skills.join(id)).unwrap(),
        ));
        dependencies.push_str(&format!(
            "{id} = {{ source = \"local\", path = \"sources/{id}\" }}\n"
        ));
    }
    for id in modified {
        fs::write(skills.join(id).join("SKILL.md"), "edited locally\n").unwrap();
    }
    for id in missing {
        entries.push(project_entry(id, &"ab".repeat(32)));
    }
    fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n{dependencies}"
        ),
    )
    .unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = entries.iter().map(|entry| entry.id.clone()).collect();
    lock.skills = entries;
    lock.save_to_file(&root.join("skills.lock")).unwrap();
}

/// Every skill file under `root`. The writer lease's lock file is not skill content.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file() && entry.file_name() != ".fastskill-state.lock")
        .map(|entry| {
            (
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn checksum(root: &Path, id: &str) -> String {
    ProjectSkillsLock::load_from_file(&root.join("skills.lock"))
        .unwrap()
        .skills
        .into_iter()
        .find(|entry| entry.id == id)
        .unwrap()
        .resolved
        .checksum
        .unwrap()
}

#[test]
fn repin_rewrites_legacy_checksums_of_installed_skills_without_touching_content() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    legacy_project(root, &["alpha", "beta"], &[], &[]);
    let content = tree(&root.join("skills"));
    let legacy_lock = fs::read_to_string(root.join("skills.lock")).unwrap();

    let output = run(root, &["project", "repin"]);

    assert_exit(&output, 0);
    assert!(
        stdout(&output).contains("Re-pinned 2 legacy content digest(s)"),
        "{}",
        stdout(&output)
    );
    let mut expected_lock = legacy_lock;
    for id in ["alpha", "beta"] {
        let installed = root.join("skills").join(id);
        let current = checksum(root, id);
        assert!(current.starts_with(CONTENT_DIGEST_PREFIX), "{current}");
        assert_eq!(current, content_digest(&installed).unwrap());
        expected_lock =
            expected_lock.replace(&legacy_content_digest(&installed).unwrap(), &current);
    }
    // Only the digests change; every other byte of the Lock is kept.
    assert_eq!(
        fs::read_to_string(root.join("skills.lock")).unwrap(),
        expected_lock
    );
    assert_eq!(tree(&root.join("skills")), content);
    assert!(!root.join(".fastskill/recovery-required").exists());
}

#[test]
fn repin_leaves_mismatched_and_uninstalled_entries_and_reports_them() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    legacy_project(root, &["alpha"], &["beta"], &["gamma"]);
    let beta_legacy = checksum(root, "beta");

    let output = run(root, &["project", "repin"]);

    assert_exit(&output, 1);
    let report = stdout(&output);
    assert!(
        report.contains("Re-pinned 1 legacy content digest(s)"),
        "{report}"
    );
    assert!(report.contains("Left as is (2)"), "{report}");
    assert!(
        report.contains(
            "skill beta (skills.lock): installed content does not match the legacy digest"
        ),
        "{report}"
    );
    assert!(
        report.contains("skill gamma (skills.lock): content is not installed"),
        "{report}"
    );
    assert!(
        stderr(&output).contains("2 legacy content digest(s) remain"),
        "{}",
        stderr(&output)
    );
    assert!(checksum(root, "alpha").starts_with(CONTENT_DIGEST_PREFIX));
    assert_eq!(checksum(root, "beta"), beta_legacy);
    assert_eq!(checksum(root, "gamma"), "ab".repeat(32));
    assert_eq!(
        fs::read_to_string(root.join("skills/beta/SKILL.md")).unwrap(),
        "edited locally\n"
    );
}

#[test]
fn repin_check_changes_nothing_and_fails_until_the_values_are_repinned() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    legacy_project(root, &["alpha"], &[], &[]);
    let legacy_lock = fs::read(root.join("skills.lock")).unwrap();

    let check = run(root, &["project", "repin", "--check"]);

    assert_exit(&check, 1);
    assert!(
        stdout(&check).contains("can be re-pinned"),
        "{}",
        stdout(&check)
    );
    assert!(
        stderr(&check).contains("run `fastskill project repin`"),
        "{}",
        stderr(&check)
    );
    assert_eq!(fs::read(root.join("skills.lock")).unwrap(), legacy_lock);
    assert!(!root.join(".fastskill").exists());

    assert_exit(&run(root, &["project", "repin"]), 0);
    let after = run(root, &["project", "repin", "--check"]);
    assert_exit(&after, 0);
    assert!(stdout(&after).contains("No legacy content digests found."));
}

#[test]
fn repin_a_second_time_changes_nothing() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    legacy_project(root, &["alpha"], &[], &[]);
    assert_exit(&run(root, &["project", "repin"]), 0);
    let repinned = fs::read(root.join("skills.lock")).unwrap();

    let again = run(root, &["project", "repin"]);

    assert_exit(&again, 0);
    assert!(stdout(&again).contains("No legacy content digests found."));
    assert_eq!(fs::read(root.join("skills.lock")).unwrap(), repinned);
}

#[test]
fn repin_json_reports_each_record_in_the_lifecycle_envelope() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    legacy_project(root, &["alpha"], &["beta"], &[]);
    let legacy_lock = fs::read(root.join("skills.lock")).unwrap();

    let output = run(root, &["project", "repin", "--check", "--json"]);

    assert_exit(&output, 1);
    let result: serde_json::Value = serde_json::from_str(stdout(&output).trim()).unwrap();
    assert_eq!(result["scope"], "project");
    assert_eq!(result["outcome"], "blocked");
    assert_eq!(result["dry_run"], true);
    let targets = result["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    let alpha = targets
        .iter()
        .find(|target| target["id"] == "alpha")
        .unwrap();
    assert_eq!(alpha["record"], "skill");
    assert_eq!(alpha["outcome"], "changed");
    assert_eq!(
        alpha["target_revision"],
        content_digest(&root.join("skills/alpha")).unwrap()
    );
    let beta = targets
        .iter()
        .find(|target| target["id"] == "beta")
        .unwrap();
    assert_eq!(beta["outcome"], "blocked");
    assert!(beta["target_revision"].is_null());
    assert_eq!(
        beta["retained"][0],
        "installed content does not match the legacy digest"
    );
    assert_eq!(fs::read(root.join("skills.lock")).unwrap(), legacy_lock);

    let written = run(root, &["project", "repin", "--json"]);
    assert_exit(&written, 1);
    let result: serde_json::Value = serde_json::from_str(stdout(&written).trim()).unwrap();
    assert_eq!(result["dry_run"], false);
    assert!(checksum(root, "alpha").starts_with(CONTENT_DIGEST_PREFIX));

    let repinnable = TempDir::new().unwrap();
    legacy_project(repinnable.path(), &["alpha"], &[], &[]);
    let changed = run(repinnable.path(), &["project", "repin", "--json"]);
    assert_exit(&changed, 0);
    let result: serde_json::Value = serde_json::from_str(stdout(&changed).trim()).unwrap();
    assert_eq!(result["outcome"], "changed");

    let clean = TempDir::new().unwrap();
    legacy_project(clean.path(), &[], &[], &[]);
    let unchanged = run(clean.path(), &["project", "repin", "--json"]);
    assert_exit(&unchanged, 0);
    let result: serde_json::Value = serde_json::from_str(stdout(&unchanged).trim()).unwrap();
    assert_eq!(result["outcome"], "unchanged");
}

#[test]
fn repin_needs_a_project_and_treats_a_missing_lock_as_nothing_to_do() {
    let empty = TempDir::new().unwrap();
    let output = run(empty.path(), &["project", "repin"]);
    assert_exit(&output, 2);
    assert!(
        stderr(&output).contains("skill-project.toml not found"),
        "{}",
        stderr(&output)
    );

    fs::write(
        empty.path().join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \"skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    let output = run(empty.path(), &["project", "repin", "--check"]);
    assert_exit(&output, 0);
    assert!(stdout(&output).contains("No legacy content digests found."));
}

fn global_entry(id: &str, checksum: &str) -> GlobalLockedSkillEntry {
    GlobalLockedSkillEntry {
        id: id.to_string(),
        name: id.to_string(),
        origin: Origin::Local {
            path: PathBuf::from(format!("/sources/{id}")),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(checksum.to_string()),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        installed_at: Utc::now(),
        last_checked_at: None,
        last_updated_at: None,
    }
}

#[test]
fn global_repin_rewrites_the_global_lock_under_the_config_directory() {
    let home = TempDir::new().unwrap();
    let root = home.path();
    let state = root.join("config/fastskill");
    write_skill(&state.join("skills/alpha"), "alpha", "global");
    write_skill(&state.join("skills/beta"), "beta", "global");
    let alpha_legacy = legacy_content_digest(&state.join("skills/alpha")).unwrap();
    let beta_legacy = legacy_content_digest(&state.join("skills/beta")).unwrap();
    fs::write(state.join("skills/beta/SKILL.md"), "edited\n").unwrap();
    let mut lock = GlobalSkillsLock::new_empty();
    lock.skills = vec![
        global_entry("alpha", &alpha_legacy),
        global_entry("beta", &beta_legacy),
    ];
    lock.save_to_file(&state.join("global-skills.lock"))
        .unwrap();
    let content = tree(&state.join("skills"));

    let check = run(root, &["--global", "project", "repin", "--check"]);
    assert_exit(&check, 1);
    assert!(
        stdout(&check).contains("Run `fastskill --global project repin`"),
        "{}",
        stdout(&check)
    );

    let output = run(root, &["--global", "project", "repin"]);

    assert_exit(&output, 1);
    assert!(
        stdout(&output).contains("skill beta (global-skills.lock)"),
        "{}",
        stdout(&output)
    );
    let lock = GlobalSkillsLock::load_from_file(&state.join("global-skills.lock")).unwrap();
    let recorded = |id: &str| {
        lock.skills
            .iter()
            .find(|entry| entry.id == id)
            .and_then(|entry| entry.resolved.checksum.clone())
            .unwrap()
    };
    assert_eq!(
        recorded("alpha"),
        content_digest(&state.join("skills/alpha")).unwrap()
    );
    assert_eq!(recorded("beta"), beta_legacy);
    assert_eq!(tree(&state.join("skills")), content);

    let json = run(root, &["--global", "project", "repin", "--json"]);
    let result: serde_json::Value = serde_json::from_str(stdout(&json).trim()).unwrap();
    assert_eq!(result["scope"], "global");

    let conflicting = run(
        root,
        &[
            "--global",
            "--skills-dir",
            root.to_str().unwrap(),
            "project",
            "repin",
        ],
    );
    assert_exit(&conflicting, 1);
    assert!(stderr(&conflicting).contains("--global and --skills-dir cannot be used together"));
}

// ── Bundles ────────────────────────────────────────────────────────────────────

fn build_bundle(author: &Path, overridable: bool) -> PathBuf {
    write_skill(
        &author.join("skills/code-review"),
        "code-review",
        "packaged",
    );
    fs::write(
        author.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \"skills\"\n\n[bundle]\nformat = \"{BUNDLE_FORMAT}\"\nid = \"team\"\nversion = \"1.0.0\"\n\n[bundle.members.code-review]\noverridable = {overridable}\n\n[dependencies]\ncode-review = \"1.0.0\"\n"
        ),
    )
    .unwrap();
    let dist = author.join("dist");
    assert_exit(
        &run(
            author,
            &["bundle", "build", "--output", dist.to_str().unwrap()],
        ),
        0,
    );
    dist.join("team-1.0.0.zip")
}

fn recipient(root: &Path, artifact: &Path) {
    fs::create_dir_all(root.join(".claude/skills")).unwrap();
    fs::write(
        root.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .unwrap();
    assert_exit(
        &run(root, &["bundle", "add", artifact.to_str().unwrap()]),
        0,
    );
}

fn hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

/// The release digest of bundle `team@1.0.0` with one member, as fastskill computes it.
fn release_digest(member_digest: &str, overridable: bool) -> String {
    let mut hasher = Sha256::new();
    for field in [BUNDLE_FORMAT, "team", "1.0.0", "code-review", member_digest] {
        hash_field(&mut hasher, field);
    }
    hash_field(
        &mut hasher,
        if overridable {
            "overridable"
        } else {
            "required"
        },
    );
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn replace_all(text: &str, replacements: &[(&str, &str)]) -> String {
    replacements
        .iter()
        .fold(text.to_string(), |text, (from, to)| text.replace(from, to))
}

/// Rewrite `path` as an older fastskill would have written it.
fn legacy_file(path: &Path, replacements: &[(&str, &str)]) {
    let text = fs::read_to_string(path).unwrap();
    fs::write(path, replace_all(&text, replacements)).unwrap();
}

/// Rewrite the archive lock inside a bundle artifact as an older `bundle build` wrote it.
fn legacy_artifact(path: &Path, replacements: &[(&str, &str)]) {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        if file.name() == "skills.lock" {
            bytes = replace_all(&String::from_utf8(bytes).unwrap(), replacements).into_bytes();
        }
        entries.push((
            file.name().to_string(),
            file.unix_mode().unwrap_or(0o644),
            bytes,
        ));
    }
    drop(archive);
    let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, mode, bytes) in entries {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default().unix_permissions(mode),
            )
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn repin_rewrites_legacy_bundle_records_and_reports_the_legacy_artifact() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), false);
    let project = TempDir::new().unwrap();
    let root = project.path();
    recipient(root, &artifact);
    let installed = root.join(".claude/skills/code-review");
    let member = content_digest(&installed).unwrap();
    let member_legacy = legacy_content_digest(&installed).unwrap();
    let release = release_digest(&member, false);
    let release_legacy = release_digest(&member_legacy, false);
    let lock_path = root.join("skills.lock");
    let history_path = root.join(".fastskill/bundle-history.toml");
    let cached = root.join(".fastskill/bundles/team-1.0.0.zip");
    let current_lock = fs::read_to_string(&lock_path).unwrap();
    let current_history = fs::read_to_string(&history_path).unwrap();
    assert!(current_lock.contains(&release), "{current_lock}");
    assert!(current_history.contains(&release), "{current_history}");
    let legacy = [
        (member.as_str(), member_legacy.as_str()),
        (release.as_str(), release_legacy.as_str()),
    ];
    legacy_file(&lock_path, &legacy);
    legacy_file(&history_path, &legacy);
    legacy_artifact(&cached, &legacy);
    let content = tree(&root.join(".claude/skills"));

    let check = run(root, &["project", "repin", "--check"]);
    assert_exit(&check, 1);
    let report = stdout(&check);
    assert!(
        report.contains("bundle team@1.0.0 (skills.lock)"),
        "{report}"
    );
    assert!(
        report.contains("bundle history team@1.0.0 (bundle-history.toml)"),
        "{report}"
    );
    assert!(
        report.contains(".fastskill/bundles/team-1.0.0.zip"),
        "{report}"
    );
    assert!(report.contains("`fastskill bundle build`"), "{report}");

    let json = run(root, &["project", "repin", "--check", "--json"]);
    assert_exit(&json, 1);
    let result: serde_json::Value = serde_json::from_str(stdout(&json).trim()).unwrap();
    assert_eq!(result["outcome"], "blocked");
    let bundle = result["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["record"] == "bundle")
        .unwrap();
    assert_eq!(bundle["id"], "team@1.0.0");
    assert_eq!(bundle["current_revision"], release_legacy.as_str());
    assert_eq!(bundle["target_revision"], release.as_str());
    assert_eq!(
        bundle["changes"],
        serde_json::json!(["digest", "member:code-review"])
    );
    assert_eq!(
        result["legacy_artifacts"],
        serde_json::json!([".fastskill/bundles/team-1.0.0.zip"])
    );
    assert!(
        result["diagnostics"][0]
            .as_str()
            .unwrap()
            .contains("rebuild it with `fastskill bundle build`"),
        "{result}"
    );

    let output = run(root, &["project", "repin"]);

    // The records are current again; the immutable artifact still holds legacy digests.
    assert_exit(&output, 1);
    assert!(
        stdout(&output).contains("Re-pinned 2 legacy content digest(s)"),
        "{}",
        stdout(&output)
    );
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), current_lock);
    assert_eq!(fs::read_to_string(&history_path).unwrap(), current_history);
    assert_eq!(tree(&root.join(".claude/skills")), content);
    assert!(
        stderr(&output).contains("1 legacy content digest(s) remain"),
        "{}",
        stderr(&output)
    );

    // The cached artifact still restores the repinned Lock.
    assert_exit(&run(root, &["project", "install", "--lock"]), 0);
}

#[test]
fn repin_checks_an_overridden_member_against_its_cached_artifact() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), true);
    let project = TempDir::new().unwrap();
    let root = project.path();
    recipient(root, &artifact);
    let packaged = root.join("packaged");
    copy_tree(&root.join(".claude/skills/code-review"), &packaged);
    let personal = root.join("personal");
    write_skill(&personal, "code-review", "personal");
    assert_exit(
        &run(
            root,
            &[
                "bundle",
                "override",
                "code-review",
                "--from",
                personal.to_str().unwrap(),
            ],
        ),
        0,
    );
    let installed = root.join(".claude/skills/code-review");
    let member = content_digest(&packaged).unwrap();
    let member_legacy = legacy_content_digest(&packaged).unwrap();
    let local = content_digest(&installed).unwrap();
    let local_legacy = legacy_content_digest(&installed).unwrap();
    let release = release_digest(&member, true);
    let release_legacy = release_digest(&member_legacy, true);
    let lock_path = root.join("skills.lock");
    let current_lock = fs::read_to_string(&lock_path).unwrap();
    assert!(current_lock.contains(&local), "{current_lock}");
    let legacy = [
        (member.as_str(), member_legacy.as_str()),
        (local.as_str(), local_legacy.as_str()),
        (release.as_str(), release_legacy.as_str()),
    ];
    legacy_file(&lock_path, &legacy);

    // Without its cached artifact the overridden member's packaged content is unknown.
    let cached = root.join(".fastskill/bundles/team-1.0.0.zip");
    let saved = fs::read(&cached).unwrap();
    fs::remove_file(&cached).unwrap();
    let missing = run(root, &["project", "repin", "--check"]);
    assert_exit(&missing, 1);
    assert!(
        stdout(&missing).contains("member 'code-review': the member is overridden"),
        "{}",
        stdout(&missing)
    );
    fs::write(&cached, saved).unwrap();

    let output = run(root, &["project", "repin"]);

    assert_exit(&output, 0);
    assert!(
        stdout(&output).contains("override code-review (skills.lock)"),
        "{}",
        stdout(&output)
    );
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), current_lock);
}

#[test]
fn repin_leaves_a_bundle_whose_member_content_changed() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), false);
    let project = TempDir::new().unwrap();
    let root = project.path();
    recipient(root, &artifact);
    let installed = root.join(".claude/skills/code-review");
    let member = content_digest(&installed).unwrap();
    let member_legacy = legacy_content_digest(&installed).unwrap();
    let release = release_digest(&member, false);
    let lock_path = root.join("skills.lock");
    // A release digest that does not hash the recorded members is left alone.
    legacy_file(&lock_path, &[(member.as_str(), member_legacy.as_str())]);
    let unmatched = run(root, &["project", "repin", "--check"]);
    assert!(
        stdout(&unmatched).contains("release digest does not match its member digests"),
        "{}",
        stdout(&unmatched)
    );
    legacy_file(
        &lock_path,
        &[(release.as_str(), &release_digest(&member_legacy, false))],
    );
    fs::write(installed.join("SKILL.md"), "edited\n").unwrap();
    let legacy_lock = fs::read(&lock_path).unwrap();

    let output = run(root, &["project", "repin"]);

    assert_exit(&output, 1);
    assert!(
        stdout(&output)
            .contains("member 'code-review': installed content does not match the legacy digest"),
        "{}",
        stdout(&output)
    );
    assert_eq!(fs::read(&lock_path).unwrap(), legacy_lock);
}

fn copy_tree(from: &Path, to: &Path) {
    for (relative, bytes) in tree(from) {
        let target = to.join(relative);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }
}

#[test]
fn repin_classifies_bundle_history_by_the_lock_entry_when_the_artifact_is_missing() {
    let author = TempDir::new().unwrap();
    let artifact = build_bundle(author.path(), false);
    let project = TempDir::new().unwrap();
    let root = project.path();
    recipient(root, &artifact);
    let installed = root.join(".claude/skills/code-review");
    let member = content_digest(&installed).unwrap();
    let member_legacy = legacy_content_digest(&installed).unwrap();
    let release = release_digest(&member, false);
    let release_legacy = release_digest(&member_legacy, false);
    let lock_path = root.join("skills.lock");
    let history_path = root.join(".fastskill/bundle-history.toml");
    let current_lock = fs::read_to_string(&lock_path).unwrap();
    let current_history = fs::read_to_string(&history_path).unwrap();
    let legacy = [
        (member.as_str(), member_legacy.as_str()),
        (release.as_str(), release_legacy.as_str()),
    ];
    legacy_file(&lock_path, &legacy);
    legacy_file(&history_path, &legacy);
    fs::remove_file(root.join(".fastskill/bundles/team-1.0.0.zip")).unwrap();
    let skill = fs::read(installed.join("SKILL.md")).unwrap();
    fs::write(installed.join("SKILL.md"), "edited\n").unwrap();

    // While the bundle entry cannot be re-pinned, neither can the release it names.
    let blocked = run(root, &["project", "repin", "--check"]);
    assert_exit(&blocked, 1);
    assert!(
        stdout(&blocked).contains(
            "bundle history team@1.0.0 (bundle-history.toml): the cached artifact is missing"
        ),
        "{}",
        stdout(&blocked)
    );

    fs::write(installed.join("SKILL.md"), skill).unwrap();
    let output = run(root, &["project", "repin"]);

    assert_exit(&output, 0);
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), current_lock);
    assert_eq!(fs::read_to_string(&history_path).unwrap(), current_history);
}

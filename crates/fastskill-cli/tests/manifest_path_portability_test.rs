#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A Manifest written by `add` must be portable across machines.
//!
//! README.md sells "a committed skill-project.toml + skills.lock so every
//! teammate and CI gets the identical skill set". `add ./src/alpha-skill` used
//! to record the *canonicalized absolute* path of the target:
//!
//! ```toml
//! [dependencies.alpha-skill.origin]
//! type = "local"
//! path = "/tmp/.../session-scoped/fsdemo/src/alpha-skill"
//! ```
//!
//! which resolves on exactly one machine. Every teammate and every CI run got a
//! path that did not exist, and the failure surfaced later, from `install`, as
//! `Local path does not exist: …` — naming the absent directory rather than the
//! cause.
//!
//! A local origin inside the project tree is therefore stored **relative to the
//! Manifest directory** and resolved back to absolute against the Manifest's own
//! location at use time. Absolute paths keep working on read (back-compat), and
//! an out-of-tree target — which genuinely cannot be made portable — keeps its
//! absolute path but is announced with a warning naming the Manifest field.
//!
//! The load-bearing test here is [`relocated_project_installs`]: a grep over the
//! written Manifest proves only what was written, never that it *travels*. The
//! relocation test copies the whole project elsewhere, removes the original, and
//! installs from the copy.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const SKILL_MD: &str =
    "---\nname: alpha-skill\nversion: \"1.0.0\"\ndescription: A demo skill\n---\nBody\n";

/// A temp project root, canonicalized so comparisons against the paths the CLI
/// writes (which are canonicalized) are not defeated by a symlinked `/tmp`.
fn new_project(skill_subpath: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path().canonicalize().expect("canonicalize temp dir");
    write_project_files(&root, skill_subpath);
    (tmp, root)
}

fn write_project_files(root: &Path, skill_subpath: &str) {
    std::fs::write(
        root.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n[dependencies]\n",
    )
    .expect("write manifest");
    std::fs::create_dir_all(root.join(".claude/skills")).expect("create skills dir");
    let skill_dir = root.join(skill_subpath);
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    std::fs::write(skill_dir.join("SKILL.md"), SKILL_MD).expect("write SKILL.md");
}

fn run(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(args)
        .current_dir(project)
        .output()
        .expect("run fastskill")
}

fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn assert_ok(out: &Output, what: &str) {
    assert!(
        out.status.success(),
        "{what} should succeed, got {:?}:\n{}",
        out.status,
        combined(out)
    );
}

fn manifest_of(project: &Path) -> String {
    std::fs::read_to_string(project.join("skill-project.toml")).expect("read manifest")
}

fn lock_of(project: &Path) -> String {
    std::fs::read_to_string(project.join("skills.lock")).expect("read lock")
}

/// Copy a directory tree recursively (symlinks are followed, matching what a
/// `git clone` of a committed project would produce).
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dst");
    for entry in std::fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("dir entry");
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

// ── 1. write side: the Manifest records a relative path ───────────────────────

#[test]
fn add_writes_manifest_relative_local_path() {
    let (_tmp, root) = new_project("src/alpha-skill");

    let out = run(&root, &["add", "./src/alpha-skill"]);
    assert_ok(&out, "add");

    let manifest = manifest_of(&root);
    assert!(
        manifest.contains(r#"path = "src/alpha-skill""#),
        "manifest must record the path relative to its own directory:\n{manifest}"
    );
    assert!(
        !manifest.contains(&root.display().to_string()),
        "manifest must not embed the machine-local project root {}:\n{manifest}",
        root.display()
    );

    // The Lock is committed alongside the Manifest and is what `install --lock`
    // reads, so it must be portable too.
    let lock = lock_of(&root);
    assert!(
        lock.contains(r#"path = "src/alpha-skill""#),
        "skills.lock must record the relative path too:\n{lock}"
    );
    assert!(
        !lock.contains(&root.display().to_string()),
        "skills.lock must not embed the machine-local project root:\n{lock}"
    );
}

// ── 2. THE REAL TEST: the project still installs somewhere else ───────────────

#[test]
fn relocated_project_installs() {
    let (_tmp_a, root_a) = new_project("src/alpha-skill");
    assert_ok(&run(&root_a, &["add", "./src/alpha-skill"]), "add");

    // A teammate clones the committed project to a different path; the path the
    // author added from does not exist for them at all.
    let tmp_b = TempDir::new().expect("temp dir b");
    let root_b = tmp_b
        .path()
        .canonicalize()
        .expect("canonicalize")
        .join("elsewhere/checkout");
    copy_tree(&root_a, &root_b);
    std::fs::remove_dir_all(root_b.join(".claude/skills")).expect("clear skills dir");
    std::fs::create_dir_all(root_b.join(".claude/skills")).expect("recreate skills dir");
    std::fs::remove_dir_all(&root_a).expect("remove the original checkout");

    let out = run(&root_b, &["install"]);
    assert_ok(&out, "install in the relocated project");
    assert!(
        root_b.join(".claude/skills/alpha-skill/SKILL.md").is_file(),
        "the skill must be installed in the relocated project:\n{}",
        combined(&out)
    );
}

#[test]
fn relocated_project_installs_from_lock() {
    let (_tmp_a, root_a) = new_project("src/alpha-skill");
    assert_ok(&run(&root_a, &["add", "./src/alpha-skill"]), "add");

    let tmp_b = TempDir::new().expect("temp dir b");
    let root_b = tmp_b
        .path()
        .canonicalize()
        .expect("canonicalize")
        .join("elsewhere/ci-runner");
    copy_tree(&root_a, &root_b);
    std::fs::remove_dir_all(root_b.join(".claude/skills")).expect("clear skills dir");
    std::fs::create_dir_all(root_b.join(".claude/skills")).expect("recreate skills dir");
    std::fs::remove_dir_all(&root_a).expect("remove the original checkout");

    let out = run(&root_b, &["install", "--lock"]);
    assert_ok(&out, "install --lock in the relocated project");
    assert!(
        root_b.join(".claude/skills/alpha-skill/SKILL.md").is_file(),
        "the skill must be installed from the Lock in the relocated project:\n{}",
        combined(&out)
    );
}

// ── 3. back-compat: an already-committed absolute path still installs ─────────

#[test]
fn preexisting_absolute_path_manifest_still_installs() {
    let (_tmp, root) = new_project("src/alpha-skill");

    // A Manifest written by an older fastskill: absolute path, hand-written here
    // exactly as that version would have left it.
    let absolute = root.join("src/alpha-skill");
    std::fs::write(
        root.join("skill-project.toml"),
        format!(
            "[tool.fastskill]\nskills_directory = \".claude/skills\"\n\n\
             [dependencies.alpha-skill.origin]\ntype = \"local\"\npath = \"{}\"\n",
            absolute.display()
        ),
    )
    .expect("write legacy manifest");

    let out = run(&root, &["install"]);
    assert_ok(&out, "install from a legacy absolute-path manifest");
    assert!(
        root.join(".claude/skills/alpha-skill/SKILL.md").is_file(),
        "an absolute path must keep working on read:\n{}",
        combined(&out)
    );
}

// ── 4. out-of-tree: unportable by construction, so say so ─────────────────────

#[test]
fn out_of_tree_local_path_stays_absolute_and_warns() {
    let (_tmp, root) = new_project("src/alpha-skill");

    // The skill lives outside the project tree, so no relative path can make the
    // Manifest portable.
    let outside = TempDir::new().expect("temp dir");
    let outside_root = outside.path().canonicalize().expect("canonicalize");
    let outside_skill = outside_root.join("beta-skill");
    std::fs::create_dir_all(&outside_skill).expect("create outside skill dir");
    std::fs::write(
        outside_skill.join("SKILL.md"),
        SKILL_MD.replace("alpha-skill", "beta-skill"),
    )
    .expect("write SKILL.md");

    let out = run(&root, &["add", outside_skill.to_str().expect("utf-8 path")]);
    assert_ok(&out, "add from outside the project tree");

    let manifest = manifest_of(&root);
    assert!(
        manifest.contains(&outside_skill.display().to_string()),
        "an out-of-tree path has to stay absolute:\n{manifest}"
    );

    let output = combined(&out);
    assert!(
        output.contains("dependencies.beta-skill.origin.path"),
        "the warning must name the Manifest field that is not portable:\n{output}"
    );
    assert!(
        output.to_lowercase().contains("warning"),
        "the unportable path must be announced as a warning:\n{output}"
    );
}

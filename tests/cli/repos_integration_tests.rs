//! Integration tests for consolidated `repos` command workflows.
//!
//! These tests execute the CLI binary end-to-end and validate the repos
//! workflow matrix required by spec 026a.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::snapshot_helpers::{run_fastskill_command, run_fastskill_command_with_env};
use std::fs;
use tempfile::TempDir;

fn write_project_manifest(project_dir: &std::path::Path) {
    fs::write(
        project_dir.join("skill-project.toml"),
        "[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();
}

fn write_local_skill_repo(
    base: &std::path::Path,
    skill_id: &str,
    version: &str,
) -> std::path::PathBuf {
    let repo_dir = base.join("local-repo");
    let skill_dir = repo_dir.join(skill_id);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {skill_id}\ndescription: test skill\nversion: {version}\n---\n# {skill_id}\n"
        ),
    )
    .unwrap();
    repo_dir
}

#[test]
fn test_repos_complete_workflow_matrix() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let local_repo_path = write_local_skill_repo(temp_dir.path(), "matrix-skill", "1.0.0");

    let add = run_fastskill_command(
        &[
            "repos",
            "add",
            "matrix-local",
            "--repo-type",
            "local",
            local_repo_path.to_str().unwrap(),
        ],
        Some(temp_dir.path()),
    );
    assert!(
        add.success,
        "repos add failed: {}{}",
        add.stdout, add.stderr
    );
    assert!(add.stdout.contains("Added repository: matrix-local"));

    let list = run_fastskill_command(&["repos", "list", "--json"], Some(temp_dir.path()));
    assert!(
        list.success,
        "repos list failed: {}{}",
        list.stdout, list.stderr
    );
    assert!(list.stdout.contains("matrix-local"));

    let info = run_fastskill_command(
        &["repos", "info", "matrix-local", "--json"],
        Some(temp_dir.path()),
    );
    assert!(
        info.success,
        "repos info failed: {}{}",
        info.stdout, info.stderr
    );
    assert!(info.stdout.contains("\"name\": \"matrix-local\""));

    let update = run_fastskill_command(
        &["repos", "update", "matrix-local", "--priority", "3"],
        Some(temp_dir.path()),
    );
    assert!(
        update.success,
        "repos update failed: {}{}",
        update.stdout, update.stderr
    );
    assert!(update.stdout.contains("Updated repository: matrix-local"));

    let test = run_fastskill_command(&["repos", "test", "matrix-local"], Some(temp_dir.path()));
    assert!(
        test.success,
        "repos test failed: {}{}",
        test.stdout, test.stderr
    );
    assert!(test.stdout.contains("Testing repository: matrix-local"));

    // PRD 006 "Local Skill Cache" (US-005): `repos refresh` now does a real
    // on-disk index refresh via `SkillCache`. Point it at a scratch cache dir
    // so the test never touches the developer/CI machine's real cache.
    let cache_dir = TempDir::new().unwrap();
    let cache_dir_str = cache_dir.path().to_str().unwrap();

    let refresh_one = run_fastskill_command_with_env(
        &["repos", "refresh", "matrix-local"],
        &[("FASTSKILL_CACHE_DIR", cache_dir_str)],
        Some(temp_dir.path()),
    );
    assert!(
        refresh_one.success,
        "repos refresh <name> failed: {}{}",
        refresh_one.stdout, refresh_one.stderr
    );
    assert!(refresh_one
        .stdout
        .contains("Refreshed matrix-local: 1 skill"));
    assert!(cache_dir
        .path()
        .join("index")
        .join("matrix-local.json")
        .is_file());

    let refresh_all = run_fastskill_command_with_env(
        &["repos", "refresh"],
        &[("FASTSKILL_CACHE_DIR", cache_dir_str)],
        Some(temp_dir.path()),
    );
    assert!(
        refresh_all.success,
        "repos refresh failed: {}{}",
        refresh_all.stdout, refresh_all.stderr
    );
    assert!(refresh_all
        .stdout
        .contains("Refreshed matrix-local: 1 skill"));

    let skills = run_fastskill_command(
        &["repos", "skills", "--repository", "matrix-local"],
        Some(temp_dir.path()),
    );
    assert!(
        !skills.success,
        "repos skills should fail for local repo: {}{}",
        skills.stdout, skills.stderr
    );
    assert!(skills.stderr.contains("is not an HTTP registry"));

    let show = run_fastskill_command(
        &[
            "repos",
            "show",
            "matrix-skill",
            "--repository",
            "matrix-local",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        show.success,
        "repos show failed: {}{}",
        show.stdout, show.stderr
    );
    assert!(show.stdout.contains("Skill: matrix-skill"));

    let versions = run_fastskill_command(
        &[
            "repos",
            "versions",
            "matrix-skill",
            "--repository",
            "matrix-local",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        versions.success,
        "repos versions failed: {}{}",
        versions.stdout, versions.stderr
    );
    assert!(versions.stdout.contains("Available versions:"));
    assert!(versions.stdout.contains("1.0.0"));

    let remove = run_fastskill_command(&["repos", "remove", "matrix-local"], Some(temp_dir.path()));
    assert!(
        remove.success,
        "repos remove failed: {}{}",
        remove.stdout, remove.stderr
    );
    assert!(remove.stdout.contains("Removed repository: matrix-local"));
}

#[test]
fn test_repos_command_excludes_search_subcommand() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let repos_search = run_fastskill_command(&["repos", "search", "test"], Some(temp_dir.path()));
    assert!(!repos_search.success);
    // cli-framework's rejection wording for an unknown nested path differs
    // from clap's native "unrecognized subcommand '...'" phrasing (see the
    // command-layer migration, spec #89); assert on the current message.
    assert!(repos_search
        .stderr
        .contains("nested command path 'repos search test' not found"));

    let search_help = run_fastskill_command(&["search", "--help"], Some(temp_dir.path()));
    assert!(search_help.success);
    assert!(search_help.stdout.contains("Search skills by query"));
}

#[test]
fn test_repos_help_does_not_advertise_search() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let repos_help = run_fastskill_command(&["repos", "--help"], Some(temp_dir.path()));
    assert!(repos_help.success);
    assert!(!repos_help.stdout.contains(" repos search "));
    assert!(repos_help.stdout.contains("skills"));
    assert!(repos_help.stdout.contains("show"));
    assert!(repos_help.stdout.contains("versions"));
}

/// `repos skills <name>` (bare positional, no `--repository`) used to miss the
/// `execute_list_skills` domain logic entirely and fail during clap arg parsing
/// instead: no positional was declared, so it hit the generic
/// `error[E002]: unknown argument` path (never names the argument, generic
/// boilerplate hint) and printed the error twice (once from cli-framework's
/// `DiagnosticReporter`, once from fastskill's own top-level `Err` handler).
/// `repos skills` now declares an optional positional `REPOSITORY` arg as
/// shorthand for `--repository`, so the bare-positional form reaches the same
/// domain error as the flag form (see `test_repos_complete_workflow_matrix`'s
/// `repos skills --repository matrix-local` case) and prints it exactly once.
#[test]
fn test_repos_skills_positional_repository_reaches_domain_error() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let skills = run_fastskill_command(&["repos", "skills", "nosuchrepo"], Some(temp_dir.path()));

    assert!(
        !skills.success,
        "repos skills nosuchrepo should fail: {}{}",
        skills.stdout, skills.stderr
    );
    assert!(
        skills.stderr.contains("Repository 'nosuchrepo' not found"),
        "expected a domain-specific 'not found' error, got: {}",
        skills.stderr
    );
    assert!(
        !skills.stderr.contains("unknown argument"),
        "must not fall back to the generic clap unknown-argument path: {}",
        skills.stderr
    );
    // The E002 double-print regression: the diagnostic must appear once, not twice.
    let not_found_occurrences = skills.stderr.matches("not found").count();
    assert_eq!(
        not_found_occurrences, 1,
        "error message must be printed exactly once, got: {}",
        skills.stderr
    );
}

/// Passing both the positional shorthand and the explicit `--repository` flag
/// is ambiguous; the command should reject it with a clear conflict error
/// rather than silently preferring one.
#[test]
fn test_repos_skills_positional_and_flag_conflict() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let skills = run_fastskill_command(
        &["repos", "skills", "foo", "--repository", "bar"],
        Some(temp_dir.path()),
    );

    assert!(!skills.success);
    assert!(
        skills.stderr.contains("conflicts with"),
        "expected a conflict diagnostic, got: {}",
        skills.stderr
    );
}

/// General regression test for the double-print bug (independent of the
/// positional-`REPOSITORY` fix above): fastskill's `main.rs` used to
/// unconditionally `eprintln!("Error: {}", e)` on any `Err` from
/// `app.run_with_args(..)`, even though cli-framework's `DiagnosticReporter`
/// had already written the same diagnostic to stderr for `UsageError`s (parse
/// failures, unknown-nested-command, arg validation). An actually-unknown
/// flag — not covered by the positional shorthand — must still print its
/// `error[E002]` diagnostic exactly once.
#[test]
fn test_unknown_flag_diagnostic_is_not_printed_twice() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());

    let skills = run_fastskill_command(
        &["repos", "skills", "--this-flag-does-not-exist"],
        Some(temp_dir.path()),
    );

    assert!(!skills.success);
    let occurrences = skills.stderr.matches("unknown argument").count();
    assert_eq!(
        occurrences, 1,
        "error[E002] diagnostic must be printed exactly once, got: {}",
        skills.stderr
    );
}

// ── PRD 006 "Local Skill Cache", US-005: `repos refresh` real semantics ────

/// `refresh <name>` for a repository that was never added must fail with an
/// error and a non-zero exit — not the old fake-success message.
#[test]
fn test_repos_refresh_unknown_repository_fails() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());
    let cache_dir = TempDir::new().unwrap();

    let refresh = run_fastskill_command_with_env(
        &["repos", "refresh", "does-not-exist"],
        &[("FASTSKILL_CACHE_DIR", cache_dir.path().to_str().unwrap())],
        Some(temp_dir.path()),
    );

    assert!(
        !refresh.success,
        "refresh of an unknown repository must fail: {}{}",
        refresh.stdout, refresh.stderr
    );
    assert!(refresh.stderr.contains("does-not-exist"));
    assert!(refresh.stderr.contains("not found"));
}

/// Refreshing "all" repositories when one source fails (a `local` repository
/// pointing at a path that does not exist) must still refresh the remaining,
/// healthy sources — reporting each outcome — and exit non-zero overall.
#[test]
fn test_repos_refresh_all_partial_failure_still_refreshes_others_and_exits_nonzero() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());
    let cache_dir = TempDir::new().unwrap();
    let cache_dir_str = cache_dir.path().to_str().unwrap();

    let good_repo_path = write_local_skill_repo(temp_dir.path(), "healthy-skill", "1.0.0");
    let add_good = run_fastskill_command(
        &[
            "repos",
            "add",
            "healthy",
            "--repo-type",
            "local",
            good_repo_path.to_str().unwrap(),
        ],
        Some(temp_dir.path()),
    );
    assert!(add_good.success, "{}{}", add_good.stdout, add_good.stderr);

    let missing_path = temp_dir.path().join("this-path-does-not-exist");
    let add_bad = run_fastskill_command(
        &[
            "repos",
            "add",
            "broken",
            "--repo-type",
            "local",
            missing_path.to_str().unwrap(),
        ],
        Some(temp_dir.path()),
    );
    assert!(add_bad.success, "{}{}", add_bad.stdout, add_bad.stderr);

    let refresh_all = run_fastskill_command_with_env(
        &["repos", "refresh"],
        &[("FASTSKILL_CACHE_DIR", cache_dir_str)],
        Some(temp_dir.path()),
    );

    assert!(
        !refresh_all.success,
        "overall refresh must exit non-zero when any source fails: {}{}",
        refresh_all.stdout, refresh_all.stderr
    );
    // The healthy source still refreshed and reported its outcome...
    assert!(refresh_all.stdout.contains("Refreshed healthy: 1 skill"));
    assert!(cache_dir
        .path()
        .join("index")
        .join("healthy.json")
        .is_file());
    // ...and the broken source's failure was reported too, not swallowed.
    assert!(refresh_all.stderr.contains("broken"));
    assert!(!cache_dir.path().join("index").join("broken.json").exists());
}

/// A successful refresh persists a `SourceIndex` to disk under the resolved
/// cache root, containing the skill(s) the source advertised.
#[test]
fn test_repos_refresh_writes_source_index_to_disk() {
    let temp_dir = TempDir::new().unwrap();
    write_project_manifest(temp_dir.path());
    let cache_dir = TempDir::new().unwrap();

    let repo_path = write_local_skill_repo(temp_dir.path(), "indexed-skill", "2.3.4");
    let add = run_fastskill_command(
        &[
            "repos",
            "add",
            "idx-repo",
            "--repo-type",
            "local",
            repo_path.to_str().unwrap(),
        ],
        Some(temp_dir.path()),
    );
    assert!(add.success, "{}{}", add.stdout, add.stderr);

    let refresh = run_fastskill_command_with_env(
        &["repos", "refresh", "idx-repo"],
        &[("FASTSKILL_CACHE_DIR", cache_dir.path().to_str().unwrap())],
        Some(temp_dir.path()),
    );
    assert!(refresh.success, "{}{}", refresh.stdout, refresh.stderr);

    let index_path = cache_dir.path().join("index").join("idx-repo.json");
    let index_contents = fs::read_to_string(&index_path)
        .unwrap_or_else(|e| panic!("expected index file at {}: {e}", index_path.display()));
    assert!(index_contents.contains("indexed-skill"));
    assert!(index_contents.contains("2.3.4"));
    assert!(index_contents.contains("fetched_at"));
}

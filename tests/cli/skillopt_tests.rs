//! CLI integration tests for optimize commands

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
};
use std::fs;
use tempfile::TempDir;

// ── Help snapshots ────────────────────────────────────────────────────────────

#[test]
fn test_skillopt_help() {
    let result = run_fastskill_command(&["optimize", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings("optimize_help", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_skillopt_run_help() {
    let result = run_fastskill_command(&["optimize", "run", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings(
        "optimize_run_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_skillopt_resume_help() {
    let result = run_fastskill_command(&["optimize", "resume", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings(
        "optimize_resume_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_skillopt_status_help() {
    let result = run_fastskill_command(&["optimize", "status", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings(
        "optimize_status_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_skillopt_inspect_help() {
    let result = run_fastskill_command(&["optimize", "inspect", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings(
        "optimize_inspect_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_skillopt_export_help() {
    let result = run_fastskill_command(&["optimize", "export", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings(
        "optimize_export_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

// ── Config validation errors ──────────────────────────────────────────────────

#[test]
fn test_skillopt_run_config_missing() {
    let result = run_fastskill_command(
        &[
            "optimize",
            "run",
            "--config",
            "/tmp/nonexistent-skillopt-config-xyz.toml",
        ],
        None,
    );
    assert!(!result.success);
}

#[test]
fn test_skillopt_run_no_selection_cases() {
    let dir = TempDir::new().unwrap();
    let base = dir.path();

    // Write a minimal skill document
    fs::write(base.join("SKILL.md"), "# Test Skill").unwrap();

    // Write a suite CSV with only train cases (no selection)
    let suite_csv = "id,prompt,should_trigger,tags\ntrain-1,hello,true,train\n";
    fs::write(base.join("suite.csv"), suite_csv).unwrap();

    // Write a valid config
    let toml = r#"
skill = "SKILL.md"
skill_name = "test-skill"
suite = "suite.csv"
out_dir = ".skillopt/runs"
target_agent = "claude"
optimizer_agent = "claude"
n_epochs = 1
batch_size = 1
accumulation = 1
aggregate_group_size = 2
lr_0 = 2
pass_threshold = 0.5
gate_metric = "hard"
gate_trials = 1
gate_epsilon = 0.0
slow_update_mode = "gated"
protected_soft_cap_chars = 500
timeout_seconds = 30
"#;
    let config_path = base.join("skillopt.toml");
    fs::write(&config_path, toml).unwrap();

    let result = run_fastskill_command(
        &["optimize", "run", "--config", config_path.to_str().unwrap()],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("OPTIMIZE_NO_SELECTION_CASES"),
        "Expected OPTIMIZE_NO_SELECTION_CASES in: {}",
        combined
    );
}

/// Spec 013 finding #3: a suite written with only `selection` + `test` rows (the
/// undocumented-but-accepted shape a user reaches by reading the aikit source
/// directly) passes the pre-existing `selection_count > 0` check but has zero
/// `train` cases for the training loop to step over. `optimize run` must fail loudly
/// with a distinct error code instead of silently no-opping.
#[test]
fn test_skillopt_run_no_train_cases() {
    let dir = TempDir::new().unwrap();
    let base = dir.path();

    fs::write(base.join("SKILL.md"), "# Test Skill").unwrap();

    // Only selection + test rows — no train cases anywhere.
    let suite_csv = "id,prompt,should_trigger,split\n\
                      sel-1,hello,true,selection\n\
                      test-1,world,true,test\n";
    fs::write(base.join("suite.csv"), suite_csv).unwrap();

    let toml = r#"
skill = "SKILL.md"
skill_name = "test-skill"
suite = "suite.csv"
out_dir = ".skillopt/runs"
target_agent = "claude"
optimizer_agent = "claude"
n_epochs = 1
batch_size = 1
accumulation = 1
aggregate_group_size = 2
lr_0 = 2
pass_threshold = 0.5
gate_metric = "hard"
gate_trials = 1
gate_epsilon = 0.0
slow_update_mode = "gated"
protected_soft_cap_chars = 500
timeout_seconds = 30
"#;
    let config_path = base.join("skillopt.toml");
    fs::write(&config_path, toml).unwrap();

    let result = run_fastskill_command(
        &["optimize", "run", "--config", config_path.to_str().unwrap()],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("OPTIMIZE_NO_TRAIN_CASES"),
        "Expected OPTIMIZE_NO_TRAIN_CASES in: {}",
        combined
    );
}

#[test]
fn test_skillopt_run_mixed_weight_missing() {
    let dir = TempDir::new().unwrap();
    let base = dir.path();

    // Files don't need to exist — structural check runs first
    let toml = r#"
skill = "SKILL.md"
skill_name = "test-skill"
suite = "suite.csv"
out_dir = ".skillopt/runs"
target_agent = "claude"
n_epochs = 1
batch_size = 1
accumulation = 1
aggregate_group_size = 2
lr_0 = 2
pass_threshold = 0.5
gate_metric = "mixed"
gate_trials = 1
gate_epsilon = 0.0
slow_update_mode = "gated"
protected_soft_cap_chars = 500
timeout_seconds = 30
"#;
    let config_path = base.join("skillopt.toml");
    fs::write(&config_path, toml).unwrap();

    let result = run_fastskill_command(
        &["optimize", "run", "--config", config_path.to_str().unwrap()],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("SKILLOPT_MIXED_WEIGHT_MISSING"),
        "Expected SKILLOPT_MIXED_WEIGHT_MISSING in: {}",
        combined
    );
}

#[test]
fn test_skillopt_run_field_out_of_range() {
    let dir = TempDir::new().unwrap();
    let base = dir.path();

    let toml = r#"
skill = "SKILL.md"
skill_name = "test-skill"
suite = "suite.csv"
out_dir = ".skillopt/runs"
target_agent = "claude"
n_epochs = 1
batch_size = 1
accumulation = 1
aggregate_group_size = 2
lr_0 = 2
pass_threshold = 1.5
gate_metric = "hard"
gate_trials = 1
gate_epsilon = 0.0
slow_update_mode = "gated"
protected_soft_cap_chars = 500
timeout_seconds = 30
"#;
    let config_path = base.join("skillopt.toml");
    fs::write(&config_path, toml).unwrap();

    let result = run_fastskill_command(
        &["optimize", "run", "--config", config_path.to_str().unwrap()],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("SKILLOPT_FIELD_OUT_OF_RANGE"),
        "Expected SKILLOPT_FIELD_OUT_OF_RANGE in: {}",
        combined
    );
}

// ── Resume and export ─────────────────────────────────────────────────────────

/// Config TOML shared by the run→resume roundtrip test. `windsurf` is a
/// supported agent key that deploys skills but is NOT a runnable backend, so
/// `AikitEvalRunner` fails fast without spawning any subprocess — the training
/// loop still completes end-to-end (rollouts and gate calls score 0.0, steps
/// reject) exactly as aikit-skillopt's own `test_train_skill_end_to_end`
/// exercises it. No real agent CLI is needed on the machine or in CI.
fn roundtrip_config_toml() -> &'static str {
    r#"
skill = "SKILL.md"
skill_name = "test-skill"
suite = "suite.csv"
checks = "checks.toml"
out_dir = "runs"
target_agent = "windsurf"
optimizer_agent = "windsurf"
n_epochs = 1
batch_size = 1
accumulation = 1
aggregate_group_size = 2
lr_0 = 2
pass_threshold = 0.5
gate_metric = "hard"
gate_trials = 1
gate_epsilon = 0.0
slow_update_mode = "gated"
protected_soft_cap_chars = 500
timeout_seconds = 30
"#
}

/// The real writer/reader pair: `optimize resume` pointed at a run directory
/// that a real `optimize run` produced.
///
/// `run` resolves the config's skill/suite/checks paths against the CONFIG
/// file's directory, but `resume` resolves the stored optimize.toml against
/// the RUN directory. Before the archive fix, `run` wrote only optimize.toml
/// into the run dir, so every real resume died at validate_config with
/// SKILLOPT_SKILL_NOT_FOUND before the loop ever started. The older resume
/// test below hand-builds its run dir and so never caught this; this test
/// drives the layout `run` actually writes.
#[test]
fn test_skillopt_run_then_resume_real_layout() {
    let dir = TempDir::new().unwrap();
    let project = dir.path();

    fs::write(project.join("SKILL.md"), "# Test Skill\n\nDo the thing.\n").unwrap();
    fs::write(
        project.join("suite.csv"),
        "id,prompt,should_trigger,split\n\
         tr-1,hello,true,train\n\
         sel-1,world,true,selection\n",
    )
    .unwrap();
    fs::write(
        project.join("checks.toml"),
        "[[check]]\nname = \"trigger_expectation\"\npattern = \"fastskill\"\nexpected = true\n",
    )
    .unwrap();
    let config_path = project.join("optimize.toml");
    fs::write(&config_path, roundtrip_config_toml()).unwrap();

    let run_result = run_fastskill_command(
        &["optimize", "run", "--config", config_path.to_str().unwrap()],
        None,
    );
    assert!(
        run_result.success,
        "optimize run failed: {}{}",
        run_result.stdout, run_result.stderr
    );

    // Locate the timestamped run dir `run` allocated under out_dir.
    let runs_dir = project.join("runs");
    let run_dirs: Vec<_> = fs::read_dir(&runs_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(
        run_dirs.len(),
        1,
        "expected exactly one run dir under {}",
        runs_dir.display()
    );
    let run_dir = run_dirs[0].path();

    // The run dir must archive the exact inputs: the stored optimize.toml's
    // skill/suite/checks paths must resolve WITHIN the run dir to copies whose
    // content matches the originals.
    let stored: toml::Value =
        toml::from_str(&fs::read_to_string(run_dir.join("optimize.toml")).unwrap()).unwrap();
    for (key, original) in [
        ("skill", "SKILL.md"),
        ("suite", "suite.csv"),
        ("checks", "checks.toml"),
    ] {
        let rel = stored[key].as_str().unwrap();
        let archived = run_dir.join(rel);
        assert!(
            archived.exists(),
            "stored config's '{key}' path must resolve inside the run dir; {} missing",
            archived.display()
        );
        assert_eq!(
            fs::read_to_string(&archived).unwrap(),
            fs::read_to_string(project.join(original)).unwrap(),
            "archived {key} must be byte-identical to the input"
        );
    }

    // And the actual payoff: resume works on the layout run produced.
    let resume_result =
        run_fastskill_command(&["optimize", "resume", run_dir.to_str().unwrap()], None);
    let combined = format!("{}{}", resume_result.stdout, resume_result.stderr);
    assert!(
        !combined.contains("SKILLOPT_SKILL_NOT_FOUND")
            && !combined.contains("SKILLOPT_SUITE_NOT_FOUND"),
        "resume of a real run dir must not die on unresolvable input paths, got: {}",
        combined
    );
    assert!(
        resume_result.success,
        "optimize resume of a real run dir failed: {}",
        combined
    );
}

/// `optimize resume` loads the suite from the stored config the same way `run`
/// does — the same OPTIMIZE_NO_TRAIN_CASES gate from spec 013 finding #3 must apply
/// there too, not just on the initial `run`.
///
/// The run dir here is hand-built, which is now faithful: since the input-archive
/// fix, a real `run` copies SKILL.md/suite CSV into the run dir with optimize.toml
/// pointing at those copies — exactly this layout. (A suite this shape cannot be
/// produced through `run` itself, because `run` rejects it up front; resume can
/// still meet it if the archived suite was edited after the fact.)
#[test]
fn test_skillopt_resume_no_train_cases() {
    let dir = TempDir::new().unwrap();
    let run_dir = dir.path();

    fs::write(run_dir.join("SKILL.md"), "# Test Skill").unwrap();

    let suite_csv = "id,prompt,should_trigger,split\n\
                      sel-1,hello,true,selection\n\
                      test-1,world,true,test\n";
    fs::write(run_dir.join("suite.csv"), suite_csv).unwrap();

    let toml = r#"
skill = "SKILL.md"
skill_name = "test-skill"
suite = "suite.csv"
out_dir = ".skillopt/runs"
target_agent = "claude"
optimizer_agent = "claude"
n_epochs = 1
batch_size = 1
accumulation = 1
aggregate_group_size = 2
lr_0 = 2
pass_threshold = 0.5
gate_metric = "hard"
gate_trials = 1
gate_epsilon = 0.0
slow_update_mode = "gated"
protected_soft_cap_chars = 500
timeout_seconds = 30
"#;
    fs::write(run_dir.join("optimize.toml"), toml).unwrap();

    let result = run_fastskill_command(&["optimize", "resume", run_dir.to_str().unwrap()], None);
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("OPTIMIZE_NO_TRAIN_CASES"),
        "Expected OPTIMIZE_NO_TRAIN_CASES in: {}",
        combined
    );
}

/// Parity with `run`'s split gates: `run` checks selection_count before
/// train_count, and `resume` must apply the same OPTIMIZE_NO_SELECTION_CASES
/// guard rather than falling through to the training loop's later, differently
/// worded failure.
#[test]
fn test_skillopt_resume_no_selection_cases() {
    let dir = TempDir::new().unwrap();
    let run_dir = dir.path();

    fs::write(run_dir.join("SKILL.md"), "# Test Skill").unwrap();

    // Only train rows — no selection cases anywhere.
    let suite_csv = "id,prompt,should_trigger,split\n\
                      tr-1,hello,true,train\n\
                      tr-2,world,true,train\n";
    fs::write(run_dir.join("suite.csv"), suite_csv).unwrap();

    let toml = r#"
skill = "SKILL.md"
skill_name = "test-skill"
suite = "suite.csv"
out_dir = ".skillopt/runs"
target_agent = "claude"
optimizer_agent = "claude"
n_epochs = 1
batch_size = 1
accumulation = 1
aggregate_group_size = 2
lr_0 = 2
pass_threshold = 0.5
gate_metric = "hard"
gate_trials = 1
gate_epsilon = 0.0
slow_update_mode = "gated"
protected_soft_cap_chars = 500
timeout_seconds = 30
"#;
    fs::write(run_dir.join("optimize.toml"), toml).unwrap();

    let result = run_fastskill_command(&["optimize", "resume", run_dir.to_str().unwrap()], None);
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("OPTIMIZE_NO_SELECTION_CASES"),
        "Expected OPTIMIZE_NO_SELECTION_CASES in: {}",
        combined
    );
}

#[test]
fn test_skillopt_resume_missing_run_dir() {
    let result = run_fastskill_command(
        &[
            "optimize",
            "resume",
            "/tmp/nonexistent-skillopt-run-dir-xyz",
        ],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("OPTIMIZE_RUN_DIR_MISSING"),
        "Expected OPTIMIZE_RUN_DIR_MISSING in: {}",
        combined
    );
}

#[test]
fn test_skillopt_export_missing_best_skill() {
    let dir = TempDir::new().unwrap();
    // Run dir exists but has no best_skill.md
    let result = run_fastskill_command(
        &[
            "optimize",
            "export",
            dir.path().to_str().unwrap(),
            "--out",
            "/tmp/skillopt_export_out_test.md",
        ],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("OPTIMIZE_EXPORT_BEST_MISSING"),
        "Expected OPTIMIZE_EXPORT_BEST_MISSING in: {}",
        combined
    );
}

// ── inspect: real writer layout (steps/step_NNNN, gate/patch/rollouts/update.json) ──

/// Builds a run-dir fixture mirroring the real `aikit-skillopt` writer layout for
/// a single step 0, as preserved at
/// `.worktrees/_evidence/real-optimize-run/` (a real completed run captured
/// 2026-08-25). Content is copied verbatim from that run.
fn write_inspect_fixture_step0(run_dir: &std::path::Path) {
    let step_dir = run_dir.join("steps").join("step_0000");
    fs::create_dir_all(&step_dir).unwrap();
    fs::create_dir_all(run_dir.join("skills")).unwrap();

    fs::write(
        step_dir.join("gate.json"),
        r#"{"accepted": false, "best_score": 0.0, "score": 0.0}"#,
    )
    .unwrap();

    fs::write(
        step_dir.join("patch.json"),
        r#"[{"op": "replace", "target": "3. Produce one greeting line.", "content": "3. Produce one greeting line.\n\n## Output\nReturn ONLY the greeting line.", "impact": 0.9}]"#,
    )
    .unwrap();

    fs::write(
        step_dir.join("rollouts.json"),
        r#"[{"case_id": "tr-001", "score": 0.0}, {"case_id": "tr-002", "score": 0.0}]"#,
    )
    .unwrap();

    fs::write(
        step_dir.join("update.json"),
        r#"{"budget": 1, "chosen": [0], "skipped_count": 0}"#,
    )
    .unwrap();

    fs::write(
        run_dir.join("skills").join("skill_v0000.md"),
        "# Greeting Helper\n\n1. Read the audience.\n2. Pick a tone.\n3. Produce one greeting line.\n",
    )
    .unwrap();

    // history.json must agree with gate.json: step 0 was rejected, so no skill_v0001.md
    // was written. `--show diffs` resolves versions from history, so an incoherent
    // fixture would test a state the optimizer can never actually produce.
    fs::write(
        run_dir.join("history.json"),
        r#"[{"global_step": 0, "accepted": false, "epoch": 0, "score_current": 0.0, "score_candidate": 0.0}]"#,
    )
    .unwrap();
}

/// Rewrite the fixture's history so `step` is recorded as accepted.
fn mark_step_accepted(run_dir: &std::path::Path, records: &str) {
    fs::write(run_dir.join("history.json"), records).unwrap();
}

#[test]
fn test_skillopt_inspect_show_patches_reads_real_layout() {
    let dir = TempDir::new().unwrap();
    write_inspect_fixture_step0(dir.path());

    let result = run_fastskill_command(
        &[
            "optimize",
            "inspect",
            dir.path().to_str().unwrap(),
            "--step",
            "0",
            "--show",
            "patches",
        ],
        None,
    );
    assert!(
        result.success,
        "inspect --show patches failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("\"impact\"") && result.stdout.contains("0.9"),
        "expected patch.json content in output, got: {}",
        result.stdout
    );
}

#[test]
fn test_skillopt_inspect_show_gate_reads_real_layout() {
    let dir = TempDir::new().unwrap();
    write_inspect_fixture_step0(dir.path());

    let result = run_fastskill_command(
        &[
            "optimize",
            "inspect",
            dir.path().to_str().unwrap(),
            "--step",
            "0",
            "--show",
            "gate",
        ],
        None,
    );
    assert!(
        result.success,
        "inspect --show gate failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("\"accepted\": false") && result.stdout.contains("\"score\": 0.0"),
        "expected gate.json content in output, got: {}",
        result.stdout
    );
}

#[test]
fn test_skillopt_inspect_show_skips_reads_update_json() {
    let dir = TempDir::new().unwrap();
    write_inspect_fixture_step0(dir.path());

    let result = run_fastskill_command(
        &[
            "optimize",
            "inspect",
            dir.path().to_str().unwrap(),
            "--step",
            "0",
            "--show",
            "skips",
        ],
        None,
    );
    assert!(
        result.success,
        "inspect --show skips failed: {}{}",
        result.stdout, result.stderr
    );
    // Must be honest about the source: no dedicated skips.json artifact exists,
    // this is derived from update.json's skipped_count/chosen/budget.
    assert!(
        result.stdout.contains("update.json"),
        "expected output to disclose it is derived from update.json, got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("skipped_count") && result.stdout.contains('0'),
        "expected skipped_count value from update.json, got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("(no skips.json artifact)"),
        "must not silently claim a nonexistent skips.json was checked, got: {}",
        result.stdout
    );
}

#[test]
fn test_skillopt_inspect_show_diffs_missing_next_version_prints_message() {
    let dir = TempDir::new().unwrap();
    write_inspect_fixture_step0(dir.path());
    // No skills/skill_v0001.md — step 0 was rejected in this fixture, exactly
    // like the real preserved run. Must not error and must not fabricate a diff.

    let result = run_fastskill_command(
        &[
            "optimize",
            "inspect",
            dir.path().to_str().unwrap(),
            "--step",
            "0",
            "--show",
            "diffs",
        ],
        None,
    );
    assert!(
        result.success,
        "inspect --show diffs must not error when the next version is absent: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        !result.stdout.contains("@@"),
        "must not fabricate a diff hunk when skill_v0001.md is missing, got: {}",
        result.stdout
    );
    // history.json records step 0 as rejected, so the reason is known definitively —
    // no need to infer it from an absent file. The message must say so and point at
    // `--show gate`.
    assert!(
        result.stdout.contains("rejected") && result.stdout.contains("--show gate"),
        "expected a definitive rejected-step explanation, got: {}",
        result.stdout
    );
}

#[test]
fn test_skillopt_inspect_show_diffs_renders_diff_when_both_versions_exist() {
    let dir = TempDir::new().unwrap();
    write_inspect_fixture_step0(dir.path());
    // Step 0 accepted, so a v0001 exists.
    mark_step_accepted(
        dir.path(),
        r#"[{"global_step": 0, "accepted": true, "epoch": 0, "score_current": 0.0, "score_candidate": 1.0}]"#,
    );
    fs::write(
        dir.path().join("skills").join("skill_v0001.md"),
        "# Greeting Helper\n\n1. Read the audience.\n2. Pick a tone.\n3. Produce one greeting line.\n\n## Output\nReturn ONLY the greeting line.\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "optimize",
            "inspect",
            dir.path().to_str().unwrap(),
            "--step",
            "0",
            "--show",
            "diffs",
        ],
        None,
    );
    assert!(
        result.success,
        "inspect --show diffs failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("skill_v0000.md") && result.stdout.contains("skill_v0001.md"),
        "expected diff header naming the real version files, got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("+## Output"),
        "expected the added line from skill_v0001.md to appear in the diff, got: {}",
        result.stdout
    );
}

/// Regression: step index is NOT version index once an earlier step was rejected.
///
/// Step 0 rejected, step 1 accepted. Only one new version was ever written, so step 1's
/// real diff is `v0000 -> v0001`. The naive `step -> v{step}/v{step+1}` mapping would
/// resolve `v0001 -> v0002` and — if a v0002 happened to exist from a later accepted
/// step — silently render a different step's diff under step 1's name.
#[test]
fn test_skillopt_inspect_diffs_after_rejected_step_uses_accepted_version_count() {
    let dir = TempDir::new().unwrap();
    write_inspect_fixture_step0(dir.path());

    let step1 = dir.path().join("steps").join("step_0001");
    fs::create_dir_all(&step1).unwrap();
    fs::write(
        step1.join("gate.json"),
        r#"{"accepted": true, "best_score": 1.0, "score": 1.0}"#,
    )
    .unwrap();

    mark_step_accepted(
        dir.path(),
        r#"[{"global_step": 0, "accepted": false, "epoch": 0, "score_current": 0.0, "score_candidate": 0.0},
            {"global_step": 1, "accepted": true, "epoch": 0, "score_current": 0.0, "score_candidate": 1.0}]"#,
    );

    fs::write(
        dir.path().join("skills").join("skill_v0001.md"),
        "# Greeting Helper\n\n1. Read the audience.\n2. Pick a tone.\n3. Produce one greeting line.\n\n## Output\nReturn ONLY the greeting line.\n",
    )
    .unwrap();
    // A decoy: if the naive mapping were used it would diff v0001 -> v0002 and show this.
    fs::write(
        dir.path().join("skills").join("skill_v0002.md"),
        "# Greeting Helper\n\nDECOY VERSION THAT MUST NOT APPEAR FOR STEP 1\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "optimize",
            "inspect",
            dir.path().to_str().unwrap(),
            "--step",
            "1",
            "--show",
            "diffs",
        ],
        None,
    );
    assert!(
        result.success,
        "inspect --show diffs failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("skill_v0000.md") && result.stdout.contains("skill_v0001.md"),
        "step 1 must diff v0000 -> v0001 (one accepted step before it), got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("DECOY"),
        "step 1 must not render the v0001 -> v0002 diff, got: {}",
        result.stdout
    );
}

#[test]
fn test_skillopt_inspect_show_all_includes_rollouts() {
    let dir = TempDir::new().unwrap();
    write_inspect_fixture_step0(dir.path());

    let result = run_fastskill_command(
        &[
            "optimize",
            "inspect",
            dir.path().to_str().unwrap(),
            "--step",
            "0",
            "--show",
            "all",
        ],
        None,
    );
    assert!(
        result.success,
        "inspect --show all failed: {}{}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("tr-001") && result.stdout.contains("tr-002"),
        "expected per-case rollout scores in --show all output, got: {}",
        result.stdout
    );
}

#[test]
fn test_skillopt_export_byte_identical() {
    use sha2::{Digest, Sha256};

    let dir = TempDir::new().unwrap();
    let best_skill_content = b"# Best Skill\n\nThis is the optimized skill document.\n";

    // Write best_skill.md to the synthetic run dir
    fs::write(dir.path().join("best_skill.md"), best_skill_content).unwrap();

    let out_path = dir.path().join("exported_skill.md");
    let result = run_fastskill_command(
        &[
            "optimize",
            "export",
            dir.path().to_str().unwrap(),
            "--out",
            out_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        result.success,
        "export failed: {}{}",
        result.stdout, result.stderr
    );

    // Verify byte-identical via SHA-256
    let exported = fs::read(&out_path).unwrap();
    let source_hash = Sha256::digest(best_skill_content);
    let exported_hash = Sha256::digest(&exported);
    assert_eq!(
        source_hash, exported_hash,
        "exported file SHA-256 does not match source"
    );
}

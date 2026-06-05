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
        &["optimize", "run", "--config", "/tmp/nonexistent-skillopt-config-xyz.toml"],
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
        &[
            "optimize",
            "run",
            "--config",
            config_path.to_str().unwrap(),
        ],
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
        &[
            "optimize",
            "run",
            "--config",
            config_path.to_str().unwrap(),
        ],
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
        &[
            "optimize",
            "run",
            "--config",
            config_path.to_str().unwrap(),
        ],
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

#[test]
fn test_skillopt_resume_missing_run_dir() {
    let result = run_fastskill_command(
        &["optimize", "resume", "/tmp/nonexistent-skillopt-run-dir-xyz"],
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
    assert!(result.success, "export failed: {}{}", result.stdout, result.stderr);

    // Verify byte-identical via SHA-256
    let exported = fs::read(&out_path).unwrap();
    let source_hash = Sha256::digest(best_skill_content);
    let exported_hash = Sha256::digest(&exported);
    assert_eq!(
        source_hash, exported_hash,
        "exported file SHA-256 does not match source"
    );
}

//! CLI integration tests for eval commands

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
    run_fastskill_command_with_env,
};

#[test]
fn test_eval_help() {
    let result = run_fastskill_command(&["eval", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings("eval_help", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_eval_validate_help() {
    let result = run_fastskill_command(&["eval", "validate", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings(
        "eval_validate_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
}

#[test]
fn test_eval_run_help() {
    let result = run_fastskill_command(&["eval", "run", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings("eval_run_help", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_eval_report_help() {
    let result = run_fastskill_command(&["eval", "report", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings("eval_report_help", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_eval_score_help() {
    let result = run_fastskill_command(&["eval", "score", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings("eval_score_help", &result.stdout, &cli_snapshot_settings());
}

#[test]
fn test_eval_run_requires_agent() {
    let result = run_fastskill_command(&["eval", "run", "--output-dir", "/tmp/evals"], None);
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("--agent") || combined.contains("agent"),
        "Expected error about missing --agent, got: {}",
        combined
    );
}

#[test]
fn test_eval_run_requires_output_dir() {
    let result = run_fastskill_command(&["eval", "run", "--agent", "codex"], None);
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("--output-dir")
            || combined.contains("output-dir")
            || combined.contains("output_dir"),
        "Expected error about missing --output-dir, got: {}",
        combined
    );
}

#[test]
fn test_eval_run_rejects_unsupported_agent() {
    let result = run_fastskill_command(
        &[
            "eval",
            "run",
            "--agent",
            "unsupported-agent-xyz",
            "--output-dir",
            "/tmp/evals",
        ],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("unsupported-agent-xyz") || combined.contains("not a supported agent"),
        "Expected error about unsupported agent, got: {}",
        combined
    );
}

#[test]
fn test_eval_validate_no_project_file() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let result = run_fastskill_command(&["eval", "validate"], Some(dir.path()));
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("skill-project.toml") || combined.contains("EVAL_CONFIG_MISSING"),
        "Expected error about missing skill-project.toml, got: {}",
        combined
    );
}

#[test]
fn test_eval_validate_no_eval_config() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["eval", "validate"], Some(dir.path()));
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_CONFIG_MISSING") || combined.contains("eval"),
        "Expected EVAL_CONFIG_MISSING error, got: {}",
        combined
    );
}

#[test]
fn test_eval_validate_with_eval_config() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    // Create evals directory and prompts.csv
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntest-1,\"Test prompt\",true,\"basic\",\n",
    )
    .unwrap();

    // Create SKILL.md so it's detected as skill context
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();

    // Create skill-project.toml with eval config
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["eval", "validate"], Some(dir.path()));
    assert!(
        result.success,
        "Expected eval validate to succeed, got stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("valid") || result.stdout.contains("prompts"),
        "Expected valid output, got: {}",
        result.stdout
    );
}

#[test]
fn test_eval_validate_invalid_csv_missing_column() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    // CSV missing required 'should_trigger' column
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt\ntest-1,\"Test prompt\"\n",
    )
    .unwrap();

    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["eval", "validate"], Some(dir.path()));
    assert!(
        !result.success,
        "Expected eval validate to fail due to missing CSV column"
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_INVALID_CSV") || combined.contains("should_trigger"),
        "Expected EVAL_INVALID_CSV error, got: {}",
        combined
    );
}

#[test]
fn test_eval_validate_invalid_checks_toml() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntest-1,\"Test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    // Invalid TOML syntax
    fs::write(
        evals_dir.join("checks.toml"),
        "[[check]\nname = broken toml {\n",
    )
    .unwrap();

    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\nchecks = \"evals/checks.toml\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["eval", "validate"], Some(dir.path()));
    assert!(
        !result.success,
        "Expected eval validate to fail due to invalid checks TOML"
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_CHECKS_INVALID")
            || combined.contains("TOML")
            || combined.contains("toml"),
        "Expected EVAL_CHECKS_INVALID error, got: {}",
        combined
    );
}

#[test]
fn test_eval_validate_with_counts_in_json_output() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntest-1,\"Test prompt\",true,\"basic\",\ntest-2,\"Another prompt\",false,\"\",\n",
    )
    .unwrap();
    fs::write(
        evals_dir.join("checks.toml"),
        "[[check]]\nname = \"trigger_expectation\"\npattern = \"fastskill\"\nexpected = true\n",
    )
    .unwrap();

    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\nchecks = \"evals/checks.toml\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["eval", "validate", "--json"], Some(dir.path()));
    assert!(
        result.success,
        "Expected eval validate to succeed, got stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );

    let json_start = result.stdout.find('{').unwrap();
    let output: serde_json::Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    assert_eq!(output["valid"], true);
    assert_eq!(output["case_count"], 2);
    assert_eq!(output["check_count"], 1);
}

#[test]
fn test_eval_report_requires_run_dir() {
    let result = run_fastskill_command(&["eval", "report"], None);
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("--run-dir") || combined.contains("run-dir"),
        "Expected error about missing --run-dir, got: {}",
        combined
    );
}

#[test]
fn test_eval_score_requires_run_dir() {
    let result = run_fastskill_command(&["eval", "score"], None);
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("--run-dir") || combined.contains("run-dir"),
        "Expected error about missing --run-dir, got: {}",
        combined
    );
}

#[test]
fn test_eval_report_nonexistent_run_dir() {
    let result = run_fastskill_command(
        &[
            "eval",
            "report",
            "--run-dir",
            "/tmp/nonexistent-fastskill-eval-dir-xyz123",
        ],
        None,
    );
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_ARTIFACTS_CORRUPT") || combined.contains("not exist"),
        "Expected error about nonexistent dir, got: {}",
        combined
    );
}

#[test]
fn test_eval_run_persists_event_trace_jsonl() {
    use serde_json::Value;
    use std::env;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntrace-case,\"test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    // Create a fake `agent` executable so aikit_sdk::is_agent_available("agent") succeeds.
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let agent_path = bin_dir.join("agent");
    fs::write(
        &agent_path,
        "#!/usr/bin/env bash\nif [[ \"$1\" == \"--version\" ]]; then echo \"agent 0.1\"; exit 0; fi\necho '{\"event\":\"ok\"}'\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&agent_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&agent_path, perms).unwrap();
    }

    let output_dir = dir.path().join("out");
    let path = env::var("PATH").unwrap_or_default();
    let merged_path = format!("{}:{}", bin_dir.display(), path);
    let env_vars = vec![("PATH", merged_path.as_str())];

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "agent",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--case",
            "trace-case",
            "--json",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        result.success,
        "Expected eval run to succeed, got stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );

    let json_start = result.stdout.find('{').unwrap();
    let summary: Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    let run_dir = summary["run_dir"].as_str().unwrap();
    let trace_path = std::path::Path::new(run_dir)
        .join("trace-case")
        .join("trace.jsonl");
    let trace_jsonl = fs::read_to_string(&trace_path).unwrap();

    assert!(
        trace_jsonl.contains("\"type\":\"raw_json\""),
        "expected persisted trace.jsonl to contain raw_json event, got: {}",
        trace_jsonl
    );

    let result_path = std::path::Path::new(run_dir)
        .join("trace-case")
        .join("result.json");
    let case_result: Value =
        serde_json::from_str(&fs::read_to_string(result_path).unwrap()).unwrap();
    assert_eq!(case_result["command_count"], 1);
}

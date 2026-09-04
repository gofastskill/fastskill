//! CLI integration tests for eval commands

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::{
    assert_snapshot_with_settings, cli_snapshot_settings, run_fastskill_command,
    run_fastskill_command_with_env,
};

/// Path to the compiled fake-agent test helper (see
/// `crates/fastskill-cli/src/bin/fake_agent.rs`), auto-discovered by Cargo
/// as a `[[bin]]` target of this same package via the `CARGO_BIN_EXE_<name>`
/// env var Cargo sets for integration tests.
///
/// Using a real compiled binary here -- instead of a bash script, as earlier
/// versions of these fixtures did -- means the exact same fixture works
/// unmodified on Windows and Unix: no shebang for a non-existent
/// interpreter, no Windows PATHEXT gap (a bash script has no `.exe`/`.cmd`
/// extension, so Windows can neither find nor execute it), and no
/// Unix-only `:` PATH-separator assumption.
fn fake_agent_binary() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_BIN_EXE_fake_agent"))
}

/// Installs the compiled fake-agent helper into `bin_dir` under
/// `logical_name` (e.g. `"agent"` or `"codex"`), applying the platform's
/// executable naming convention (a `.exe` suffix on Windows, so
/// aikit-sdk's PATH+PATHEXT probing in `command_resolve.rs` finds it) and
/// returns a PATH value with `bin_dir` prepended using the OS-correct
/// path-list separator (`std::env::join_paths`), ready to hand to
/// `run_fastskill_command_with_env`.
fn install_fake_agent(bin_dir: &std::path::Path, logical_name: &str) -> String {
    std::fs::create_dir_all(bin_dir).unwrap();
    let file_name = if cfg!(windows) {
        format!("{logical_name}.exe")
    } else {
        logical_name.to_string()
    };
    std::fs::copy(fake_agent_binary(), bin_dir.join(&file_name)).unwrap();

    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let mut entries = vec![bin_dir.to_path_buf()];
    entries.extend(std::env::split_paths(&existing_path));
    std::env::join_paths(entries)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

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
fn test_eval_judge_help() {
    let result = run_fastskill_command(&["eval", "judge", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings("eval_judge_help", &result.stdout, &cli_snapshot_settings());
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
fn test_eval_scorecard_help() {
    let result = run_fastskill_command(&["eval", "scorecard", "--help"], None);
    assert!(result.success);
    assert_snapshot_with_settings(
        "eval_scorecard_help",
        &result.stdout,
        &cli_snapshot_settings(),
    );
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

/// `fastskill eval run` must persist per-trial artifacts (trace.jsonl,
/// result.json, stdout.txt) and report a well-formed `command_count`.
///
/// This originally guarded an upstream aikit bug (goaikit/aikit#145) where an
/// agent's text output was miscounted as commands. That bug was fixed in aikit
/// 0.1.192 (goaikit/aikit#148); the fix is verified end-to-end against a real
/// agent (a zero-tool prompt reports `command_count: 0`, with the agent's text
/// typed as a `message` event rather than a counted `raw_json`).
///
/// A synthetic PATH-binary fake can't reproduce a real agent's recognized event
/// stream, so this test asserts what a fake *can* prove deterministically: the
/// per-trial artifacts are persisted at the expected paths, the agent's raw
/// output is captured, and non-command output yields `command_count == 0` — it
/// is not re-inflated the way the pre-#148 pipeline was.
#[test]
fn test_eval_run_persists_event_trace_jsonl() {
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    // The fake agent never consults the skill, so this case declares that.
    // `should_trigger` is scored (R7): claiming `true` here would fail every
    // trial on a check that has nothing to do with what this test measures.
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntrace-case,\"test prompt\",false,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    // Cross-platform fake `codex` (a supported agent key): install_fake_agent
    // copies the compiled fake_agent binary — which emits a generic
    // `{"event":"ok"}` line — and returns PATH with its dir prepended, so
    // aikit_sdk::is_agent_available("codex") finds it (shadowing any real codex)
    // on both Unix and Windows.
    let bin_dir = dir.path().join("bin");
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let output_dir = dir.path().join("out");
    let env_vars = vec![("PATH", merged_path.as_str())];

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
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
        .join("trial-1")
        .join("trace.jsonl");
    let trace_jsonl = fs::read_to_string(&trace_path).unwrap();

    // The trace artifact is persisted (created for the trial), and the agent's
    // raw output was captured to stdout.txt — the persistence layer works.
    assert!(
        trace_path.exists(),
        "expected per-trial trace.jsonl to be persisted at {}",
        trace_path.display()
    );
    let stdout_txt = fs::read_to_string(
        std::path::Path::new(run_dir)
            .join("trace-case")
            .join("trial-1")
            .join("stdout.txt"),
    )
    .unwrap();
    assert!(
        stdout_txt.contains("{\"event\":\"ok\"}"),
        "expected the fake agent's raw output to be captured; got: {stdout_txt}"
    );
    let _ = trace_jsonl; // read above to prove the file is readable

    // command_count is a well-formed integer and is 0 for this non-command
    // output — the #145 regression: pre-#148, unrecognized events were
    // miscounted and would have inflated this.
    let result_path = std::path::Path::new(run_dir)
        .join("trace-case")
        .join("trial-1")
        .join("result.json");
    let case_result: Value =
        serde_json::from_str(&fs::read_to_string(result_path).unwrap()).unwrap();
    assert_eq!(case_result["command_count"], 0);
}

#[test]
fn test_eval_run_trials_threshold_and_ci_exit_semantics() {
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntrial-case,\"test prompt\",false,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        evals_dir.join("checks.toml"),
        "[[check]]\nname = \"command_contains\"\npattern = \"fake-agent-ok\"\nrequired = true\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\nchecks = \"evals/checks.toml\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    // Fake agent (see `install_fake_agent`) that answers correctly on the
    // first 3 invocations and wrongly after -- `FAKE_AGENT_MODE=counter` with
    // `FAKE_AGENT_FAIL_KIND=answer`. The failing trials still exit 0 and still
    // complete their turn: only the answer is wrong, which is what the check
    // above reads. That distinction is the whole point of the fail kind. An
    // exit-1 trial is an `error`, and errors are excluded from the rate, so a
    // fixture that crashed instead of answering badly could never move a
    // threshold at all -- see the error-accounting test below.
    let bin_dir = dir.path().join("bin");
    let output_dir = dir.path().join("out");
    let state_dir = dir.path().join("state");
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let env_vars = vec![
        ("PATH", merged_path.as_str()),
        ("FASTSKILL_TEST_STATE_DIR", state_dir.to_str().unwrap()),
        ("FAKE_AGENT_MODE", "counter"),
        ("FAKE_AGENT_PASS_LIMIT", "3"),
        ("FAKE_AGENT_FAIL_KIND", "answer"),
    ];

    // Threshold 0.6 should pass for 3/5.
    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--case",
            "trial-case",
            "--trials",
            "5",
            "--threshold",
            "0.6",
            "--ci",
            "--json",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        result.success,
        "Expected eval run to succeed in CI mode at threshold=0.6, got stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let json_start = result.stdout.find('{').unwrap();
    let summary: Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    assert_eq!(summary["cases"][0]["id"], "trial-case");
    assert_eq!(summary["cases"][0]["status"], "passed");
    assert_eq!(summary["cases"][0]["pass_count"], 3);
    assert_eq!(summary["cases"][0]["total_trials"], 5);
    // Nothing crashed, so every trial is a measurement and the rate is 3/5.
    assert_eq!(summary["cases"][0]["error_count"], 0);
    assert_eq!(summary["cases"][0]["scored_trials"], 5);

    // Reset state and require 100% suite pass rate should fail in --ci mode.
    fs::remove_file(state_dir.join("count")).ok();
    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--case",
            "trial-case",
            "--trials",
            "5",
            "--threshold",
            "1.0",
            "--ci",
            "--json",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        !result.success,
        "Expected eval run to fail in CI mode at threshold=1.0"
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("threshold") || combined.contains("Eval suite failed"),
        "Expected threshold-related failure, got: {}",
        combined
    );
}

/// R1: a trial that produced no measurement is not evidence about the skill,
/// so it is excluded from the pass rate and reported on its own. The fixture
/// here is the same counter, failing the other way: past the limit it exits 1
/// instead of answering badly. Five trials, three of them clean, and the rate
/// the threshold sees must be 3/3 rather than 3/5 -- with the two crashes
/// still visible in the summary rather than quietly dropped.
#[test]
fn test_eval_run_excludes_error_trials_from_the_rate_and_counts_them() {
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nerror-case,\"test prompt\",false,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    let bin_dir = dir.path().join("bin");
    let output_dir = dir.path().join("out");
    let state_dir = dir.path().join("state");
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let env_vars = vec![
        ("PATH", merged_path.as_str()),
        ("FASTSKILL_TEST_STATE_DIR", state_dir.to_str().unwrap()),
        ("FAKE_AGENT_MODE", "counter"),
        ("FAKE_AGENT_PASS_LIMIT", "3"),
        // default fail kind: exit 1, i.e. no measurement at all
    ];

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--case",
            "error-case",
            "--trials",
            "5",
            "--threshold",
            "1.0",
            "--ci",
            "--json",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        result.success,
        "3 clean trials out of 3 measurements is a rate of 1.0, so a \
         threshold of 1.0 is met; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let json_start = result.stdout.find('{').unwrap();
    let summary: Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    let case = &summary["cases"][0];
    assert_eq!(case["id"], "error-case");
    assert_eq!(case["status"], "passed");
    assert_eq!(case["pass_count"], 3);
    assert_eq!(case["total_trials"], 5);
    assert_eq!(
        case["error_count"], 2,
        "the two crashed trials must still be reported: {}",
        result.stdout
    );
    assert_eq!(
        case["scored_trials"], 3,
        "only the measurements may be scored: {}",
        result.stdout
    );
}

/// Was: a raw wall-clock comparison (`elapsed < 1.6s` for 4 trials that each
/// `sleep 0.5`, vs. ~2s if serialized). That's inherently flaky on loaded or
/// slow CI runners — process spawn overhead, scheduler contention, or a busy
/// shared box can all push a genuinely-parallel run over an arbitrary fixed
/// threshold with zero relationship to whether parallelism actually happened.
///
/// Trace-based evidence was considered and rejected: `aikit-evals`'s trace
/// pipeline has no per-event wall-clock timestamps (`TraceEvent` only carries a
/// `seq` ordinal — see aikit-evals/src/trace.rs), and separately, JSON stdout
/// lines from the fake `agent` script never survive into trace.jsonl at all
/// for the "agent" runtime key (see the REAL BUG documented on
/// `test_eval_run_persists_event_trace_jsonl` above) — so trace evidence
/// cannot be used as a parallelism signal here without depending on that
/// already-broken pipeline.
///
/// Instead, this asserts the actual observable structural property directly:
/// each trial's fake `agent` invocation records its own start/end wall-clock
/// window (independent of overall command duration) to a shared interval log,
/// serialized with `flock` the same way `test_eval_run_trials_threshold_and_ci_exit_semantics`
/// above already serializes its counter file. If trials ran with real
/// concurrency, at least two of the four 0.5s windows must overlap in time —
/// true regardless of how fast or slow trial dispatch/spawn is on the host,
/// since it only compares trial windows to each other, not to a fixed budget.
/// If trials ran strictly sequentially (parallelism silently broken), no two
/// windows can ever overlap, so the assertion would correctly fail.
#[test]
fn test_eval_run_parallelism_produces_overlapping_trial_windows() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nsleep-case,\"test prompt\",false,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nparallel = 4\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    // Fake agent (see `install_fake_agent`) that, per invocation, sleeps
    // 500ms and then records its own "<start_ns> <end_ns>" wall-clock
    // window to a shared, lock-guarded intervals file --
    // `FAKE_AGENT_MODE=interval`.
    let bin_dir = dir.path().join("bin");
    let state_dir = dir.path().join("state");
    fs::create_dir_all(&state_dir).unwrap();
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let output_dir = dir.path().join("out");
    let env_vars = vec![
        ("PATH", merged_path.as_str()),
        ("FASTSKILL_TEST_STATE_DIR", state_dir.to_str().unwrap()),
        ("FAKE_AGENT_MODE", "interval"),
    ];

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--case",
            "sleep-case",
            "--trials",
            "4",
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

    let intervals_content = fs::read_to_string(state_dir.join("intervals.txt"))
        .expect("expected the fake agent to have recorded trial start/end intervals");
    let intervals: Vec<(u128, u128)> = intervals_content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let start: u128 = parts.next().unwrap().parse().unwrap();
            let end: u128 = parts.next().unwrap().parse().unwrap();
            (start, end)
        })
        .collect();
    assert_eq!(
        intervals.len(),
        4,
        "expected 4 recorded trial windows, got: {:?}",
        intervals
    );

    // Two windows [s1,e1) and [s2,e2) overlap iff s1 < e2 && s2 < e1.
    let overlaps = |a: (u128, u128), b: (u128, u128)| a.0 < b.1 && b.0 < a.1;
    let has_overlap = intervals
        .iter()
        .enumerate()
        .any(|(i, &a)| intervals.iter().skip(i + 1).any(|&b| overlaps(a, b)));

    assert!(
        has_overlap,
        "Expected at least two of the 4 trial windows to overlap in time, proving \
         they ran concurrently under parallel=4; got non-overlapping (i.e. serialized) \
         windows: {:?}",
        intervals
    );
}

// ─── RFC-055: new runtime-selection primitive tests ─────────────────────────

#[test]
fn test_eval_run_conflicting_flags() {
    let result = run_fastskill_command(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--all",
            "--output-dir",
            "/tmp/evals",
        ],
        None,
    );
    assert!(!result.success, "eval run --agent codex --all must fail");
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("RUNTIME_CONFLICTING_FLAGS"),
        "Expected RUNTIME_CONFLICTING_FLAGS, got: {}",
        combined
    );
}

#[test]
fn test_eval_run_unknown_runtime_id() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let result = run_fastskill_command(
        &[
            "eval",
            "run",
            "--agent",
            "invalid-runtime-xyz",
            "--output-dir",
            dir.path().to_str().unwrap(),
        ],
        None,
    );
    assert!(!result.success, "eval run with unknown runtime must fail");
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("RUNTIME_UNKNOWN_ID"),
        "Expected RUNTIME_UNKNOWN_ID, got: {}",
        combined
    );
    assert!(
        combined.contains("invalid-runtime-xyz"),
        "Error must name the unknown runtime, got: {}",
        combined
    );
}

#[test]
fn test_eval_run_no_selection_error() {
    let result = run_fastskill_command(&["eval", "run", "--output-dir", "/tmp/evals"], None);
    assert!(
        !result.success,
        "eval run without --agent or --all must fail"
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("RUNTIME_NO_SELECTION")
            || combined.contains("--agent")
            || combined.contains("agent"),
        "Expected RUNTIME_NO_SELECTION or mention of --agent, got: {}",
        combined
    );
}

#[test]
fn test_eval_run_all_flag() {
    use std::env;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nall-case,\"test prompt\",false,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    // Create a fake "agent" binary so at least one runtime is available.
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let agent_path = bin_dir.join("agent");
    fs::write(
        &agent_path,
        "#!/usr/bin/env bash\nif [[ \"${1:-}\" == \"--version\" ]]; then echo \"agent 0.1\"; exit 0; fi\necho '{\"event\":\"ok\"}'\nexit 0\n",
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

    // Use --no-fail because most runtimes won't be available in CI.
    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--all",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--no-fail",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        result.success,
        "eval run --all with --no-fail must succeed; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
}

#[test]
fn test_eval_validate_conflicting_flags() {
    use std::fs;
    use tempfile::TempDir;

    // `eval validate` checks for skill-project.toml (EVAL_CONFIG_MISSING)
    // before it ever reaches the --agent/--all conflict check (see
    // `execute_validate` in fastskill-cli/src/commands/eval/validate.rs),
    // unlike `eval run`, which validates runtime flags first. Running this
    // with `None` (no project file in scope) hits EVAL_CONFIG_MISSING
    // instead of the conflict this test wants to exercise, so it needs the
    // same minimal eval project fixture the other `eval validate` tests use.
    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntest-1,\"Test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &["eval", "validate", "--agent", "codex", "--all"],
        Some(dir.path()),
    );
    assert!(
        !result.success,
        "eval validate --agent codex --all must fail"
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("RUNTIME_CONFLICTING_FLAGS"),
        "Expected RUNTIME_CONFLICTING_FLAGS, got: {}",
        combined
    );
}

#[test]
fn test_eval_validate_unknown_runtime_id() {
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
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &["eval", "validate", "--agent", "invalid-runtime-xyz"],
        Some(dir.path()),
    );
    assert!(
        !result.success,
        "eval validate with unknown runtime must fail"
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("RUNTIME_UNKNOWN_ID"),
        "Expected RUNTIME_UNKNOWN_ID, got: {}",
        combined
    );
}

#[test]
fn test_eval_validate_all_flag() {
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
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let result = run_fastskill_command(&["eval", "validate", "--all"], Some(dir.path()));
    assert!(
        result.success,
        "eval validate --all must succeed; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("valid") || result.stdout.contains("prompts"),
        "Expected valid output, got: {}",
        result.stdout
    );
}

#[test]
fn test_eval_run_json_output_contains_agent_field() {
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nagent-json-case,\"test prompt\",false,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    let bin_dir = dir.path().join("bin");
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let output_dir = dir.path().join("out");
    let env_vars = vec![("PATH", merged_path.as_str())];

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--json",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        result.success,
        "Expected eval run --json to succeed; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );

    let json_start = result.stdout.find('{').unwrap();
    let summary: Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    assert!(
        summary.get("agent").is_some(),
        "JSON summary must contain 'agent' field; got: {}",
        summary
    );
    assert_eq!(
        summary["agent"].as_str().unwrap(),
        "codex",
        "JSON 'agent' field must match the requested agent"
    );
}

/// R7 red-green, run side. The fixtures above flip `should_trigger` to `false`
/// so the fake agent's behaviour matches the column; this is the other half,
/// and it is what proves those fixtures are passing on a real check rather than
/// on the column being ignored. Same agent, same fake, `true` instead of
/// `false`: the run must now fail, on the check the column generates.
#[test]
fn test_eval_run_scores_the_should_trigger_column() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntrigger-case,\"test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    let bin_dir = dir.path().join("bin");
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let output_dir = dir.path().join("out");
    let env_vars = vec![("PATH", merged_path.as_str())];

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--json",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        !result.success,
        "an agent that never consults the skill must fail a should_trigger=true \
         case; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("skill_invoked"),
        "the failure must name the check the column generates, got: {combined}"
    );

    // A crashed trial would also make the run non-zero, and would prove
    // nothing about the column. Pin the shape: the agent completed, the trial
    // was scored, and it is the check that came back false.
    let json_start = result.stdout.find('{').unwrap();
    let summary: serde_json::Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    let case = &summary["cases"][0];
    assert_eq!(case["status"], "failed", "got: {}", result.stdout);
    assert_eq!(case["error_count"], 0, "got: {}", result.stdout);
    assert_eq!(case["scored_trials"], 1, "got: {}", result.stdout);
}

/// R10 on the `--agent` path: a backend whose decoder emits no tool frames has
/// no evidence for the check `should_trigger` generates, so the run is refused
/// before it costs anything rather than reported as a failing suite afterwards.
#[test]
fn test_eval_run_refuses_a_backend_that_cannot_score_the_suite() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nblind-case,\"test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let bin_dir = dir.path().join("bin");
    let merged_path = install_fake_agent(&bin_dir, "gemini");
    let output_dir = dir.path().join("out");
    let env_vars = vec![("PATH", merged_path.as_str())];

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "gemini",
            "--output-dir",
            output_dir.to_str().unwrap(),
            // --no-fail suppresses "the skill scored badly"; it must not
            // suppress "this backend cannot produce a score at all".
            "--no-fail",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        !result.success,
        "a suite with no score on this backend must not exit zero; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_CHECKS_UNOBSERVABLE"),
        "got: {combined}"
    );
    assert!(
        !output_dir.exists(),
        "the refusal must land before any trial runs, so no run directory is \
         allocated; found one at {}",
        output_dir.display()
    );
}

#[test]
fn test_eval_validate_agent_flag() {
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
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    // Provide a fake `codex` executable rather than depending on a real one
    // being installed: a developer machine may have the agent CLIs on PATH
    // while a CI runner does not, which would make this test pass locally and
    // fail in CI. Same approach as the fake `agent` binary used above.
    let bin_dir = dir.path().join("bin");
    let merged_path = install_fake_agent(&bin_dir, "codex");

    let result = run_fastskill_command_with_env(
        &["eval", "validate", "--agent", "codex"],
        &[("PATH", merged_path.as_str())],
        Some(dir.path()),
    );
    assert!(
        result.success,
        "eval validate --agent codex must succeed; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("valid") || result.stdout.contains("prompts"),
        "Expected valid output, got: {}",
        result.stdout
    );
}

/// A suite whose CSV parses to zero cases (e.g. a header-only file left behind
/// by a truncated write or a wrong `prompts` path pointing at a template) must
/// be rejected loudly. Pre-guard, `eval run` reported `0/0 passed` with the
/// default `failed == 0` verdict — i.e. PASSED, exit 0 — green-lighting CI
/// while running nothing at all.
#[test]
fn test_eval_run_empty_suite_errors() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    // Header only — zero cases.
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    let output_dir = dir.path().join("out");
    // `--agent aikit`: the in-process runtime is the only one guaranteed to
    // exist everywhere (CI has no external agent CLIs installed, and runtime
    // validation runs before suite loading — an uninstalled agent would fail
    // with RUNTIME_UNKNOWN_ID before the guard under test is ever reached).
    let result = run_fastskill_command(
        &[
            "eval",
            "run",
            "--agent",
            "aikit",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ],
        Some(dir.path()),
    );
    assert!(
        !result.success,
        "eval run with an empty suite must fail, not report 0/0 PASSED; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_EMPTY_SUITE"),
        "Expected EVAL_EMPTY_SUITE, got: {}",
        combined
    );
}

/// Same guard for `eval score`: a summary.json with zero cases (as pre-guard
/// `eval run` could produce from an empty suite) must not re-score as
/// `0/0 passed · PASSED` with exit 0.
#[test]
fn test_eval_score_empty_summary_errors() {
    use fastskill_evals::artifacts::{write_summary, SummaryResult};
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = dir.path().join("run");
    fs::create_dir_all(&run_dir).unwrap();
    let checks_path = dir.path().join("checks.toml");
    fs::write(
        &checks_path,
        "[[check]]\nname = \"trigger_expectation\"\npattern = \"fastskill\"\nexpected = true\n",
    )
    .unwrap();

    let summary = SummaryResult {
        suite_pass: true,
        suite_pass_rate: Some(0.0),
        agent: "codex".to_string(),
        model: None,
        total_cases: 0,
        passed: 0,
        failed: 0,
        trials_per_case: Some(1),
        parallel: None,
        pass_threshold: Some(1.0),
        run_dir: run_dir.clone(),
        checks_path: Some(checks_path),
        skill_project_root: dir.path().to_path_buf(),
        isolation: None,
        judge_errors: None,
        judge_skipped_trials: None,
        judge_tokens: None,
        judge_cost_usd: None,
        skill_git_sha: None,
        skill_dirty: None,
        cases: vec![],
    };
    write_summary(&run_dir, &summary).unwrap();

    let result = run_fastskill_command(
        &["eval", "score", "--run-dir", run_dir.to_str().unwrap()],
        None,
    );
    assert!(
        !result.success,
        "eval score of a zero-case summary must fail, not report 0/0 PASSED; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_EMPTY_SUITE"),
        "Expected EVAL_EMPTY_SUITE, got: {}",
        combined
    );
}

/// Same guard for `eval report`: a summary.json with zero cases must not
/// render as `result: PASSED · cases: 0/0 passed` — the vacuous verdict the
/// run/score guards exist to prevent, surviving via the artifact viewer.
#[test]
fn test_eval_report_empty_summary_errors() {
    use fastskill_evals::artifacts::{write_summary, SummaryResult};
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = dir.path().join("run");
    fs::create_dir_all(&run_dir).unwrap();

    let summary = SummaryResult {
        suite_pass: true,
        suite_pass_rate: Some(0.0),
        agent: "codex".to_string(),
        model: None,
        total_cases: 0,
        passed: 0,
        failed: 0,
        trials_per_case: Some(1),
        parallel: None,
        pass_threshold: Some(1.0),
        run_dir: run_dir.clone(),
        checks_path: None,
        skill_project_root: dir.path().to_path_buf(),
        isolation: None,
        judge_errors: None,
        judge_skipped_trials: None,
        judge_tokens: None,
        judge_cost_usd: None,
        skill_git_sha: None,
        skill_dirty: None,
        cases: vec![],
    };
    write_summary(&run_dir, &summary).unwrap();

    let result = run_fastskill_command(
        &["eval", "report", "--run-dir", run_dir.to_str().unwrap()],
        None,
    );
    assert!(
        !result.success,
        "eval report of a zero-case summary must fail, not render 0/0 PASSED; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_EMPTY_SUITE"),
        "Expected EVAL_EMPTY_SUITE, got: {}",
        combined
    );
}

#[test]
fn test_eval_report_displays_token_info_when_present() {
    use fastskill_evals::artifacts::{write_summary, CaseStatus, CaseSummary, SummaryResult};
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = dir.path().join("run");
    fs::create_dir_all(&run_dir).unwrap();

    let summary = SummaryResult {
        suite_pass: true,
        suite_pass_rate: Some(1.0),
        agent: "agent".to_string(),
        model: None,
        total_cases: 1,
        passed: 1,
        failed: 0,
        trials_per_case: Some(1),
        parallel: None,
        pass_threshold: Some(1.0),
        run_dir: run_dir.clone(),
        checks_path: None,
        skill_project_root: dir.path().to_path_buf(),
        isolation: None,
        judge_errors: None,
        judge_skipped_trials: None,
        judge_tokens: None,
        judge_cost_usd: None,
        skill_git_sha: None,
        skill_dirty: None,
        cases: vec![CaseSummary {
            id: "token-case".to_string(),
            status: CaseStatus::Passed,
            command_count: Some(1),
            input_tokens: Some(1234),
            output_tokens: Some(567),
            pass_count: Some(1),
            total_trials: Some(1),
            pass_rate: Some(1.0),
            error_count: Some(0),
            scored_trials: Some(1),
            should_trigger: None,
            judge_excluded_count: None,
            scores: Default::default(),
            trials: vec![],
        }],
    };
    write_summary(&run_dir, &summary).unwrap();

    let result = run_fastskill_command(
        &["eval", "report", "--run-dir", run_dir.to_str().unwrap()],
        None,
    );
    assert!(
        result.success,
        "eval report must succeed; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("in=1234") && result.stdout.contains("out=567"),
        "eval report must display token totals when present; got: {}",
        result.stdout
    );
}

/// Shared scaffolding for the `eval score` artifact tests: one case, one
/// trial, a trace the caller supplies, and a `summary.json` that records the
/// case's `should_trigger` column exactly as `eval run` now writes it.
#[allow(clippy::too_many_arguments)]
fn write_scoreable_run(
    root: &std::path::Path,
    case_id: &str,
    checks_toml: &str,
    trace_jsonl: &str,
    recorded: fastskill_evals::artifacts::TrialResult,
    should_trigger: Option<bool>,
) -> std::path::PathBuf {
    use fastskill_evals::artifacts::{write_summary, CaseStatus, CaseSummary, SummaryResult};
    use std::fs;

    let run_dir = root.join("run");
    let trial_dir = run_dir.join(case_id).join("trial-1");
    fs::create_dir_all(&trial_dir).unwrap();

    let checks_path = root.join("checks.toml");
    fs::write(&checks_path, checks_toml).unwrap();
    fs::write(trial_dir.join("trace.jsonl"), trace_jsonl).unwrap();
    fs::write(trial_dir.join("stdout.txt"), "").unwrap();
    fs::write(
        trial_dir.join("result.json"),
        serde_json::to_string_pretty(&recorded).unwrap(),
    )
    .unwrap();

    let summary = SummaryResult {
        suite_pass: false,
        suite_pass_rate: Some(0.0),
        agent: "claude".to_string(),
        model: None,
        total_cases: 1,
        passed: 0,
        failed: 1,
        trials_per_case: Some(1),
        parallel: None,
        pass_threshold: Some(1.0),
        run_dir: run_dir.clone(),
        checks_path: Some(checks_path),
        skill_project_root: root.to_path_buf(),
        isolation: None,
        judge_errors: None,
        judge_skipped_trials: None,
        judge_tokens: None,
        judge_cost_usd: None,
        skill_git_sha: None,
        skill_dirty: None,
        cases: vec![CaseSummary {
            id: case_id.to_string(),
            status: CaseStatus::Failed,
            command_count: recorded.command_count,
            input_tokens: None,
            output_tokens: None,
            pass_count: Some(0),
            total_trials: Some(1),
            pass_rate: Some(0.0),
            error_count: Some(0),
            scored_trials: Some(1),
            should_trigger,
            judge_excluded_count: None,
            scores: Default::default(),
            trials: vec![recorded],
        }],
    };
    write_summary(&run_dir, &summary).unwrap();
    run_dir
}

/// One `tool_use` frame naming the typed `Skill` tool: the skill was invoked.
const SKILL_INVOKED_TRACE: &str = concat!(
    r#"{"seq":0,"payload":{"type":"tool_use","call_id":"toolu_1","tool_name":"Skill","input":{"skill":"fastskill"}}}"#,
    "\n"
);

const SKILL_INVOKED_CHECK: &str =
    "[[check]]\nname = \"skill_invoked\"\nskill = \"fastskill\"\nexpected = true\nrequired = true\n";

fn passing_trial(command_count: Option<usize>) -> fastskill_evals::artifacts::TrialResult {
    use fastskill_evals::artifacts::{CaseStatus, TrialResult};
    TrialResult {
        trial_id: 1,
        status: CaseStatus::Passed,
        command_count,
        input_tokens: None,
        output_tokens: None,
        check_results: vec![],
        error_message: None,
        exit_code: Some(0),
        terminal: None,
        cost_usd: None,
        tokens: Default::default(),
        skill_path: None,
        judge_excluded: false,
    }
}

/// A trial that timed out or errored at run time carries a recorded
/// `status: error` and an `error_message`. `eval score` re-applies checks to
/// the saved trace, but checks passing over a *partial* trace are not a pass:
/// the recorded verdict must survive re-scoring, otherwise `score` reports
/// more passes than the `run` that wrote the artifacts (observed live: 10/14
/// from `run`, 11/14 from `score` on the same directory).
///
/// Under R4 the case now takes the verdict `error` rather than `failed`: its
/// only trial produced no measurement, so there is nothing left to average.
#[test]
fn test_eval_score_preserves_recorded_error_status() {
    use fastskill_evals::artifacts::{CaseStatus, TrialResult};
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let errored = TrialResult {
        status: CaseStatus::Error,
        command_count: Some(31),
        error_message: Some("EVAL_CASE_TIMEOUT: Case timed out after 300s".to_string()),
        ..passing_trial(Some(31))
    };
    // The skill WAS invoked before the case timed out, so every check passes
    // on the partial trace. Only the recorded verdict stops it reading as one.
    let run_dir = write_scoreable_run(
        dir.path(),
        "timeout-case",
        SKILL_INVOKED_CHECK,
        SKILL_INVOKED_TRACE,
        errored,
        None,
    );

    let result = run_fastskill_command(
        &[
            "eval",
            "score",
            "--run-dir",
            run_dir.to_str().unwrap(),
            "--json",
        ],
        None,
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        !result.success,
        "a timed-out trial must not become a pass on re-score; output: {combined}"
    );
    assert!(
        combined.contains("EVAL_CASES_UNMEASURED") && combined.contains("timeout-case"),
        "the exit must name the unmeasured case; got: {combined}"
    );

    let json_start = result.stdout.find('{').unwrap();
    let scored: serde_json::Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    assert_eq!(scored["passed"], 0);
    let case = &scored["cases"][0];
    assert_eq!(
        case["status"], "error",
        "a case with no scored trials is `error`, not a 0% fail; got: {case}"
    );
    assert_eq!(case["error_count"], 1);
    assert_eq!(case["scored_trials"], 0);
    assert_eq!(
        case["trials"][0]["status"], "error",
        "recorded trial error status must survive re-scoring"
    );
    assert_eq!(
        case["trials"][0]["error_message"], "EVAL_CASE_TIMEOUT: Case timed out after 300s",
        "recorded error_message must survive re-scoring"
    );
}

/// R11. `eval score` used to backfill counts into the run's `summary.json`
/// while reading it, so scoring the fixtures committed in `gofastskill/skill`
/// dirtied that repository's working tree. The writer owns the artifact.
#[test]
fn test_eval_score_does_not_write_to_the_run_directory() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = write_scoreable_run(
        dir.path(),
        "happy-case",
        SKILL_INVOKED_CHECK,
        SKILL_INVOKED_TRACE,
        passing_trial(Some(1)),
        None,
    );

    let summary_path = run_dir.join("summary.json");
    let before = std::fs::read(&summary_path).unwrap();

    let result = run_fastskill_command(
        &["eval", "score", "--run-dir", run_dir.to_str().unwrap()],
        None,
    );
    assert!(
        result.success,
        "the fixture scores clean; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );

    let after = std::fs::read(&summary_path).unwrap();
    assert_eq!(
        before, after,
        "eval score must not rewrite summary.json — scoring a committed fixture would dirty the tree"
    );
}

/// R7. The `should_trigger` column is scored, and `eval score` reads it off the
/// artifact so an offline re-score reaches the same verdict the run did. Here
/// the case says the skill must stay out of the way and the trace shows it was
/// invoked, so the case fails on a checks file that asserts nothing about it.
#[test]
fn test_eval_score_honours_the_recorded_should_trigger_column() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = write_scoreable_run(
        dir.path(),
        "no-trigger-case",
        "[[check]]\nname = \"max_tool_calls\"\nlimit = 100\nrequired = true\n",
        SKILL_INVOKED_TRACE,
        passing_trial(Some(1)),
        Some(false),
    );

    let result = run_fastskill_command(
        &["eval", "score", "--run-dir", run_dir.to_str().unwrap()],
        None,
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        !result.success,
        "should_trigger=false with the skill invoked must fail the case; got: {combined}"
    );
    assert!(
        combined.contains("cases: 0/1 passed"),
        "expected `cases: 0/1 passed`, got: {combined}"
    );
}

/// The mirror of the test above: the same trace and the same checks file, with
/// the column flipped, must pass. Without this half the check could be failing
/// for any reason at all.
#[test]
fn test_eval_score_passes_when_should_trigger_matches_the_trace() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = write_scoreable_run(
        dir.path(),
        "trigger-case",
        "[[check]]\nname = \"max_tool_calls\"\nlimit = 100\nrequired = true\n",
        SKILL_INVOKED_TRACE,
        passing_trial(Some(1)),
        Some(true),
    );

    let result = run_fastskill_command(
        &["eval", "score", "--run-dir", run_dir.to_str().unwrap()],
        None,
    );
    assert!(
        result.success,
        "should_trigger=true with the skill invoked must pass; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
}

/// A pre-R7 artifact has no `should_trigger` column. Guessing `false` would
/// invent an assertion the run never made and turn every archived run into a
/// failure, so the fallback is the explicit checks alone.
#[test]
fn test_eval_score_falls_back_to_explicit_checks_when_the_column_is_absent() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = write_scoreable_run(
        dir.path(),
        "legacy-case",
        "[[check]]\nname = \"max_tool_calls\"\nlimit = 100\nrequired = true\n",
        SKILL_INVOKED_TRACE,
        passing_trial(Some(1)),
        None,
    );

    let result = run_fastskill_command(
        &["eval", "score", "--run-dir", run_dir.to_str().unwrap()],
        None,
    );
    assert!(
        result.success,
        "an artifact without the column scores on its explicit checks; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
}

#[test]
fn test_eval_validate_json_reports_agent_availability() {
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
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();

    // Same fake-agent approach as test_eval_validate_agent_flag: the JSON
    // document must carry the per-agent availability that the table output
    // prints, otherwise `--all --json` gives a script no way to learn which
    // agents `eval run --all` would use.
    let bin_dir = dir.path().join("bin");
    let merged_path = install_fake_agent(&bin_dir, "codex");

    let result = run_fastskill_command_with_env(
        &["eval", "validate", "--agent", "codex", "--json"],
        &[("PATH", merged_path.as_str())],
        Some(dir.path()),
    );
    assert!(
        result.success,
        "eval validate --agent codex --json must succeed; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );
    let json_start = result.stdout.find('{').unwrap();
    let output: serde_json::Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    assert_eq!(output["valid"], true);
    assert_eq!(
        output["agents"]["codex"], true,
        "JSON output must report the probed agent; got: {}",
        output
    );
}

/// End-to-end proof that the scorecard reads real artifacts and drops errored
/// trials before folding their check results.
///
/// The fixture crashes two of five trials. Those two still carry check results
/// -- the checks ran, over an empty trace -- so a reader that folds
/// `check_results` without consulting `status` reports a denominator of 5. The
/// only honest denominator is 3.
#[test]
fn test_eval_scorecard_folds_runs_and_drops_errored_trials() {
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nop-crash,\"test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    let bin_dir = dir.path().join("bin");
    let runs_dir = dir.path().join("eval-runs").join("consultation");
    let state_dir = dir.path().join("state");
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let env_vars = vec![
        ("PATH", merged_path.as_str()),
        ("FASTSKILL_TEST_STATE_DIR", state_dir.to_str().unwrap()),
        ("FAKE_AGENT_MODE", "counter"),
        ("FAKE_AGENT_PASS_LIMIT", "3"),
    ];

    let run = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "codex",
            "--output-dir",
            runs_dir.to_str().unwrap(),
            "--case",
            "op-crash",
            "--trials",
            "5",
            "--no-fail",
        ],
        &env_vars,
        Some(dir.path()),
    );
    assert!(
        run.success,
        "--no-fail makes the run a measurement; stdout: {}, stderr: {}",
        run.stdout, run.stderr
    );

    let metrics_path = dir.path().join("metrics.toml");
    fs::write(
        &metrics_path,
        r#"
[[metric]]
name = "Skill-open rate"
kind = "check_rate"
cases = ["op-*"]
checks = ["skill_invoked"]
min_rate = 0.85

[[metric]]
name = "Efficiency"
kind = "tool_calls_p95"
cases = ["op-*"]
max = 25
"#,
    )
    .unwrap();

    let runs_root = dir.path().join("eval-runs");
    let args = [
        "eval",
        "scorecard",
        "--root",
        runs_root.to_str().unwrap(),
        "--metrics",
        metrics_path.to_str().unwrap(),
        "--json",
    ];

    // The fake agent never opens the skill, so a `should_trigger = true` case
    // fails its implicit skill-invocation check on every measured trial. The
    // metric is therefore below its threshold and the command exits non-zero.
    let gated = run_fastskill_command_with_env(&args, &env_vars, Some(dir.path()));
    assert!(
        !gated.success,
        "a metric below its threshold must fail the command; stdout: {}",
        gated.stdout
    );
    assert!(
        gated.stderr.contains("EVAL_SCORECARD_BELOW_THRESHOLD"),
        "stderr: {}",
        gated.stderr
    );

    let mut relaxed_args = args.to_vec();
    relaxed_args.push("--no-fail");
    let result = run_fastskill_command_with_env(&relaxed_args, &env_vars, Some(dir.path()));
    assert!(
        result.success,
        "--no-fail reports without gating; stdout: {}, stderr: {}",
        result.stdout, result.stderr
    );

    let json_start = result.stdout.find('{').unwrap();
    let card: Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();

    assert_eq!(card["totals"]["runs"], 1);
    assert_eq!(card["totals"]["trials"], 5);
    assert_eq!(
        card["totals"]["scored_trials"], 3,
        "two crashed trials carry no measurement: {}",
        result.stdout
    );
    assert_eq!(card["totals"]["error_trials"], 2);

    let open = &card["metrics"][0];
    assert_eq!(open["name"], "Skill-open rate");
    assert_eq!(
        open["observed"], 3,
        "the denominator is scored trials, never attempted trials: {}",
        result.stdout
    );
    assert_eq!(open["passed"], 0);
    assert_eq!(open["verdict"], "BELOW THRESHOLD");

    assert_eq!(card["metrics"][1]["name"], "Efficiency");
    assert_eq!(card["metrics"][1]["verdict"], "PASS");
}

/// A mistyped case pattern must not quietly delete a gate.
#[test]
fn test_eval_scorecard_refuses_a_metric_that_matched_nothing() {
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let run_dir = dir
        .path()
        .join("eval-runs/suite/2026-09-03T00-00-00Z/codex");
    fs::create_dir_all(&run_dir).unwrap();
    fs::write(
        run_dir.join("summary.json"),
        r#"{
          "suite_pass": true, "agent": "codex", "model": null,
          "total_cases": 1, "passed": 1, "failed": 0,
          "run_dir": "/tmp/run", "checks_path": null, "skill_project_root": "/tmp",
          "cases": [{
            "id": "op-init", "status": "passed",
            "command_count": 2, "input_tokens": null, "output_tokens": null,
            "trials": [{
              "trial_id": 1, "status": "passed",
              "command_count": 2, "input_tokens": null, "output_tokens": null,
              "check_results": [
                {"check_name": "skill_invoked", "passed": true, "required": true, "message": null}
              ],
              "error_message": null
            }]
          }]
        }"#,
    )
    .unwrap();

    let metrics_path = dir.path().join("metrics.toml");
    fs::write(
        &metrics_path,
        "[[metric]]\nname = \"Typo\"\nkind = \"check_rate\"\ncases = [\"typo-*\"]\nchecks = [\"skill_invoked\"]\nmin_rate = 0.5\n",
    )
    .unwrap();

    let result = run_fastskill_command(
        &[
            "eval",
            "scorecard",
            "--root",
            dir.path().join("eval-runs").to_str().unwrap(),
            "--metrics",
            metrics_path.to_str().unwrap(),
            "--no-fail",
        ],
        None,
    );
    assert!(
        !result.success,
        "--no-fail must not suppress a metric with no data; stdout: {}",
        result.stdout
    );
    assert!(
        result.stderr.contains("EVAL_SCORECARD_EMPTY_METRIC"),
        "stderr: {}",
        result.stderr
    );
}

/// One run directory holding a hand-written `summary.json`, so a scorecard test
/// can pin exactly what the artifacts said without running an agent.
fn staged_run(root: &std::path::Path, leaf: &str, agent: &str, model: &str, case_id: &str) {
    use std::fs;
    let run_dir = root.join(leaf);
    fs::create_dir_all(&run_dir).unwrap();
    let model_json = if model.is_empty() {
        "null".to_string()
    } else {
        format!("\"{}\"", model)
    };
    fs::write(
        run_dir.join("summary.json"),
        format!(
            r#"{{
              "suite_pass": true, "agent": "{agent}", "model": {model_json},
              "total_cases": 1, "passed": 1, "failed": 0,
              "run_dir": "/tmp/run", "checks_path": null, "skill_project_root": "/tmp/skill",
              "skill_git_sha": "abc1234def", "skill_dirty": false,
              "cases": [{{
                "id": "{case_id}", "status": "passed",
                "command_count": 2, "input_tokens": null, "output_tokens": null,
                "trials": [{{
                  "trial_id": 1, "status": "passed",
                  "command_count": 2, "input_tokens": null, "output_tokens": null,
                  "check_results": [
                    {{"check_name": "skill_invoked", "passed": true, "required": true, "message": null}}
                  ],
                  "error_message": null
                }}]
              }}]
            }}"#
        ),
    )
    .unwrap();
}

/// A metrics file whose one metric always matches the staged runs above.
fn staged_metrics(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("metrics.toml");
    std::fs::write(
        &path,
        "[[metric]]\nname = \"Skill-open rate\"\nkind = \"check_rate\"\nchecks = [\"skill_invoked\"]\nmin_rate = 0.5\n",
    )
    .unwrap();
    path
}

/// R2: the document names what was measured, not just the numbers. A rate
/// whose target, skill revision and benchmark are gone cannot be compared with
/// last month's, which is the only thing a scorecard is for.
#[test]
fn test_eval_scorecard_emits_the_identity_of_what_it_measured() {
    use serde_json::Value;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path().join("eval-runs");
    staged_run(
        &root,
        "2026-09-03T00-00-00Z/codex",
        "codex",
        "gpt-5",
        "op-init",
    );
    let metrics_path = staged_metrics(dir.path());

    let result = run_fastskill_command(
        &[
            "eval",
            "scorecard",
            "--root",
            root.to_str().unwrap(),
            "--metrics",
            metrics_path.to_str().unwrap(),
            "--json",
        ],
        None,
    );
    assert!(result.success, "stderr: {}", result.stderr);
    let json_start = result.stdout.find('{').unwrap();
    let card: Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();

    assert_eq!(card["schema"], "fastskill.scorecard/1");
    assert!(card["generated_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(card["agent"], "codex");
    assert_eq!(card["model"], "gpt-5");
    assert_eq!(card["targets"][0]["runs"], 1);
    assert_eq!(card["skill"]["git_sha"], "abc1234def");
    assert_eq!(card["skill"]["dirty"], false);
    assert_eq!(
        card["benchmark"]["sha256"],
        Value::Null,
        "a metrics file declaring no suites has no benchmark hash: {}",
        result.stdout
    );
    assert_eq!(
        card["runs"][0]["started_at"], "2026-09-03T00:00:00Z",
        "the run id in the path is the start time: {}",
        result.stdout
    );
    assert!(!card["fastskill_version"].as_str().unwrap().is_empty());
    assert!(!card["aikit_evals_version"].as_str().unwrap().is_empty());

    // R1: one row per (run, case), carrying what was observed rather than a fold.
    assert_eq!(card["cases"].as_array().unwrap().len(), 1);
    let case = &card["cases"][0];
    assert_eq!(case["case_id"], "op-init");
    assert_eq!(case["scored_trials"], 1);
    assert_eq!(case["checks"][0]["name"], "skill_invoked");
    assert_eq!(case["checks"][0]["observed"], true);

    // Every key the scorecard emitted before this document existed is still here.
    assert_eq!(card["totals"]["runs"], 1);
    assert_eq!(card["metrics"][0]["verdict"], "PASS");
    assert!(card["unclaimed_checks"].is_array());
}

/// R2/R5: two scorecards are comparable only when they asked the same question,
/// and `benchmark.sha256` is what says they did.
#[test]
fn test_eval_scorecard_hashes_every_file_the_benchmark_selects() {
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path().join("eval-runs");
    staged_run(
        &root,
        "2026-09-03T00-00-00Z/codex",
        "codex",
        "gpt-5",
        "op-init",
    );

    let suite = dir.path().join("suites/consultation");
    fs::create_dir_all(&suite).unwrap();
    fs::write(
        suite.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nop-init,\"go\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(
        suite.join("checks.toml"),
        "[[check]]\nname = \"skill_invoked\"\n",
    )
    .unwrap();

    let metrics_path = dir.path().join("metrics.toml");
    // `suites` is a top-level key, so it has to precede the first [[metric]]
    // table header — after it, TOML would read it as a field of that metric.
    let metrics = "suites = [\"./suites/consultation\"]\n\n[[metric]]\nname = \"Skill-open rate\"\nkind = \"check_rate\"\nchecks = [\"skill_invoked\"]\nmin_rate = 0.5\n";
    fs::write(&metrics_path, metrics).unwrap();

    let args = [
        "eval",
        "scorecard",
        "--root",
        root.to_str().unwrap(),
        "--metrics",
        metrics_path.to_str().unwrap(),
        "--json",
    ];
    let hash_of = |args: &[&str]| -> String {
        let out = run_fastskill_command(args, None);
        assert!(out.success, "stderr: {}", out.stderr);
        let json_start = out.stdout.find('{').unwrap();
        let card: Value = serde_json::from_str(&out.stdout[json_start..]).unwrap();
        assert!(
            card["benchmark"]["sha256"].is_string(),
            "a declared suite must produce a hash: {}",
            out.stdout
        );
        card["benchmark"]["sha256"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };

    let before = hash_of(&args);
    assert_eq!(before.len(), 64, "sha256 hex");

    // Change one case in one suite file: a different question was asked, so the
    // benchmark is a different benchmark and the hash must say so.
    fs::write(
        suite.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nop-init,\"go further\",true,\"basic\",\n",
    )
    .unwrap();
    let after = hash_of(&args);
    assert_ne!(
        before, after,
        "editing a suite file must change the benchmark hash"
    );

    // ...and a file the benchmark does not select is not part of the question,
    // so it must not move the hash. Without this half the requirement is met
    // by hashing the whole directory tree, which would make every scorecard
    // incomparable with every other for reasons no reader could see.
    fs::write(dir.path().join("notes.txt"), "scratch, not a suite file").unwrap();
    fs::create_dir_all(dir.path().join("suites/unselected")).unwrap();
    fs::write(
        dir.path().join("suites/unselected/prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nother,\"x\",true,\"basic\",\n",
    )
    .unwrap();
    assert_eq!(
        after,
        hash_of(&args),
        "only the files the suites list selects belong to the benchmark"
    );
}

/// R4: a rate averaged over two agents is a measurement of neither.
#[test]
fn test_eval_scorecard_refuses_two_targets_under_one_root() {
    use serde_json::Value;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path().join("eval-runs");
    staged_run(
        &root,
        "2026-09-03T00-00-00Z/codex",
        "codex",
        "gpt-5",
        "op-init",
    );
    staged_run(
        &root,
        "2026-09-03T01-00-00Z/claude",
        "claude",
        "opus",
        "off-idle",
    );
    let metrics_path = staged_metrics(dir.path());

    let args = [
        "eval",
        "scorecard",
        "--root",
        root.to_str().unwrap(),
        "--metrics",
        metrics_path.to_str().unwrap(),
        "--json",
    ];
    let refused = run_fastskill_command(&args, None);
    assert!(
        !refused.success,
        "two targets must not be folded silently; stdout: {}",
        refused.stdout
    );
    assert!(
        refused.stderr.contains("EVAL_SCORECARD_MIXED_TARGETS")
            && refused.stderr.contains("codex/gpt-5")
            && refused.stderr.contains("claude/opus"),
        "the error must name the offending targets; stderr: {}",
        refused.stderr
    );

    // `--no-fail` says "report the numbers rather than gate on them". It does
    // not say the numbers mean something they do not.
    let mut no_fail = args.to_vec();
    no_fail.push("--no-fail");
    assert!(
        !run_fastskill_command(&no_fail, None).success,
        "--no-fail must not suppress a mixed-measurement guard"
    );

    let mut allowed = args.to_vec();
    allowed.push("--allow-mixed-targets");
    let folded = run_fastskill_command(&allowed, None);
    assert!(folded.success, "stderr: {}", folded.stderr);
    let json_start = folded.stdout.find('{').unwrap();
    let card: Value = serde_json::from_str(&folded.stdout[json_start..]).unwrap();
    assert_eq!(
        card["agent"],
        Value::Null,
        "with two targets there is no one agent this card is about: {}",
        folded.stdout
    );
    assert_eq!(card["targets"].as_array().unwrap().len(), 2);
    assert_eq!(
        card["metrics"][0]["mixed_targets"], true,
        "the override records itself: {}",
        folded.stdout
    );
}

/// R4: the same case measured twice is counted twice, and the reader is told.
#[test]
fn test_eval_scorecard_refuses_a_case_measured_by_two_runs() {
    use serde_json::Value;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path().join("eval-runs");
    staged_run(
        &root,
        "2026-09-03T00-00-00Z/codex",
        "codex",
        "gpt-5",
        "op-init",
    );
    staged_run(
        &root,
        "2026-09-03T01-00-00Z/codex",
        "codex",
        "gpt-5",
        "op-init",
    );
    let metrics_path = staged_metrics(dir.path());

    let args = [
        "eval",
        "scorecard",
        "--root",
        root.to_str().unwrap(),
        "--metrics",
        metrics_path.to_str().unwrap(),
        "--json",
    ];
    let refused = run_fastskill_command(&args, None);
    assert!(!refused.success, "stdout: {}", refused.stdout);
    assert!(
        refused.stderr.contains("EVAL_SCORECARD_DUPLICATE_CASES")
            && refused.stderr.contains("op-init"),
        "stderr: {}",
        refused.stderr
    );

    let mut allowed = args.to_vec();
    allowed.push("--allow-duplicate-cases");
    let folded = run_fastskill_command(&allowed, None);
    assert!(folded.success, "stderr: {}", folded.stderr);
    let json_start = folded.stdout.find('{').unwrap();
    let card: Value = serde_json::from_str(&folded.stdout[json_start..]).unwrap();
    let rows = card["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "every occurrence keeps its own row");
    assert_ne!(
        rows[0]["run_dir"], rows[1]["run_dir"],
        "each row names the run it came from: {}",
        folded.stdout
    );
    assert_eq!(
        card["totals"]["cases"], 2,
        "the totals count each occurrence"
    );
}

/// R1 / ADR 0020: the scorecard grew, and a reader written against the shape
/// it had before must still be able to read it.
#[test]
fn test_eval_scorecard_stays_readable_by_a_reader_written_against_the_old_shape() {
    use serde_json::Value;
    use tempfile::TempDir;

    /// Exactly the fields the scorecard emitted before this change. If any of
    /// them was renamed, retyped or dropped, this fails to deserialise.
    #[derive(serde::Deserialize)]
    struct OldReader {
        metrics: Vec<OldMetric>,
        totals: serde_json::Map<String, Value>,
        unclaimed_checks: Vec<String>,
    }
    #[derive(serde::Deserialize)]
    struct OldMetric {
        name: String,
        verdict: String,
        rate: Option<f64>,
    }

    let dir = TempDir::new().unwrap();
    let root = dir.path().join("eval-runs");
    staged_run(
        &root,
        "2026-09-03T00-00-00Z/codex",
        "codex",
        "gpt-5",
        "op-init",
    );
    let metrics_path = staged_metrics(dir.path());

    let out = run_fastskill_command(
        &[
            "eval",
            "scorecard",
            "--root",
            root.to_str().unwrap(),
            "--metrics",
            metrics_path.to_str().unwrap(),
            "--json",
        ],
        None,
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let json_start = out.stdout.find('{').unwrap();
    let parsed = serde_json::from_str::<OldReader>(&out.stdout[json_start..]);
    assert!(
        parsed.is_ok(),
        "old-shape reader must still parse: {:?}\n{}",
        parsed.as_ref().err(),
        out.stdout
    );
    let old = parsed.unwrap();

    assert_eq!(old.metrics.len(), 1);
    assert_eq!(old.metrics[0].name, "Skill-open rate");
    assert_eq!(old.metrics[0].verdict, "PASS");
    assert!(
        old.metrics[0].rate.is_some(),
        "rate keeps its name and type"
    );
    assert!(old.totals.contains_key("runs"), "totals keeps its key");
    assert!(old.unclaimed_checks.is_empty());
}

/// R4: two judge identities folded into one score is a mixed measurement, and
/// the reader cannot see it in the number. The command refuses by name.
#[test]
fn test_eval_scorecard_refuses_two_judge_identities_in_one_score() {
    use serde_json::Value;
    use std::fs;
    use tempfile::TempDir;

    /// One judgment record, as `trial-N/judgments.json` holds them.
    fn stage_judgment(run_dir: &std::path::Path, case_id: &str, trial: u32, hash: &str) {
        let dir = run_dir.join(case_id).join(format!("trial-{trial}"));
        fs::create_dir_all(&dir).unwrap();
        let record = serde_json::json!([{
            "schema": "aikit.judgment/1",
            "judge": "command-correctness",
            "judge_hash": hash,
            "cache_key": format!("{hash}-command-correctness"),
            "identity": {
                "model": "judge-1", "model_reported": null,
                "endpoint_host": "api.example.com",
                "temperature": 0.0, "top_p": null, "max_tokens": 1024
            },
            "attempts": [],
            "scores": {"overall": 0.9},
            "error": null,
            "usage": {"input": 100, "output": 20, "total": 120},
            "cost_usd": null,
            "truncated": [],
            "judged_at": "2026-09-04T12:00:00Z"
        }]);
        fs::write(
            dir.join("judgments.json"),
            serde_json::to_string_pretty(&record).unwrap(),
        )
        .unwrap();
    }

    let dir = TempDir::new().unwrap();
    let run_dir = dir.path().join("eval-runs/2026-09-03T00-00-00Z/codex");
    fs::create_dir_all(&run_dir).unwrap();
    fs::write(
        run_dir.join("summary.json"),
        r#"{
          "suite_pass": true, "agent": "codex", "model": "gpt-5",
          "total_cases": 1, "passed": 1, "failed": 0,
          "run_dir": "/tmp/run", "checks_path": null, "skill_project_root": "/tmp/skill",
          "cases": [{
            "id": "op-init", "status": "passed",
            "command_count": 2, "input_tokens": null, "output_tokens": null,
            "trials": [
              {"trial_id": 1, "status": "passed", "command_count": 2,
               "input_tokens": null, "output_tokens": null, "error_message": null,
               "check_results": []},
              {"trial_id": 2, "status": "passed", "command_count": 2,
               "input_tokens": null, "output_tokens": null, "error_message": null,
               "check_results": []}
            ]
          }]
        }"#,
    )
    .unwrap();
    // Same judge name, same prompt, two different resolved identities: exactly
    // the case a name-keyed fold would hide.
    stage_judgment(&run_dir, "op-init", 1, "hash-aaa");
    stage_judgment(&run_dir, "op-init", 2, "hash-bbb");

    let metrics_path = dir.path().join("metrics.toml");
    fs::write(
        &metrics_path,
        "[[metric]]\nname = \"Command correctness\"\nkind = \"judge_score\"\njudges = [\"command-correctness\"]\nmin_score = 0.5\n",
    )
    .unwrap();

    let runs_root = dir.path().join("eval-runs");
    let args = [
        "eval",
        "scorecard",
        "--root",
        runs_root.to_str().unwrap(),
        "--metrics",
        metrics_path.to_str().unwrap(),
        "--json",
    ];
    let refused = run_fastskill_command(&args, None);
    assert!(
        !refused.success,
        "two judge identities must not fold into one score; stdout: {}",
        refused.stdout
    );
    assert!(
        refused.stderr.contains("EVAL_SCORECARD_MIXED_JUDGES")
            && refused.stderr.contains("Command correctness"),
        "the error must name the metric; stderr: {}",
        refused.stderr
    );

    let mut no_fail = args.to_vec();
    no_fail.push("--no-fail");
    assert!(
        !run_fastskill_command(&no_fail, None).success,
        "--no-fail must not suppress a mixed-measurement guard"
    );

    let mut allowed = args.to_vec();
    allowed.push("--allow-mixed-judges");
    let folded = run_fastskill_command(&allowed, None);
    assert!(folded.success, "stderr: {}", folded.stderr);
    let json_start = folded.stdout.find('{').unwrap();
    let card: Value = serde_json::from_str(&folded.stdout[json_start..]).unwrap();
    assert_eq!(
        card["metrics"][0]["mixed_judges"], true,
        "the override records itself in the artifact: {}",
        folded.stdout
    );
    let hashes: Vec<&str> = card["judges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["judge_hash"].as_str().unwrap())
        .collect();
    assert!(
        hashes.contains(&"hash-aaa") && hashes.contains(&"hash-bbb"),
        "every identity that contributed is listed: {}",
        folded.stdout
    );
}

// ---------------------------------------------------------------------------
// eval scorecard --format html (spec eval-scorecard-report R5, R6)
// ---------------------------------------------------------------------------

/// The reasoning one staged judgment carries. Distinctive on purpose: the
/// `--no-reasoning` test asserts this exact string appears nowhere in the file.
const STAGED_REASONING: &str =
    "The transcript names every flag the rubric asks about, in the order it asks.";

/// A run directory with one judged trial. `passed` decides whether the
/// `skill_invoked` check holds, which is what makes two cards' verdicts differ.
fn staged_judged_run(
    root: &std::path::Path,
    leaf: &str,
    passed: bool,
    reasoning: &str,
) -> std::path::PathBuf {
    use std::fs;
    let run_dir = root.join(leaf);
    fs::create_dir_all(&run_dir).unwrap();
    fs::write(
        run_dir.join("summary.json"),
        format!(
            r#"{{
              "suite_pass": {passed}, "agent": "codex", "model": "gpt-5",
              "total_cases": 1, "passed": 1, "failed": 0,
              "run_dir": "/tmp/run", "checks_path": null, "skill_project_root": "/tmp/skill",
              "skill_git_sha": "abc1234def", "skill_dirty": false,
              "cases": [{{
                "id": "op-init", "status": "passed",
                "command_count": 2, "input_tokens": null, "output_tokens": null,
                "trials": [{{
                  "trial_id": 1, "status": "passed",
                  "command_count": 2, "input_tokens": null, "output_tokens": null,
                  "check_results": [
                    {{"check_name": "skill_invoked", "passed": {passed}, "required": true, "message": null}}
                  ],
                  "error_message": null
                }}]
              }}]
            }}"#
        ),
    )
    .unwrap();

    let trial_dir = run_dir.join("op-init").join("trial-1");
    fs::create_dir_all(&trial_dir).unwrap();
    let reply = serde_json::json!({
        "criteria": [{"name": "clarity", "answer": 4, "reasoning": reasoning}],
        "notes": null
    })
    .to_string();
    let record = serde_json::json!([{
        "schema": "aikit.judgment/1",
        "judge": "command-correctness",
        "judge_hash": "hash-aaa",
        "cache_key": "hash-aaa-command-correctness",
        "identity": {
            "model": "judge-1", "model_reported": null,
            "endpoint_host": "api.example.com",
            "temperature": 0.0, "top_p": null, "max_tokens": 1024
        },
        "attempts": [{
            "kind": "validation", "request": {}, "response_text": reply,
            "finish_reason": null, "usage": null, "error": null
        }],
        "scores": {"overall": 0.9, "clarity": 0.9},
        "error": null,
        "usage": {"input": 100, "output": 20, "total": 120},
        "cost_usd": null,
        "truncated": [],
        "judged_at": "2026-09-04T12:00:00Z"
    }]);
    fs::write(
        trial_dir.join("judgments.json"),
        serde_json::to_string_pretty(&record).unwrap(),
    )
    .unwrap();
    run_dir
}

/// A metrics file that declares a suite, so the cards it produces carry a
/// `benchmark.sha256` and `--from` will compare them (R5).
fn hashed_metrics(dir: &std::path::Path) -> std::path::PathBuf {
    use std::fs;
    let suite = dir.join("suites/consultation");
    fs::create_dir_all(&suite).unwrap();
    fs::write(
        suite.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\nop-init,\"go\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(
        suite.join("checks.toml"),
        "[[check]]\nname = \"skill_invoked\"\n",
    )
    .unwrap();

    let path = dir.join("metrics.toml");
    fs::write(
        &path,
        "suites = [\"./suites/consultation\"]\n\n\
         [[metric]]\nname = \"Skill-open rate\"\nkind = \"check_rate\"\n\
         checks = [\"skill_invoked\"]\nmin_rate = 0.5\n\n\
         [[metric]]\nname = \"Command correctness\"\nkind = \"judge_score\"\n\
         judges = [\"command-correctness\"]\nmin_score = 0.5\n",
    )
    .unwrap();
    path
}

/// Every `href="…"` and `src="…"` value in `page`, in document order.
fn linked_urls(page: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attribute in ["href=\"", "src=\""] {
        let mut rest = page;
        while let Some(at) = rest.find(attribute) {
            rest = &rest[at + attribute.len()..];
            let end = rest.find('"').unwrap_or(rest.len());
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        }
    }
    out
}

/// The contents of the one `<script type="application/json">`, parsed.
fn embedded_cards(page: &str) -> Vec<serde_json::Value> {
    let open = "<script type=\"application/json\" id=\"scorecards\">";
    let start = page.find(open).expect("the embedded JSON block") + open.len();
    let end = start + page[start..].find("</script>").expect("a closed script");
    serde_json::from_str(&page[start..end]).expect("the embedded block is JSON")
}

/// R6: the report is one file. A run directory is scratch space that gets
/// deleted, and the machine that opens the report months later is usually not
/// the machine that produced it — so a `<link>`, a CDN font or a remote script
/// is a report that renders differently, or not at all, exactly when it matters.
#[test]
fn test_eval_scorecard_html_is_one_self_contained_file() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path().join("runs-a");
    let run_dir = staged_judged_run(&root, "2026-09-01T00-00-00Z/codex", true, STAGED_REASONING);
    let metrics_path = hashed_metrics(dir.path());
    let out_path = dir.path().join("report.html");

    let result = run_fastskill_command(
        &[
            "eval",
            "scorecard",
            "--root",
            root.to_str().unwrap(),
            "--metrics",
            metrics_path.to_str().unwrap(),
            "--format",
            "html",
            "-o",
            out_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(result.success, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("Wrote "),
        "-o says where it put the file: {}",
        result.stdout
    );
    let page = std::fs::read_to_string(&out_path).unwrap();

    assert_eq!(
        page.matches("<script").count(),
        1,
        "the report runs no code; the one script element is the data block"
    );
    assert!(
        page.contains("<script type=\"application/json\" id=\"scorecards\">"),
        "the one script element must be the JSON data block"
    );
    assert!(
        !page.contains("<link"),
        "a stylesheet or icon fetched from anywhere is a report that renders differently"
    );

    let urls = linked_urls(&page);
    assert!(
        urls.iter().any(|u| u == run_dir.to_str().unwrap()),
        "the run directory is on this machine, so it is a link: {urls:?}"
    );
    for url in &urls {
        assert!(
            !url.contains("http"),
            "no href or src may leave this file: {url}"
        );
    }

    // The fonts travel inside the file, under a licence the file reproduces.
    assert!(
        page.contains("src:url(data:font/woff2;base64,"),
        "the faces are embedded, not fetched"
    );
    assert!(
        page.contains("SIL OPEN FONT LICENSE"),
        "embedding IBM Plex means shipping its licence"
    );

    // The embedded block is the scorecard itself, not a summary of it.
    let cards = embedded_cards(&page);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["schema"], "fastskill.scorecard/1");
    assert_eq!(cards[0]["cases"][0]["case_id"], "op-init");
}

/// R6: `--no-reasoning` is a statement about the file, not about the tables in
/// it. A report that hides reasoning on screen and ships it in the embedded
/// JSON is worse than one that shows it, because the reader believes it is gone.
#[test]
fn test_eval_scorecard_html_no_reasoning_strips_the_whole_file() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path().join("runs-a");
    staged_judged_run(&root, "2026-09-01T00-00-00Z/codex", true, STAGED_REASONING);
    let metrics_path = hashed_metrics(dir.path());

    let render = |extra: &[&str]| -> String {
        let out_path = dir.path().join(format!("report{}.html", extra.len()));
        let mut args = vec![
            "eval",
            "scorecard",
            "--root",
            root.to_str().unwrap(),
            "--metrics",
            metrics_path.to_str().unwrap(),
            "--format",
            "html",
            "-o",
            out_path.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let result = run_fastskill_command(&args, None);
        assert!(result.success, "stderr: {}", result.stderr);
        std::fs::read_to_string(&out_path).unwrap()
    };

    // Without the flag the reasoning is there — twice over, once for the reader
    // and once in the data block. Without this half the assertion below passes
    // against a fixture that never carried reasoning at all.
    let shown = render(&[]);
    assert!(
        shown.contains(STAGED_REASONING),
        "the fixture's reasoning must reach the tables"
    );
    assert_eq!(
        embedded_cards(&shown)[0]["cases"][0]["judgments"][0]["criteria"][0]["reasoning"],
        STAGED_REASONING,
        "and the data block"
    );

    let withheld = render(&["--no-reasoning"]);
    assert!(
        !withheld.contains(STAGED_REASONING),
        "--no-reasoning must leave the text nowhere in the file, data block included"
    );
    assert!(
        withheld.contains("withheld"),
        "the table says the reasoning was withheld rather than showing an empty cell"
    );
    // The document is stripped of one field, not truncated: everything a
    // reader needs to find the judgment again is still there.
    let cards = embedded_cards(&withheld);
    let criterion = &cards[0]["cases"][0]["judgments"][0]["criteria"][0];
    assert_eq!(criterion["name"], "clarity");
    assert!(
        criterion.get("reasoning").is_none(),
        "the key itself is gone, not blanked: {criterion}"
    );
}

/// R5: a progress chart over two benchmarks draws two questions on one axis.
/// There is no override for that, and the point where a verdict flipped is the
/// news the chart exists to carry.
#[test]
fn test_eval_scorecard_html_progress_needs_one_benchmark_and_marks_every_flip() {
    use serde_json::Value;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let metrics_path = hashed_metrics(dir.path());

    // Same target, same benchmark, two runs: the second one stopped opening the
    // skill, so "Skill-open rate" flips and "Command correctness" does not.
    let root_a = dir.path().join("runs-a");
    let root_b = dir.path().join("runs-b");
    staged_judged_run(
        &root_a,
        "2026-09-01T00-00-00Z/codex",
        true,
        STAGED_REASONING,
    );
    staged_judged_run(
        &root_b,
        "2026-09-02T00-00-00Z/codex",
        false,
        STAGED_REASONING,
    );

    // Render one scorecard to JSON and stamp it with a fixed `generated_at`,
    // so the chart's time axis is the fixture's rather than the clock's.
    let card_file =
        |root: &std::path::Path, name: &str, generated_at: &str| -> std::path::PathBuf {
            let out = run_fastskill_command(
                &[
                    "eval",
                    "scorecard",
                    "--root",
                    root.to_str().unwrap(),
                    "--metrics",
                    metrics_path.to_str().unwrap(),
                    "--json",
                    "--no-fail",
                ],
                None,
            );
            assert!(out.success, "stderr: {}", out.stderr);
            let start = out.stdout.find('{').unwrap();
            let mut card: Value = serde_json::from_str(&out.stdout[start..]).unwrap();
            card["generated_at"] = Value::String(generated_at.to_string());
            let path = dir.path().join(name);
            std::fs::write(&path, serde_json::to_string(&card).unwrap()).unwrap();
            path
        };

    let a = card_file(&root_a, "a.json", "2026-09-01T00:00:00Z");
    let b = card_file(&root_b, "b.json", "2026-09-02T00:00:00Z");
    let out_path = dir.path().join("progress.html");

    // Named newest first on purpose: the chart reads left to right in time
    // whatever order the files arrived in.
    let result = run_fastskill_command(
        &[
            "eval",
            "scorecard",
            "--format",
            "html",
            "--from",
            b.to_str().unwrap(),
            "--from",
            a.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(result.success, "stderr: {}", result.stderr);
    let page = std::fs::read_to_string(&out_path).unwrap();

    assert!(
        page.contains("generated 2026-09-02T00:00:00Z"),
        "the newest card is the one the report describes"
    );
    assert_eq!(
        page.matches("class=\"pt s0 flip\"").count(),
        1,
        "one verdict flip in the fixture, one highlighted point"
    );
    assert!(
        page.contains("1 verdict change<"),
        "and the chart says so in words: {}",
        &page[page.find("<h2>Progress</h2>").unwrap_or(0)..]
    );

    // R6: rendering from `--from` touches no run directory — not even to ask
    // whether one exists. Those paths were written on another machine.
    assert!(
        linked_urls(&page).is_empty(),
        "a `--from` render links nothing: {:?}",
        linked_urls(&page)
    );

    // Two benchmarks are two questions.
    let mut card: Value = serde_json::from_str(&std::fs::read_to_string(&b).unwrap()).unwrap();
    card["benchmark"]["sha256"] = Value::String("f".repeat(64));
    std::fs::write(&b, serde_json::to_string(&card).unwrap()).unwrap();
    let mismatched = run_fastskill_command(
        &[
            "eval",
            "scorecard",
            "--format",
            "html",
            "--from",
            b.to_str().unwrap(),
            "--from",
            a.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        !mismatched.success,
        "two benchmarks must not be drawn on one axis; stdout: {}",
        mismatched.stdout
    );
    assert!(
        mismatched
            .stderr
            .contains("EVAL_SCORECARD_BENCHMARK_MISMATCH"),
        "stderr: {}",
        mismatched.stderr
    );

    // ...and a card that never declared a benchmark cannot be compared at all.
    card["benchmark"]["sha256"] = Value::Null;
    std::fs::write(&b, serde_json::to_string(&card).unwrap()).unwrap();
    let unhashed = run_fastskill_command(
        &[
            "eval",
            "scorecard",
            "--format",
            "html",
            "--from",
            b.to_str().unwrap(),
            "--from",
            a.to_str().unwrap(),
            "-o",
            out_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(!unhashed.success, "stdout: {}", unhashed.stdout);
    assert!(
        unhashed.stderr.contains("EVAL_SCORECARD_NO_BENCHMARK_HASH"),
        "stderr: {}",
        unhashed.stderr
    );
}

// ---------------------------------------------------------------------------
// eval judge / eval run --judge / eval validate judge rules (spec eval-judge
// R13, R14)
// ---------------------------------------------------------------------------

/// A skill project whose checks file is exactly `checks_toml`. Returns the
/// temp dir; every judge test below differs only in that string.
fn judge_project(checks_toml: &str) -> tempfile::TempDir {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let evals_dir = dir.path().join("evals");
    fs::create_dir_all(&evals_dir).unwrap();
    fs::write(
        evals_dir.join("prompts.csv"),
        "id,prompt,should_trigger,tags,workspace_subdir\ntest-1,\"Test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(evals_dir.join("checks.toml"), checks_toml).unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\nchecks = \"evals/checks.toml\"\ntimeout_seconds = 300\nfail_on_missing_agent = false\n",
    )
    .unwrap();
    dir
}

/// One `[[judge]]`, with the prompt spliced in so each test can leave out the
/// one variable it is about.
fn judge_checks(prompt: &str, extra: &str) -> String {
    format!(
        "[[judge]]\nname = \"quality\"\nmodel = \"judge-1\"\nprompt = \"\"\"\n{prompt}\n\"\"\"\n{extra}\n[[judge.criterion]]\nname = \"clear\"\nkind = \"scale\"\ndescription = \"Is the answer clear?\"\n"
    )
}

/// R14: a prompt that never renders `{{output_contract}}` leaves the model
/// guessing the reply shape, so the envelope can only fail. It is an error,
/// found from file content alone — no endpoint, no key.
#[test]
fn test_eval_validate_rejects_a_judge_prompt_without_the_output_contract() {
    let dir = judge_project(&judge_checks(
        "Answer: {{trial.final_answer}}\n{{rubric}}",
        "",
    ));
    let result = run_fastskill_command_with_env(
        &["eval", "validate"],
        // Emptied deliberately: the check must not depend on a key or on
        // being able to reach anything.
        &[("OPENAI_API_KEY", ""), ("AIKIT_LLM_URL", "")],
        Some(dir.path()),
    );
    assert!(
        !result.success,
        "a judge prompt without {{{{output_contract}}}} must fail validation; stdout: {}",
        result.stdout
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("EVAL_JUDGE_INVALID") && combined.contains("output_contract"),
        "combined: {}",
        combined
    );
}

/// R14: a prompt without `{{rubric}}` still scores, it just scores criteria the
/// model was never shown. That is a warning, not an error.
#[test]
fn test_eval_validate_warns_when_a_judge_prompt_has_no_rubric() {
    let dir = judge_project(&judge_checks(
        "Answer: {{trial.final_answer}}\n{{output_contract}}",
        "",
    ));
    let result = run_fastskill_command_with_env(
        &["eval", "validate"],
        &[("OPENAI_API_KEY", ""), ("AIKIT_LLM_URL", "")],
        Some(dir.path()),
    );
    assert!(
        result.success,
        "a missing {{{{rubric}}}} is a warning, not an error; stderr: {}",
        result.stderr
    );
    assert!(
        result.stderr.contains("rubric"),
        "the warning must name what is missing; stderr: {}",
        result.stderr
    );
    assert!(
        result.stdout.contains("judges: quality"),
        "validate must list the judges it checked; stdout: {}",
        result.stdout
    );
}

/// R14: a judge grading the model that produced the answer is self-preference.
/// `--model` is how validate learns the target, because asking a runtime would
/// be a network call.
#[test]
fn test_eval_validate_warns_when_a_judge_shares_the_target_model() {
    let dir = judge_project(&judge_checks(
        "Answer: {{trial.final_answer}}\n{{rubric}}\n{{output_contract}}",
        "",
    ));
    let result = run_fastskill_command(
        &["eval", "validate", "--model", "judge-1"],
        Some(dir.path()),
    );
    assert!(result.success, "stderr: {}", result.stderr);
    assert!(
        result.stderr.contains("model under test"),
        "stderr: {}",
        result.stderr
    );

    // The same file with a different target is clean: the warning is about the
    // two models matching, not about declaring a model at all.
    let other = run_fastskill_command(
        &["eval", "validate", "--model", "some-other-model"],
        Some(dir.path()),
    );
    assert!(other.success, "stderr: {}", other.stderr);
    assert!(
        !other.stderr.contains("model under test"),
        "stderr: {}",
        other.stderr
    );
}

/// R14: `cases` names exact case ids, so one that matches nothing means the
/// judge silently judges no trial at all.
#[test]
fn test_eval_validate_rejects_a_judge_case_id_that_matches_no_case() {
    let dir = judge_project(&judge_checks(
        "Answer: {{trial.final_answer}}\n{{rubric}}\n{{output_contract}}",
        "cases = [\"no-such-case\"]\n",
    ));
    let result = run_fastskill_command(&["eval", "validate"], Some(dir.path()));
    assert!(!result.success, "stdout: {}", result.stdout);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("no-such-case"),
        "the error must name the id that matched nothing; combined: {}",
        combined
    );
}

/// R14: an unknown key inside a `[[judge]]` is a parse error — a judge must
/// never be quietly ignored by a misspelling. `agent` is the specific key the
/// spec names: a judge is one native completion, never an agent.
#[test]
fn test_eval_validate_rejects_an_unknown_key_in_a_judge() {
    let dir = judge_project(&judge_checks(
        "Answer: {{trial.final_answer}}\n{{rubric}}\n{{output_contract}}",
        "agent = \"claude\"\n",
    ));
    let result = run_fastskill_command(&["eval", "validate"], Some(dir.path()));
    assert!(!result.success, "stdout: {}", result.stdout);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("agent"),
        "the error must name the unknown key; combined: {}",
        combined
    );
}

/// A checks file with no `[[judge]]` validates exactly as it did before the
/// judge tier existed.
#[test]
fn test_eval_validate_reports_no_judges_when_none_are_declared() {
    let dir = judge_project(
        "[[check]]\nname = \"trigger_expectation\"\npattern = \"fastskill\"\nexpected = true\n",
    );
    let result = run_fastskill_command(&["eval", "validate", "--json"], Some(dir.path()));
    assert!(result.success, "stderr: {}", result.stderr);
    let json_start = result.stdout.find('{').unwrap();
    let output: serde_json::Value = serde_json::from_str(&result.stdout[json_start..]).unwrap();
    assert_eq!(output["judges"], serde_json::json!([]));
    assert_eq!(output["judge_warnings"], serde_json::json!([]));
}

#[test]
fn test_eval_judge_requires_run_dir() {
    let result = run_fastskill_command(&["eval", "judge"], None);
    assert!(!result.success);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("run-dir"),
        "the error must name the missing option; combined: {}",
        combined
    );
}

#[test]
fn test_eval_judge_nonexistent_run_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let missing = dir.path().join("no-such-run");
    let result = run_fastskill_command(
        &["eval", "judge", "--run-dir", missing.to_str().unwrap()],
        None,
    );
    assert!(!result.success);
    assert!(
        result.stderr.contains("EVAL_ARTIFACTS_CORRUPT"),
        "stderr: {}",
        result.stderr
    );
}

/// The range is checked before anything is read, so an impossible value is
/// reported as what the user typed rather than as a downstream failure.
#[test]
fn test_eval_judge_rejects_an_out_of_range_parallel() {
    let dir = tempfile::TempDir::new().unwrap();
    let result = run_fastskill_command(
        &[
            "eval",
            "judge",
            "--run-dir",
            dir.path().to_str().unwrap(),
            "--judge-parallel",
            "0",
        ],
        None,
    );
    assert!(!result.success);
    assert!(
        result.stderr.contains("EVAL_JUDGE_PARALLEL_INVALID"),
        "stderr: {}",
        result.stderr
    );
}

/// `--judge` on a run whose checks file declares none says so and changes
/// nothing: the verdict is the run's own, and no request is made.
#[test]
fn test_eval_run_judge_with_no_judges_declared_is_a_no_op() {
    use std::fs;
    let dir = judge_project("[[check]]\nname = \"trigger_expectation\"\npattern = \"greeting-helper\"\nexpected = true\n");
    fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: greeting-helper\n---\nbody\n",
    )
    .unwrap();
    let bin_dir = dir.path().join("bin");
    let path = install_fake_agent(&bin_dir, "agent");
    let out_dir = dir.path().join("eval-runs");

    let result = run_fastskill_command_with_env(
        &[
            "eval",
            "run",
            "--agent",
            "aikit",
            "--output-dir",
            out_dir.to_str().unwrap(),
            "--judge",
            "--no-fail",
        ],
        &[("PATH", &path)],
        Some(dir.path()),
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("no [[judge]] declared"),
        "combined: {}",
        combined
    );

    // Nothing was judged, so the run's summary carries no judge totals.
    let summary_path = std::fs::read_dir(&out_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path().join("aikit").join("summary.json"))
        .find(|p| p.is_file())
        .expect("a summary.json under the run dir");
    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(summary_path).unwrap()).unwrap();
    assert!(
        summary.get("judge_errors").is_none_or(|v| v.is_null()),
        "summary: {}",
        summary
    );
}

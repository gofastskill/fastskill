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
        "id,prompt,should_trigger,tags,workspace_subdir\ntrial-case,\"test prompt\",true,\"basic\",\n",
    )
    .unwrap();
    fs::write(dir.path().join("SKILL.md"), "# Test Skill\n").unwrap();
    fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nid = \"test-skill\"\n\n[tool.fastskill.eval]\nprompts = \"evals/prompts.csv\"\ntimeout_seconds = 30\nfail_on_missing_agent = true\n",
    )
    .unwrap();

    // Fake agent (see `install_fake_agent`) that passes the first 3
    // invocations, then fails -- driven by `FAKE_AGENT_MODE=counter`.
    let bin_dir = dir.path().join("bin");
    let output_dir = dir.path().join("out");
    let state_dir = dir.path().join("state");
    let merged_path = install_fake_agent(&bin_dir, "codex");
    let env_vars = vec![
        ("PATH", merged_path.as_str()),
        ("FASTSKILL_TEST_STATE_DIR", state_dir.to_str().unwrap()),
        ("FAKE_AGENT_MODE", "counter"),
        ("FAKE_AGENT_PASS_LIMIT", "3"),
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
        "id,prompt,should_trigger,tags,workspace_subdir\nsleep-case,\"test prompt\",true,\"basic\",\n",
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
        "id,prompt,should_trigger,tags,workspace_subdir\nall-case,\"test prompt\",true,\"basic\",\n",
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
        "id,prompt,should_trigger,tags,workspace_subdir\nagent-json-case,\"test prompt\",true,\"basic\",\n",
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
        cases: vec![CaseSummary {
            id: "token-case".to_string(),
            status: CaseStatus::Passed,
            command_count: Some(1),
            input_tokens: Some(1234),
            output_tokens: Some(567),
            pass_count: Some(1),
            total_trials: Some(1),
            pass_rate: Some(1.0),
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

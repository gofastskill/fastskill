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
            error_count: Some(0),
            scored_trials: Some(1),
            should_trigger: None,
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

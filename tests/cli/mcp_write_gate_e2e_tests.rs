//! E2E tests for the `fastskill mcp serve` write gate (ADR-0003 / WRITE-GATE).
//!
//! These drive a real `fastskill mcp serve --transport stdio` child process over
//! newline-delimited JSON-RPC, so they exercise the same surface an MCP host
//! sees: `tools/list` for discovery and `tools/call` for dispatch.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use super::snapshot_helpers::get_binary_path;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;

/// Tools that mutate state and must be gated behind `--enable-write`.
///
/// Kept as a literal list on purpose: the production set is derived from
/// `fastskill_core::write_ops`, and a test that derived it the same way could
/// not catch the set being emptied.
const MUTATING_TOOLS: &[&str] = &[
    "fastskill_init",
    "fastskill_install",
    "fastskill_add",
    "fastskill_update",
    "fastskill_remove",
    "fastskill_reindex",
    "fastskill_repos_add",
    "fastskill_repos_remove",
    "fastskill_repos_update",
    "fastskill_repos_refresh",
    "fastskill_marketplace_create",
    "fastskill_optimize_run",
];

/// A read-only tool that must stay exported with the gate closed. Without this
/// assertion, "no mutating tool is listed" would also pass for an empty list.
const READ_ONLY_TOOL: &str = "fastskill_list";

/// Build a minimal initialised project containing one installed skill.
fn project_with_skill(skill: &str) -> TempDir {
    let temp = TempDir::new().unwrap();
    let skills_dir = temp.path().join(".claude").join("skills").join(skill);
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        skills_dir.join("SKILL.md"),
        format!(
            "---\nname: {}\ndescription: Fixture skill for write-gate tests\nversion: 1.0.0\n---\n# {}\n",
            skill, skill
        ),
    )
    .unwrap();
    fs::write(
        temp.path().join("skill-project.toml"),
        "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();
    temp
}

fn skill_dir(project: &Path, skill: &str) -> std::path::PathBuf {
    project.join(".claude").join("skills").join(skill)
}

/// Run one `mcp serve --transport stdio` session: perform the MCP handshake,
/// send `requests`, close stdin, and return every JSON-RPC message the server
/// wrote to stdout.
fn mcp_stdio_session(project: &Path, extra_args: &[&str], requests: &[Value]) -> Vec<Value> {
    let mut cmd = Command::new(get_binary_path());
    cmd.args(["mcp", "serve", "--transport", "stdio"])
        .args(extra_args)
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("failed to spawn `fastskill mcp serve`");

    let mut payload = String::new();
    payload.push_str(&format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "fastskill-write-gate-tests", "version": "0.0.0"}
            }
        })
    ));
    payload.push_str(&format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    ));
    for request in requests {
        payload.push_str(&format!("{}\n", request));
    }

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(payload.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }
    // EOF on stdin is how the stdio transport is asked to shut down.
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("mcp serve did not exit");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let messages: Vec<Value> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    assert!(
        !messages.is_empty(),
        "mcp serve produced no JSON-RPC output.\nstdout:\n{}\nstderr:\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    messages
}

fn response_for(messages: &[Value], id: i64) -> Value {
    messages
        .iter()
        .find(|m| m.get("id").and_then(Value::as_i64) == Some(id))
        .unwrap_or_else(|| panic!("no JSON-RPC response with id {} in {:?}", id, messages))
        .clone()
}

fn tool_names(messages: &[Value], id: i64) -> Vec<String> {
    let response = response_for(messages, id);
    response
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("tools/list response had no result.tools: {}", response))
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn list_tools_request(id: i64) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {}})
}

fn remove_skill_request(id: i64, skill: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "fastskill_remove",
            "arguments": {"skill-ids": [skill], "force": true, "no-reindex": true}
        }
    })
}

#[test]
fn mcp_tools_list_hides_mutating_tools_without_enable_write() {
    let project = project_with_skill("hello-skill");
    let messages = mcp_stdio_session(project.path(), &[], &[list_tools_request(1)]);
    let names = tool_names(&messages, 1);

    assert!(
        names.iter().any(|n| n == READ_ONLY_TOOL),
        "expected the read-only tool {} to stay exported, got {:?}",
        READ_ONLY_TOOL,
        names
    );
    for tool in MUTATING_TOOLS {
        assert!(
            !names.iter().any(|n| n == tool),
            "mutating tool {} was listed without --enable-write; tools: {:?}",
            tool,
            names
        );
    }
}

#[test]
fn mcp_tools_call_refuses_mutating_tool_without_enable_write() {
    let project = project_with_skill("hello-skill");
    let installed = skill_dir(project.path(), "hello-skill");
    assert!(installed.is_dir(), "fixture skill was not created");

    let messages = mcp_stdio_session(
        project.path(),
        &[],
        &[remove_skill_request(1, "hello-skill")],
    );
    let response = response_for(&messages, 1);

    let error = response
        .get("error")
        .unwrap_or_else(|| panic!("expected a JSON-RPC error, got {}", response));
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        message.contains("--enable-write"),
        "the refusal must name the write gate, got {}",
        message
    );

    // Do not trust the message alone: the skill must still be on disk.
    assert!(
        installed.is_dir(),
        "fastskill_remove deleted {} despite the write gate being closed",
        installed.display()
    );
}

#[test]
fn mcp_enable_write_lists_and_runs_mutating_tools() {
    let project = project_with_skill("hello-skill");
    let installed = skill_dir(project.path(), "hello-skill");

    let messages = mcp_stdio_session(
        project.path(),
        &["--enable-write"],
        &[
            list_tools_request(1),
            remove_skill_request(2, "hello-skill"),
        ],
    );

    let names = tool_names(&messages, 1);
    assert!(
        names.iter().any(|n| n == "fastskill_remove"),
        "fastskill_remove must be listed with --enable-write; tools: {:?}",
        names
    );

    let response = response_for(&messages, 2);
    assert!(
        response.get("error").is_none(),
        "fastskill_remove must succeed with --enable-write, got {}",
        response
    );
    assert!(
        !installed.exists(),
        "fastskill_remove did not delete {} with --enable-write",
        installed.display()
    );
}

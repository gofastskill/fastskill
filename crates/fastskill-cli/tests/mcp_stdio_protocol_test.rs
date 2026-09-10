#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `fastskill mcp serve --transport stdio` must speak only JSON-RPC on stdout.
//!
//! Regression coverage for the bug where commands wrote to `stdout` with
//! `println!`. Under stdio transport `stdout` *is* the JSON-RPC channel, so the
//! real output was interleaved between protocol frames and the tool result was
//! the literal string `"OK"`:
//!
//! ```text
//! {"jsonrpc":"2.0","id":1,"result":{...}}
//! ID          Name        Description        <- raw table, corrupts the stream
//! {"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"OK"}]}}
//! ```

use std::io::Write;
use std::process::{Command, Stdio};

/// Drive `mcp serve --transport stdio` with `requests` and return stdout.
fn mcp_session(project: &std::path::Path, requests: &[&str]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(["mcp", "serve", "--transport", "stdio"])
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fastskill mcp serve");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        for line in requests {
            writeln!(stdin, "{}", line).expect("write request");
        }
    }

    let out = child.wait_with_output().expect("wait for mcp server");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// A project with one installed skill, so `list` has something to print.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("skill-project.toml"),
        "[metadata]\nname = \"mcp-protocol-test\"\nversion = \"0.1.0\"\n\
         [tool.fastskill]\nskills_directory = \".claude/skills\"\n[dependencies]\n",
    )
    .expect("write manifest");

    let skill = dir.path().join(".claude/skills/demo-skill");
    std::fs::create_dir_all(&skill).expect("create skill dir");
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: demo-skill\ndescription: A demo skill for MCP protocol testing\n---\n# Demo\n",
    )
    .expect("write SKILL.md");
    dir
}

fn response(stdout: &str, id: i64) -> serde_json::Value {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v: &serde_json::Value| v.get("id").and_then(serde_json::Value::as_i64) == Some(id))
        .unwrap_or_else(|| panic!("no response with id {id} in:\n{stdout}"))
}

#[test]
fn stdout_carries_only_json_rpc_frames() {
    let project = fixture();
    let stdout = mcp_session(
        project.path(),
        &[
            INIT,
            INITIALIZED,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fastskill_skill_list","arguments":{}}}"#,
        ],
    );

    let leaked: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| serde_json::from_str::<serde_json::Value>(l).is_err())
        .collect();

    assert!(
        leaked.is_empty(),
        "command output leaked onto the JSON-RPC stream: {leaked:#?}\nfull stdout:\n{stdout}"
    );
}

#[test]
fn tool_result_carries_the_command_output_not_ok() {
    let project = fixture();
    let stdout = mcp_session(
        project.path(),
        &[
            INIT,
            INITIALIZED,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fastskill_skill_list","arguments":{}}}"#,
        ],
    );

    let text = response(&stdout, 2)["result"]["content"][0]["text"]
        .as_str()
        .expect("text content")
        .to_string();

    assert_ne!(
        text.trim(),
        "OK",
        "tool returned the placeholder, not output"
    );
    assert!(
        text.contains("demo-skill"),
        "tool result should carry the rendered skill list, got:\n{text}"
    );
}

#[test]
fn long_running_serve_is_not_exported_as_a_tool() {
    // `serve` starts an HTTP server and never returns; exporting it as a tool
    // means any caller invoking it hangs until the transport times out.
    let project = fixture();
    let stdout = mcp_session(
        project.path(),
        &[
            INIT,
            INITIALIZED,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        ],
    );

    let listed = response(&stdout, 2);
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect::<Vec<_>>();

    assert!(
        !tools.contains(&"fastskill_server_serve".to_string()),
        "serve must not be an MCP tool; exported: {tools:#?}"
    );
    // Sanity: the export policy did not filter everything away.
    assert!(tools.contains(&"fastskill_skill_list".to_string()));
    assert!(!tools.contains(&"fastskill_list".to_string()));
}

#[test]
fn retired_tool_names_are_not_callable_and_cannot_mutate_project_state() {
    const RETIRED_TOOL_NAMES: &[&str] = &[
        "fastskill_init",
        "fastskill_install",
        "fastskill_add",
        "fastskill_update",
        "fastskill_remove",
        "fastskill_reindex",
        "fastskill_list",
        "fastskill_read",
        "fastskill_search",
        "fastskill_completion",
        "fastskill_doctor",
        "fastskill_spec",
        "fastskill_repos_add",
        "fastskill_repos_info",
        "fastskill_repos_list",
        "fastskill_repos_refresh",
        "fastskill_repos_remove",
        "fastskill_repos_show",
        "fastskill_repos_skills",
        "fastskill_repos_test",
        "fastskill_repos_update",
        "fastskill_repos_versions",
        "fastskill_analyze_cluster",
        "fastskill_analyze_duplicates",
        "fastskill_analyze_matrix",
        "fastskill_optimize_export",
        "fastskill_optimize_inspect",
        "fastskill_optimize_resume",
        "fastskill_optimize_run",
        "fastskill_optimize_status",
    ];

    let project = fixture();
    let sentinel = project.path().join("sentinel");
    std::fs::write(&sentinel, "unchanged").expect("write sentinel");

    let mut requests = vec![INIT.to_string(), INITIALIZED.to_string()];
    for (offset, name) in RETIRED_TOOL_NAMES.iter().enumerate() {
        requests.push(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 10 + offset,
                "method": "tools/call",
                "params": {"name": name, "arguments": {}},
            })
            .to_string(),
        );
    }
    let request_refs = requests.iter().map(String::as_str).collect::<Vec<_>>();
    let stdout = mcp_session(project.path(), &request_refs);

    for (offset, name) in RETIRED_TOOL_NAMES.iter().enumerate() {
        let rejected = response(&stdout, 10 + offset as i64);
        assert!(
            rejected.get("error").is_some(),
            "retired MCP tool unexpectedly dispatched: {name}\n{rejected}"
        );
        assert_eq!(rejected["error"]["code"], -32001, "{name}: {rejected}");
        let message = rejected["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("MCP_CMD_NOT_FOUND") && message.contains(name),
            "retired MCP tool did not fail as unknown: {name}\n{rejected}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&sentinel).expect("read sentinel"),
        "unchanged"
    );
}

#[test]
fn stdio_transport_rejects_http_only_flags() {
    // cli-framework 0.5.8 (src/mcp/commands.rs) rejects `--host`/`--port`/`--path`
    // overrides when `--transport=stdio` with a `[E004]` error instead of silently
    // ignoring them, since stdio has no host/port/path to bind.
    let project = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(["mcp", "serve", "--transport", "stdio", "--host", "0.0.0.0"])
        .current_dir(project.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run fastskill mcp serve");

    assert!(
        !output.status.success(),
        "expected non-zero exit, got status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "[E004] invalid usage: '--host', '--port', and '--path' are only valid when --transport=http"
        ),
        "expected E004 invalid-usage error on stderr, got:\n{stderr}"
    );
}

#[test]
fn tool_schemas_document_their_parameters() {
    // cli-framework 0.5.8 forwards `ArgSpec.help` into the JSON-Schema
    // `description`. Without it every property is a bare `{"type":"boolean"}`
    // and a calling agent has to guess what the flags do.
    let project = fixture();
    let stdout = mcp_session(
        project.path(),
        &[
            INIT,
            INITIALIZED,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        ],
    );

    let tools = response(&stdout, 2)["result"]["tools"].clone();
    let list = tools
        .as_array()
        .expect("tools array")
        .iter()
        .find(|t| t["name"] == "fastskill_skill_list")
        .expect("fastskill_skill_list tool");

    let props = list["inputSchema"]["properties"]
        .as_object()
        .expect("schema properties");
    assert!(
        !props.is_empty(),
        "fastskill_skill_list should expose parameters"
    );

    let undocumented: Vec<&String> = props
        .iter()
        .filter(|(_, schema)| schema.get("description").is_none())
        .map(|(name, _)| name)
        .collect();
    assert!(
        undocumented.is_empty(),
        "these parameters have no description: {undocumented:?}"
    );
}

#[test]
fn read_only_server_hides_and_denies_optimize_export_before_writing() {
    let project = fixture();
    let run_dir = project.path().join("run");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    std::fs::write(run_dir.join("best_skill.md"), "replacement").expect("write candidate");
    let destination = project.path().join("sentinel.md");
    std::fs::write(&destination, "original").expect("write sentinel");

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "fastskill_optimization_export",
            "arguments": {
                "run-dir": run_dir,
                "out": destination,
            }
        }
    })
    .to_string();
    let stdout = mcp_session(
        project.path(),
        &[
            INIT,
            INITIALIZED,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            &call,
        ],
    );

    let listed = response(&stdout, 2);
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !tools.contains(&"fastskill_optimization_export"),
        "artifact writer must be hidden from a read-only server"
    );

    let denied = response(&stdout, 3);
    assert_eq!(denied["error"]["code"], -32005);
    assert!(denied["error"]["message"]
        .as_str()
        .expect("denial message")
        .contains("--enable-write"));
    assert_eq!(
        std::fs::read_to_string(project.path().join("sentinel.md")).expect("read sentinel"),
        "original",
        "denied call must not dispatch the artifact writer"
    );
}

#[test]
fn every_mcp_capable_command_has_an_explicit_effect() {
    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(["cli", "spec", "--format", "json"])
        .output()
        .expect("run command spec");
    assert!(output.status.success());
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse command spec");

    let mut unclassified = Vec::new();
    for command in document["commands"].as_array().expect("commands array") {
        let path = command["path"].as_str().expect("command path");
        // Long-running servers are intentionally registered with no MCP export.
        if matches!(path, "server/serve" | "mcp/serve") {
            continue;
        }
        let segments = path.split('/').collect::<Vec<_>>();
        if fastskill_core::write_ops::command_effect(&segments).is_none() {
            unclassified.push(path.to_string());
        }
    }

    assert!(
        unclassified.is_empty(),
        "commands must be classified before MCP exposure: {unclassified:?}"
    );
}

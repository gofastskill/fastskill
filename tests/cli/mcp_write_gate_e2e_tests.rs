//! E2E tests for the `fastskill mcp serve` write gate (ADR-0003 / WRITE-GATE).
//!
//! These drive a real `fastskill mcp serve --transport stdio` child process over
//! newline-delimited JSON-RPC, so they exercise the same surface an MCP host
//! sees: `tools/list` for discovery and `tools/call` for dispatch.

#![allow(clippy::all, clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::snapshot_helpers::get_binary_path;
use fastskill_core::core::lock::{ProjectLockedSkillEntry, ProjectSkillsLock};
use fastskill_core::core::origin::{Origin, Resolved};
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
    "fastskill_project_init",
    "fastskill_project_install",
    "fastskill_skill_add",
    "fastskill_skill_update",
    "fastskill_skill_remove",
    "fastskill_index_rebuild",
    "fastskill_repo_add",
    "fastskill_repo_remove",
    "fastskill_repo_update",
    "fastskill_repo_refresh",
    "fastskill_cache_clean",
    "fastskill_marketplace_create",
    "fastskill_bundle_build",
    "fastskill_bundle_add",
    "fastskill_bundle_update",
    "fastskill_bundle_remove",
    "fastskill_bundle_override",
    "fastskill_mcp_install",
    "fastskill_eval_run",
    "fastskill_eval_judge",
    "fastskill_eval_scorecard",
    "fastskill_optimization_run",
    "fastskill_optimization_resume",
    "fastskill_optimization_export",
];

/// A read-only tool that must stay exported with the gate closed. Without this
/// assertion, "no mutating tool is listed" would also pass for an empty list.
const READ_ONLY_TOOL: &str = "fastskill_skill_list";

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
        format!(
            "[dependencies]\n{skill} = {{ origin = {{ type = \"local\", path = \".claude/skills/{skill}\" }} }}\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n"
        ),
    )
    .unwrap();
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = vec![skill.to_string()];
    lock.skills = vec![ProjectLockedSkillEntry {
        id: skill.to_string(),
        name: skill.to_string(),
        origin: Origin::Local {
            path: format!(".claude/skills/{skill}").into(),
            editable: false,
        },
        resolved: Resolved {
            version: "1.0.0".to_string(),
            commit_hash: None,
            checksum: Some(
                fastskill_core::core::project_removal::managed_tree_digest(&skills_dir).unwrap(),
            ),
        },
        dependencies: Vec::new(),
        groups: Vec::new(),
        depth: 0,
        parent_skill: None,
        required_by: Vec::new(),
    }];
    lock.save_to_file(&temp.path().join("skills.lock")).unwrap();
    temp
}

/// Build a project where `child` is owned only as a dependency of `root`.
fn project_with_transitive_skill() -> TempDir {
    let temp = TempDir::new().unwrap();
    let skills = temp.path().join(".claude/skills");
    for skill in ["root", "child"] {
        let directory = skills.join(skill);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            format!(
                "---\nname: {skill}\ndescription: MCP ownership fixture\nversion: 1.0.0\n---\n# {skill}\n"
            ),
        )
        .unwrap();
    }
    fs::write(
        temp.path().join("skill-project.toml"),
        "[dependencies]\nroot = { origin = { type = \"local\", path = \".claude/skills/root\" } }\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
    )
    .unwrap();
    let entry = |skill: &str, dependencies: Vec<String>, depth, parent_skill, required_by| {
        ProjectLockedSkillEntry {
            id: skill.to_string(),
            name: skill.to_string(),
            origin: Origin::Local {
                path: format!(".claude/skills/{skill}").into(),
                editable: false,
            },
            resolved: Resolved {
                version: "1.0.0".to_string(),
                commit_hash: None,
                checksum: Some(
                    fastskill_core::core::project_removal::managed_tree_digest(&skills.join(skill))
                        .unwrap(),
                ),
            },
            dependencies,
            groups: Vec::new(),
            depth,
            parent_skill,
            required_by,
        }
    };
    let mut lock = ProjectSkillsLock::new_empty();
    lock.covered_roots = vec!["root".to_string()];
    lock.skills = vec![
        entry("root", vec!["child".to_string()], 0, None, Vec::new()),
        entry(
            "child",
            Vec::new(),
            1,
            Some("root".to_string()),
            vec!["root".to_string()],
        ),
    ];
    lock.save_to_file(&temp.path().join("skills.lock")).unwrap();
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
            "name": "fastskill_skill_remove",
            "arguments": {"skill-ids": [skill], "force": true, "no-reindex": true}
        }
    })
}

fn unconfirmed_remove_skill_request(id: i64, skill: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "fastskill_skill_remove",
            "arguments": {"skill-ids": [skill], "no-reindex": true}
        }
    })
}

fn execute_tool_request(id: i64, name: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": {}}
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
        &[
            remove_skill_request(1, "hello-skill"),
            execute_tool_request(2, "fastskill_eval_run"),
        ],
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
        "fastskill_skill_remove deleted {} despite the write gate being closed",
        installed.display()
    );

    let execution = response_for(&messages, 2);
    assert!(
        execution["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("--enable-write"),
        "execution tools must be refused by the write gate, got {execution}"
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
        names.iter().any(|n| n == "fastskill_skill_remove"),
        "fastskill_skill_remove must be listed with --enable-write; tools: {:?}",
        names
    );

    let response = response_for(&messages, 2);
    assert!(
        response.get("error").is_none(),
        "fastskill_skill_remove must succeed with --enable-write, got {}",
        response
    );
    assert!(
        !installed.exists(),
        "fastskill_skill_remove did not delete {} with --enable-write",
        installed.display()
    );
}

#[test]
fn mcp_remove_without_force_fails_noninteractively_and_preserves_state() {
    let project = project_with_skill("hello-skill");
    let installed = skill_dir(project.path(), "hello-skill");
    let manifest = fs::read(project.path().join("skill-project.toml")).unwrap();
    let lock = fs::read(project.path().join("skills.lock")).unwrap();
    let skill = fs::read(installed.join("SKILL.md")).unwrap();

    let messages = mcp_stdio_session(
        project.path(),
        &["--enable-write"],
        &[unconfirmed_remove_skill_request(1, "hello-skill")],
    );
    let response = response_for(&messages, 1);
    assert!(
        response.to_string().contains("requires --force"),
        "MCP removal must return a noninteractive error, got {response}"
    );
    assert_eq!(
        fs::read(project.path().join("skill-project.toml")).unwrap(),
        manifest
    );
    assert_eq!(fs::read(project.path().join("skills.lock")).unwrap(), lock);
    assert_eq!(fs::read(installed.join("SKILL.md")).unwrap(), skill);
}

#[test]
fn mcp_enable_write_still_refuses_removal_owned_by_a_retained_root() {
    let project = project_with_transitive_skill();
    let manifest = fs::read(project.path().join("skill-project.toml")).unwrap();
    let lock = fs::read(project.path().join("skills.lock")).unwrap();
    let root = fs::read(skill_dir(project.path(), "root").join("SKILL.md")).unwrap();
    let child = fs::read(skill_dir(project.path(), "child").join("SKILL.md")).unwrap();

    let messages = mcp_stdio_session(
        project.path(),
        &["--enable-write"],
        &[remove_skill_request(1, "child")],
    );
    let response = response_for(&messages, 1);
    let rendered = response.to_string();

    assert!(
        response.pointer("/result/isError") == Some(&Value::Bool(true))
            || response.get("error").is_some(),
        "required dependency removal must fail, got {response}"
    );
    assert!(
        rendered.contains("root") || rendered.contains("required"),
        "refusal must explain the retaining owner, got {response}"
    );
    assert_eq!(
        fs::read(project.path().join("skill-project.toml")).unwrap(),
        manifest
    );
    assert_eq!(fs::read(project.path().join("skills.lock")).unwrap(), lock);
    assert_eq!(
        fs::read(skill_dir(project.path(), "root").join("SKILL.md")).unwrap(),
        root
    );
    assert_eq!(
        fs::read(skill_dir(project.path(), "child").join("SKILL.md")).unwrap(),
        child
    );
}

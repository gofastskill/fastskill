//! E2E tests for mcp install, mcp register, and mcp list subcommands.
//!
//! These tests execute the CLI binary and verify actual behavior.

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::snapshot_helpers::run_fastskill_command;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_mcp_help_lists_install_register_list_serve() {
    let result = run_fastskill_command(&["mcp", "--help"], None);
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("install"),
        "mcp --help should list 'install'"
    );
    assert!(
        combined.contains("register"),
        "mcp --help should list 'register'"
    );
    assert!(combined.contains("list"), "mcp --help should list 'list'");
    assert!(combined.contains("serve"), "mcp --help should list 'serve'");
}

#[test]
fn test_mcp_install_dry_run_exits_zero_and_prints_output() {
    let result = run_fastskill_command(
        &[
            "mcp",
            "install",
            "--agent",
            "cursor",
            "--stdio",
            "--dry-run",
        ],
        None,
    );
    assert!(
        result.success,
        "dry-run should exit 0; stderr: {}",
        result.stderr
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        !combined.is_empty(),
        "dry-run should produce non-empty output"
    );
}

#[test]
fn test_mcp_install_cursor_writes_config_file() {
    let temp_dir = TempDir::new().unwrap();
    let result = run_fastskill_command(
        &[
            "mcp",
            "install",
            "--agent",
            "cursor",
            "--stdio",
            "--scope",
            "project",
            "--overwrite",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        result.success,
        "mcp install should exit 0; stderr: {}",
        result.stderr
    );
    let config_path = temp_dir.path().join(".cursor").join("mcp.json");
    assert!(config_path.exists(), ".cursor/mcp.json should be created");
    let contents = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
    // Entries live under the "mcpServers" bucket, not at the config root
    // (see aikit_sdk::mcp_deploy::merge_json_bucket).
    assert!(
        json["mcpServers"].get("fastskill").is_some(),
        "mcp.json's 'mcpServers' should contain 'fastskill' key; got: {json}"
    );
    let args = &json["mcpServers"]["fastskill"]["args"];
    let args_str = args.to_string();
    assert!(
        args_str.contains("mcp") && args_str.contains("serve") && args_str.contains("stdio"),
        "args should include mcp serve --transport stdio; got: {args_str}"
    );
}

#[test]
fn test_mcp_install_duplicate_without_overwrite_exits_nonzero() {
    let temp_dir = TempDir::new().unwrap();
    // First install
    let first = run_fastskill_command(
        &[
            "mcp",
            "install",
            "--agent",
            "cursor",
            "--stdio",
            "--scope",
            "project",
            "--overwrite",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        first.success,
        "first install should succeed; stderr: {}",
        first.stderr
    );

    // Second install without --overwrite should fail
    let second = run_fastskill_command(
        &[
            "mcp", "install", "--agent", "cursor", "--stdio", "--scope", "project",
        ],
        Some(temp_dir.path()),
    );
    assert!(
        !second.success,
        "second install without --overwrite should exit non-zero"
    );
    let combined = format!("{}{}", second.stdout, second.stderr);
    assert!(
        combined.to_lowercase().contains("already") || combined.to_lowercase().contains("exist"),
        "error should mention entry already exists; got: {combined}"
    );
}

#[test]
fn test_mcp_list_exits_zero() {
    let result = run_fastskill_command(&["mcp", "list"], None);
    assert!(
        result.success,
        "mcp list should exit 0; stderr: {}",
        result.stderr
    );
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(!combined.is_empty(), "mcp list should produce output");
}

// ---------------------------------------------------------------------------
// Coverage for all 6 `mcp install` agent targets (not just `claude`/`cursor`).
//
// Source of truth verified against aikit_sdk::mcp_deploy (goaikit/aikit,
// crate `aikit-sdk`, pinned rev in this workspace's Cargo.lock):
//   - claude       -> project_root/.mcp.json               top-level "mcpServers"
//   - cursor-agent -> project_root/.cursor/mcp.json         top-level "mcpServers"
//   - gemini       -> project_root/.gemini/settings.json    top-level "mcpServers"
//   - copilot      -> project_root/.vscode/mcp.json         top-level "servers" (VS Code shape)
//   - opencode     -> project_root/opencode.json            top-level "mcp"
//   - codex        -> project_root/.codex/config.toml       TOML "[mcp_servers.NAME]"
// `--agent cursor` is a documented alias that normalizes to "cursor-agent"
// (aikit_sdk::normalize_mcp_agent_key).
// ---------------------------------------------------------------------------

/// One row per supported `mcp install --agent` target: the flag value, the
/// project-scope config path it must write, and the top-level key holding
/// the server map (`None` for the codex TOML target, which nests servers
/// under `[mcp_servers.NAME]` instead of a JSON bucket).
struct AgentTarget {
    agent_flag: &'static str,
    config_relpath: &'static str,
    json_bucket_key: Option<&'static str>,
}

const AGENT_TARGETS: &[AgentTarget] = &[
    AgentTarget {
        agent_flag: "claude",
        config_relpath: ".mcp.json",
        json_bucket_key: Some("mcpServers"),
    },
    AgentTarget {
        agent_flag: "cursor-agent",
        config_relpath: ".cursor/mcp.json",
        json_bucket_key: Some("mcpServers"),
    },
    AgentTarget {
        agent_flag: "gemini",
        config_relpath: ".gemini/settings.json",
        json_bucket_key: Some("mcpServers"),
    },
    AgentTarget {
        agent_flag: "copilot",
        config_relpath: ".vscode/mcp.json",
        json_bucket_key: Some("servers"),
    },
    AgentTarget {
        agent_flag: "opencode",
        config_relpath: "opencode.json",
        json_bucket_key: Some("mcp"),
    },
    AgentTarget {
        agent_flag: "codex",
        config_relpath: ".codex/config.toml",
        json_bucket_key: None,
    },
];

/// Extracts the stdio invocation tokens (command/args) from a JSON server
/// entry as a single string for substring assertions. `opencode` is the one
/// JSON target that folds the exe path and argv into a single "command"
/// array instead of separate "command"/"args" fields.
fn json_entry_invocation_repr(agent_flag: &str, entry: &serde_json::Value) -> String {
    if agent_flag == "opencode" {
        entry["command"].to_string()
    } else {
        entry["args"].to_string()
    }
}

#[test]
fn test_mcp_install_writes_expected_config_for_every_agent_target() {
    for target in AGENT_TARGETS {
        let temp_dir = TempDir::new().unwrap();
        let result = run_fastskill_command(
            &[
                "mcp",
                "install",
                "--agent",
                target.agent_flag,
                "--stdio",
                "--scope",
                "project",
                "--overwrite",
            ],
            Some(temp_dir.path()),
        );
        assert!(
            result.success,
            "mcp install --agent {} should exit 0; stderr: {}",
            target.agent_flag, result.stderr
        );

        let config_path = temp_dir.path().join(target.config_relpath);
        assert!(
            config_path.exists(),
            "--agent {} should create config at {}",
            target.agent_flag,
            target.config_relpath
        );
        let contents = fs::read_to_string(&config_path).unwrap();

        match target.json_bucket_key {
            Some(bucket_key) => {
                let json: serde_json::Value = serde_json::from_str(&contents).unwrap_or_else(|e| {
                    panic!(
                        "--agent {} config should be valid JSON: {e}",
                        target.agent_flag
                    )
                });
                let bucket = json.get(bucket_key).unwrap_or_else(|| {
                    panic!(
                        "--agent {} config should have top-level '{bucket_key}' key; got: {json}",
                        target.agent_flag
                    )
                });
                let entry = bucket.get("fastskill").unwrap_or_else(|| {
                    panic!(
                        "--agent {} config['{bucket_key}'] should contain a 'fastskill' entry; got: {bucket}",
                        target.agent_flag
                    )
                });
                let invocation = json_entry_invocation_repr(target.agent_flag, entry);
                assert!(
                    invocation.contains("mcp") && invocation.contains("serve") && invocation.contains("stdio"),
                    "--agent {} fastskill entry should invoke mcp serve --transport stdio; got: {invocation}",
                    target.agent_flag
                );
            }
            None => {
                // codex: TOML, servers nested under [mcp_servers.NAME].
                let root: toml::Value = toml::from_str(&contents).unwrap_or_else(|e| {
                    panic!(
                        "--agent {} config should be valid TOML: {e}",
                        target.agent_flag
                    )
                });
                let entry = root
                    .get("mcp_servers")
                    .and_then(|servers| servers.get("fastskill"))
                    .unwrap_or_else(|| {
                        panic!(
                            "--agent {} config should have [mcp_servers.fastskill]; got: {root}",
                            target.agent_flag
                        )
                    });
                let args = entry.get("args").map(|v| v.to_string()).unwrap_or_default();
                assert!(
                    args.contains("mcp") && args.contains("serve") && args.contains("stdio"),
                    "--agent {} fastskill entry args should invoke mcp serve --transport stdio; got: {args}",
                    target.agent_flag
                );
            }
        }
    }
}

#[test]
fn test_mcp_install_cursor_alias_matches_cursor_agent() {
    let alias_dir = TempDir::new().unwrap();
    let canonical_dir = TempDir::new().unwrap();

    let alias_result = run_fastskill_command(
        &[
            "mcp",
            "install",
            "--agent",
            "cursor",
            "--stdio",
            "--scope",
            "project",
            "--overwrite",
        ],
        Some(alias_dir.path()),
    );
    assert!(
        alias_result.success,
        "--agent cursor should exit 0; stderr: {}",
        alias_result.stderr
    );

    let canonical_result = run_fastskill_command(
        &[
            "mcp",
            "install",
            "--agent",
            "cursor-agent",
            "--stdio",
            "--scope",
            "project",
            "--overwrite",
        ],
        Some(canonical_dir.path()),
    );
    assert!(
        canonical_result.success,
        "--agent cursor-agent should exit 0; stderr: {}",
        canonical_result.stderr
    );

    let alias_path = alias_dir.path().join(".cursor").join("mcp.json");
    let canonical_path = canonical_dir.path().join(".cursor").join("mcp.json");
    assert!(
        alias_path.exists(),
        "--agent cursor should write to .cursor/mcp.json (cursor-agent's path)"
    );
    assert!(
        canonical_path.exists(),
        "--agent cursor-agent should write to .cursor/mcp.json"
    );

    let alias_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&alias_path).unwrap()).unwrap();
    let canonical_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&canonical_path).unwrap()).unwrap();
    assert_eq!(
        alias_json, canonical_json,
        "--agent cursor and --agent cursor-agent should produce identical config content"
    );
}

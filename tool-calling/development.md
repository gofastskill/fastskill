# MCP Tool Calling

FastSkill 0.9.229

Source: https://docs.gofastskill.com/tool-calling/development

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



## Overview

FastSkill's tool/integration surface for agents is **MCP** (Model Context Protocol). You do not
author custom tools inside FastSkill — instead, `fastskill mcp serve` exposes FastSkill CLI commands
to an agent as MCP tools, and `fastskill mcp install` writes the server config into your agent's
configuration.

The server is **read-only by default**: the mutating commands are not exported as tools unless you
start it with `--enable-write`. See [Write access](#write-access) below.

## Serve FastSkill as an MCP server

```bash
fastskill mcp serve
```

Each exported CLI command is registered as an MCP tool named `fastskill_<path>`, where the command
path has its separators replaced by underscores. For example:

| CLI command                 | MCP tool name               |
| --------------------------- | --------------------------- |
| `fastskill skill search`    | `fastskill_skill_search`    |
| `fastskill repo add`        | `fastskill_repo_add`        |
| `fastskill project install` | `fastskill_project_install` |
| `fastskill skill list`      | `fastskill_skill_list`      |

The `mcp` commands themselves, `server serve`, and the built-in `cli completion` and `cli spec` commands are not
exported as tools.

### Write access

`fastskill mcp serve` is read-only unless you pass `--enable-write`, spelled exactly as
`fastskill server serve`'s flag because it means the same thing. Both gates read one table —
`fastskill_core::write_ops::WRITE_OPERATIONS` — so a mutating command cannot be gated on one surface
and open on the other.

Without the flag, these tools are absent from `tools/list`, and a `tools/call` naming one is refused
with JSON-RPC error `-32005 MCP_TOOL_DENIED` quoting the flag, before the command is dispatched:

| Blocked tool                    | CLI command                     |
| ------------------------------- | ------------------------------- |
| `fastskill_project_init`        | `fastskill project init`        |
| `fastskill_skill_add`           | `fastskill skill add`           |
| `fastskill_project_install`     | `fastskill project install`     |
| `fastskill_skill_update`        | `fastskill skill update`        |
| `fastskill_skill_remove`        | `fastskill skill remove`        |
| `fastskill_bundle_add`          | `fastskill bundle add`          |
| `fastskill_bundle_update`       | `fastskill bundle update`       |
| `fastskill_bundle_remove`       | `fastskill bundle remove`       |
| `fastskill_index_rebuild`       | `fastskill index rebuild`       |
| `fastskill_repo_add`            | `fastskill repo add`            |
| `fastskill_repo_remove`         | `fastskill repo remove`         |
| `fastskill_repo_update`         | `fastskill repo update`         |
| `fastskill_repo_refresh`        | `fastskill repo refresh`        |
| `fastskill_cache_clean`         | `fastskill cache clean`         |
| `fastskill_marketplace_create`  | `fastskill marketplace create`  |
| `fastskill_optimization_run`    | `fastskill optimization run`    |
| `fastskill_optimization_resume` | `fastskill optimization resume` |

```bash
# Read-only: 23 tools, none of them mutating
fastskill mcp serve --transport stdio

# Allow the agent to mutate the project
fastskill mcp serve --transport stdio --enable-write
```

### Transports

| Option                      | Description                                    | Default     |
| --------------------------- | ---------------------------------------------- | ----------- |
| `--transport <http\|stdio>` | Streamable HTTP or stdin/stdout JSON-RPC       | `http`      |
| `--host <HOST>`             | Bind address (http only)                       | `127.0.0.1` |
| `--port <PORT>`             | Bind port (http only)                          | `8080`      |
| `--path <PATH>`             | HTTP path prefix for MCP endpoints (http only) | `/mcp`      |

```bash
# HTTP transport on a custom path
fastskill mcp serve --transport http --host 127.0.0.1 --port 8080 --path /mcp

# stdio transport (for agents that spawn the process directly)
fastskill mcp serve --transport stdio
```

`--host`, `--port`, and `--path` are only valid with `--transport http`.

## Install into an agent

`fastskill mcp install` writes an MCP server entry into a supported agent's configuration file:

```bash
fastskill mcp install --agent claude --scope project
```

| Option            | Description                                                                |
| ----------------- | -------------------------------------------------------------------------- |
| `--agent <AGENT>` | Target agent: `claude`, `cursor`, `gemini`, `copilot`, `opencode`, `codex` |
| `--scope <SCOPE>` | `project` (default) or `global`                                            |
| `--name <NAME>`   | Server name in the config (defaults to the app name)                       |
| `--url <URL>`     | HTTP MCP URL (defaults to `http://127.0.0.1:8080/mcp`)                     |
| `--stdio`         | Use stdio transport (registers `current_exe` as the command)               |

`--url` and `--stdio` are mutually exclusive. Use `--url` to point the agent at a running HTTP MCP
server, or `--stdio` to have the agent launch FastSkill itself.

```bash
# Point Cursor at a running HTTP MCP server, globally
fastskill mcp install --agent cursor --scope global --url http://127.0.0.1:8080/mcp

# Register a stdio server for Claude Code in this project
fastskill mcp install --agent claude --scope project --stdio
```

## List configured MCP servers

```bash
fastskill mcp list
```

Because MCP tools are generated from the CLI command tree, agents get the FastSkill skill,
project, repository, analysis, evaluation, and optimization operations without per-tool wiring.


## See also

* [`server serve`](/cli-reference/serve-command) — the separate HTTP REST/UI server.
* [Integration tutorials](/examples/integration-tutorials) — end-to-end MCP setup for Claude Code and Cursor.


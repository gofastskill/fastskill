# Integration Tutorials

FastSkill 0.9.230

Source: https://docs.gofastskill.com/examples/integration-tutorials

Release revision: dd983e89e5fee977325b77ae386b879e8310ed26

Documentation revision: dd983e89e5fee977325b77ae386b879e8310ed26



## Overview

These tutorials cover FastSkill's real integration surfaces: the **MCP** server for agents, the
**HTTP API** from `fastskill server serve`, and **locked installs** in CI. Each uses commands and endpoints
that exist in the tool.

## MCP into Claude Code

Expose FastSkill to Claude Code as MCP tools — read-only unless you start the server with
`--enable-write`.

```bash
# Register FastSkill as an MCP server for Claude Code in this project
fastskill mcp install --agent claude --scope project --stdio

# Confirm it's registered
fastskill mcp list
```

Claude Code will now see tools named after complete command paths, such as
`fastskill_skill_search`, `fastskill_skill_list`, `fastskill_skill_read`, and
`fastskill_repo_show`. Mutating tools such as `fastskill_project_install` and
`fastskill_repo_add` are withheld until the server is started with `--enable-write`; see
[Write access](/tool-calling/development#write-access). To point the agent at a running HTTP MCP
server instead of spawning FastSkill itself:

```bash
fastskill mcp serve --transport http --port 8080 --path /mcp --enable-write
fastskill mcp install --agent claude --scope project --url http://127.0.0.1:8080/mcp
```


## MCP into Cursor

Same flow, different agent key:

```bash
fastskill mcp install --agent cursor --scope global --url http://127.0.0.1:8080/mcp
```

Supported agents: `claude`, `cursor`, `gemini`, `copilot`, `opencode`, `codex`. See the full
[MCP tool-calling guide](/tool-calling/development).


## Call the HTTP API from an app

Run the server and use the versioned `/api/v1/...` REST surface. Start read-only:

```bash
fastskill server serve
```

```bash
# List installed skills
curl http://localhost:8080/api/v1/skills

# Search
curl -X POST http://localhost:8080/api/v1/search \
  -H 'Content-Type: application/json' \
  -d '{"query": "convert pdf"}'

# Resolve the most relevant skills for a prompt (context loading)
curl -X POST http://localhost:8080/api/v1/resolve \
  -H 'Content-Type: application/json' \
  -d '{"prompt": "build a slide deck from notes", "limit": 5}'
```

These are all read endpoints and work without `--enable-write`. To manage skills over HTTP (install /
update / delete), start with `fastskill server serve --enable-write`, and front the port with an
authenticating proxy if it is exposed — `server serve` enforces no auth of its own. See the
[`server serve` reference](/cli-reference/serve-command) and [security model](/security/model).


## Reproducible installs in CI

Commit `skill-project.toml` and `skills.lock`, then install exact locked versions in the pipeline:

```bash
# In CI: install exactly what the lockfile records
fastskill project install --lock
```

Set `OPENAI_API_KEY` from a CI secret if the pipeline needs semantic search or reindex; without it
those steps skip silently rather than failing.

For agents, install FastSkill as an MCP server. For services and pipelines, use the `server serve`
HTTP API and `project install --lock`. There is no separate SDK to integrate — the CLI, MCP tools,
and HTTP API are the surface.




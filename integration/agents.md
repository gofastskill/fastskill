# Documentation and tools for agents

FastSkill 0.9.228

Source: https://docs.gofastskill.com/integration/agents

Release revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d

Documentation revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d



## Read the documentation

This site documents one release. The release number appears on every page and in
all Markdown exports. Start with [llms.txt](/llms.txt), follow its per-page Markdown
links, or download [llms-full.txt](/llms-full.txt). These URLs need no authentication
or browser JavaScript. Each page also provides View Markdown and Copy Markdown.

## Inspect the CLI contract

```bash
fastskill -V
fastskill cli spec --format json
fastskill skill add --help
fastskill cli completion bash
```

Match the binary version to the documentation. `cli spec` describes the installed
binary's complete command tree and flags. Use only the flags supported by that
command; `--json` is not a global flag.

## Preview, apply, verify

```bash
fastskill project install --dry-run --json
fastskill project install --json
fastskill skill list --check --json
```

Check the process exit status and the returned diagnostics. A valid JSON response
can describe failure or partial success. Dry runs validate a plan without changing
managed state; online plans can still fetch inputs. Use `--offline` when supported
and when required verified content is already available locally.

## Agent skills and MCP

Installed skills provide task instructions. FastSkill's MCP tools provide package
operations. They are separate interfaces; reading a skill does not require an MCP
server. See [Claude Code](/integration/claude-code-integration) and
[Cursor](/integration/cursor-integration) for installation locations and registration.

```bash
fastskill mcp list
fastskill mcp serve --transport stdio
```

The server exposes read-only tools by default. Mutating tools require
`mcp serve --enable-write`. Tool names use the full command path, for example
`fastskill_skill_read` and `fastskill_project_install`. See
[MCP tool access](/tool-calling/development) for write configuration.


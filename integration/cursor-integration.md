# Cursor Integration

FastSkill 0.9.228

Source: https://docs.gofastskill.com/integration/cursor-integration

Release revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d

Documentation revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d



## Overview

Cursor discovers skills **directly** from the skills directory (`.claude/skills/` by default)
and via the FastSkill MCP server. FastSkill's job is to install the right skills onto disk and
keep them reconciled with your manifest — there is no metadata file to generate or keep in sync.

Modern agents (Cursor, Claude Code, …) read installed skills from the skills directory natively.
Just `fastskill project install` (or `fastskill skill add`) and the skills are available. Register the MCP
server below for richer, tool-based access from inside Cursor.


## Quick start

```bash
# 1. Install the skills declared in skill-project.toml
fastskill project install

# 2. Verify what is on disk
fastskill skill list

# 3. (Optional) Build the local index for semantic search
fastskill index rebuild --skills-dir .claude/skills/
```

## MCP registration

Register `fastskill` as an MCP server inside Cursor with a single command:

```bash
# Register for the current project (stdio transport, recommended)
fastskill mcp install --agent cursor --stdio --scope project --overwrite

# Preview the config change without writing any files
fastskill mcp install --agent cursor --stdio --dry-run
```

After running the command, reload Cursor. The tools exposed by
`fastskill mcp serve --transport stdio` will be callable from the agent — the read-only ones only.
Mutating tools (`fastskill_project_install`, `fastskill_skill_add`,
`fastskill_skill_remove`, …) require
`mcp serve --enable-write`; see [Write access](/tool-calling/development#write-access).

## How Cursor uses the skills

When you ask Cursor to perform a task, it:

1. **Reads available skills** from the skills directory (and the FastSkill MCP server, if
   registered).
2. **Matches task requirements** against each skill's `description` frontmatter.
3. **Loads the full `SKILL.md`** for relevant skills to get instructions and parameters —
   the same content `fastskill skill read <skill-id>` prints.

### Example

**User query:*&#x2A; &#x2A;"Help me create a PowerPoint presentation for my quarterly review."*

Cursor scans installed skills for presentation-related descriptions, finds a match (e.g.
`pptx`), loads its `SKILL.md`, and follows the skill's instructions to complete the task.

## Configuration

Ensure your project has a `skill-project.toml` and that Cursor and FastSkill agree on the
skills directory. The default is `.claude/skills/`; override it via `skills_directory` under
`[tool.fastskill]` in `skill-project.toml` or the `--skills-dir` flag.

```bash
fastskill project install
fastskill skill list
```

### Multiple skill directories

```bash
fastskill index rebuild --skills-dir .claude/skills/
fastskill index rebuild --skills-dir ./custom-skills/
```

## Automation

Keep the skills directory reconciled in CI so commits always carry the resolved skill set:

```yaml
# .github/workflows/reconcile-skills.yml
name: Reconcile skills
on:
  push:
    paths:
      - 'skill-project.toml'
      - 'skills.lock'

jobs:
  install-skills:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install FastSkill CLI
        run: |
          # Add the steps your org uses to place `fastskill` on PATH (release tarball, package, etc.)
          fastskill -V
      - name: Install skills from lock
        run: fastskill project install --lock
      - name: Verify
        run: fastskill skill list
```

## Troubleshooting

**Cursor doesn't see a skill**

```bash
# Confirm it is installed and reconciled
fastskill skill list

# Confirm its content is readable
fastskill skill read <skill-id>
```

**Semantic search returns nothing**

```bash
# Reindex (requires an embedding provider; see `fastskill cli doctor`)
fastskill index rebuild --skills-dir .claude/skills/

# Keyword fallback works without a provider
fastskill skill search "test query" --local --embedding false
```

**Check environment readiness**

```bash
fastskill cli doctor
```

## Summary

FastSkill's Cursor integration provides:

* **Direct skill discovery** — Cursor reads installed skills from the skills directory, no sync step
* **MCP access** — register the FastSkill MCP server for tool-based access inside Cursor
* **Reproducible installs** — `skill-project.toml` + `skills.lock` keep the skill set consistent across the team
* **Optional semantic search** — `fastskill index rebuild` + `fastskill skill search --local` when an embedding provider is configured

See the [agent’s skill documentation](https://cursor.com/docs/skills) for native discovery behavior.


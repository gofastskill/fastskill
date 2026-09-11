# Install your first skill

FastSkill 0.9.228

Source: https://docs.gofastskill.com/quickstart

Release revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d

Documentation revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d



## Before you start

[Install FastSkill](/installation) and open a Bash shell (Linux, macOS, or WSL).
This walkthrough uses only local files. No account, registry, or API key is needed.
Run it in a new directory so it does not affect an existing project.

## Create a project and a skill

```bash
mkdir fastskill-demo
cd fastskill-demo
fastskill project init --yes --skills-dir .claude/skills
mkdir -p source/review-notes
cat > source/review-notes/SKILL.md <<'EOF'
---
name: review-notes
description: Review meeting notes and extract decisions, owners, and next actions.
metadata:
  version: "1.0.0"
---

When reviewing meeting notes:
1. List the decisions that were made.
2. Extract action items with an owner and due date when stated.
3. Mark missing owners or dates as unspecified; do not invent them.
EOF
```

The project manifest is `skill-project.toml`. The skill's source is separate from
its installation directory, `.claude/skills`.


## Install and verify

```bash
fastskill skill add ./source/review-notes --no-reindex
fastskill skill list --check
fastskill skill read review-notes
fastskill skill search "meeting" --local --embedding false --format json
```

`skill add` records the dependency and installs it. `skill list --check` must
succeed; `skill read` prints the instructions above. Keyword search returns the
installed skill as JSON without calling an embedding provider.


## Use it with an agent

Open Claude Code in `fastskill-demo` and ask it to review some meeting notes using
`review-notes`. The skill is available in `.claude/skills/review-notes/SKILL.md`.
The agent decides when to load a skill; installation does not execute its instructions.
See [Claude Code](/integration/claude-code-integration) or
[Cursor](/integration/cursor-integration) for discovery and troubleshooting.


## Restore the recorded selection

```bash
fastskill project install --lock --offline
fastskill skill list --check
```

Keep `skill-project.toml`, `skills.lock`, and `source/review-notes` together when
sharing this local-source project. The local source must remain available for
restoration. For a portable artifact, use a [team bundle](/cli-reference/bundle-command).



## Next steps

* [Edit and manage skills](/skill-management/reconciliation): updates, ownership, and local changes.
* [Configure sources](/registry/sources): Git, local catalogs, and HTTP registries.
* [Enable semantic search](/cli-reference/reindex-command): optional embedding configuration.
* [Use FastSkill from an agent](/integration/agents): Markdown docs, JSON, and MCP tools.


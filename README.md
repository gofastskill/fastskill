# FastSkill

**Package manager and operational toolkit for AI agent skills.**

[![CI](https://github.com/gofastskill/fastskill/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/gofastskill/fastskill/actions/workflows/test.yml)
[![codecov](https://codecov.io/gh/gofastskill/fastskill/branch/main/graph/badge.svg)](https://codecov.io/gh/gofastskill/fastskill)

FastSkill brings package management to AI agent skills. It follows the Claude Code `SKILL.md`
layout and adds a manifest (`skill-project.toml`), a lockfile (`skills.lock`), semantic search,
quality evals, and a local HTTP/MCP server — so you can **install, organize, discover, and operate
skills reproducibly**, alone or across a team. Think `npm`/`uv`, but for the skills your agents load.

Skills are read directly from your skills directory by Claude Code, Cursor, and other compatible
agents — there is no metadata-file sync step. FastSkill manages the files; your agent reads them.

---

## Who it's for

| You are… | FastSkill gives you… |
|----------|----------------------|
| **A developer** using Claude Code / Cursor | One command to install a skill from git, a folder, a zip, or a registry — and `list`/`read`/`search` to see exactly what your agent will load. |
| **A team** sharing an agent setup | A committed `skill-project.toml` + `skills.lock` so every teammate and CI gets the identical skill set, with groups to split dev-only from production skills. |
| **An organization** operating skills at scale | Private repositories, reproducible locked installs, duplicate/cluster analysis across large skill collections, and a local API/MCP surface for integration. |
| **A skill author** | `init` to scaffold a manifest, `eval` to test that your skill triggers on the cases you care about, `optimize` to refine it automatically, and `marketplace create` to publish a catalog. |

## What you can do

- **Install skills** from a local folder, a git repo (branch/tag/subdirectory), a zip URL, or a registry ID.
- **Keep installs reproducible** with `skill-project.toml` + `skills.lock`; split optional vs production skills into groups.
- **Discover skills** by meaning with semantic search (remote catalogs by default, `--local` for installed skills).
- **Test skill quality** with eval suites (`fastskill eval`) before you ship — each case runs isolated in a scratch workspace with only your skill, so trigger rates are reproducible across machines.
- **Improve skills automatically** with the text-gradient optimizer (`fastskill optimize`).
- **Analyze a collection** for near-duplicates, clusters, and similarity (`fastskill analyze`).
- **Serve locally** — a read-only-by-default HTTP API and web UI (`fastskill serve`), plus an MCP server for your agent (`fastskill mcp serve`) whose tools are read-only by default too, until you pass `--enable-write`.
- **Diagnose** your setup at any time with `fastskill doctor`.

## Install

Pick one (see the [installation guide](webdocs/installation.mdx) for all options and platform notes):

```bash
# macOS & Linux (Homebrew)
brew install gofastskill/cli/fastskill

# Windows (Scoop)
scoop bucket add gofastskill https://github.com/gofastskill/scoop-bucket
scoop install fastskill

# Linux & macOS (install script)
curl -fsSL https://raw.githubusercontent.com/gofastskill/fastskill/main/scripts/install.sh | bash
```

Verify:

```bash
fastskill -V
```

## Quick start

```bash
fastskill init                              # scaffold skill-project.toml
fastskill add ./skills/my-skill -e --group dev   # add a local skill (editable), in the dev group
fastskill install                           # apply the manifest, write skills.lock
fastskill list                              # see installed skills + reconciliation status
```

Optional semantic search (needs an embedding provider — set `OPENAI_API_KEY`):

```bash
fastskill reindex                           # build the local vector index
fastskill search "text processing" --local  # find installed skills by meaning
```

## Common scenarios

**Add a skill from anywhere**

```bash
fastskill add ./skills/pptx-helper -e             # local folder, editable (symlink)
fastskill add ./skills -r --group dev             # every SKILL.md under a folder
fastskill add https://github.com/org/skill.git --branch main
fastskill add "https://github.com/org/repo/tree/main/path/to/skill"   # git subdirectory
fastskill add scope/pptx@1.0.0                    # a pinned registry skill
```

**Reproducible install in CI**

```bash
fastskill install --lock          # install exact versions from skills.lock
fastskill install --without dev   # skip the dev group for production
```

**Use a shared catalog (repository)**

```bash
fastskill repos add team-skills --repo-type git-marketplace https://github.com/org/team-skills.git
fastskill repos list
fastskill search "web scraping"   # remote catalogs by default
```

**Test and refine a skill you're authoring**

```bash
fastskill eval validate           # check your eval config
fastskill eval run --all --output-dir ./eval-runs
fastskill optimize run --config optimize.toml     # auto-improve the skill document
```

**Integrate with your agent**

```bash
fastskill mcp install --agent claude --scope project   # expose fastskill as MCP tools
fastskill mcp serve                                    # MCP over stdio (read-only tools)
fastskill serve                                        # local HTTP API + web UI (read-only)
```

Both servers are read-only by default, from one table of mutating operations. Without
`--enable-write`, `fastskill mcp serve` omits the mutating tools (`init`, `add`, `install`,
`update`, `remove`, `reindex`, `repos add/remove/update/refresh`, `cache clean`,
`marketplace create`, `optimize run/resume`) from `tools/list` and refuses a `tools/call` naming one with JSON-RPC
`-32005 MCP_TOOL_DENIED`; `fastskill serve` likewise mounts no write routes. Pass
`--enable-write` to either one to allow mutation.

## Command reference

| Command | What it does |
|---------|--------------|
| `fastskill init` | Scaffold `skill-project.toml` in the current project or skill |
| `fastskill add <source>` | Add a skill from a local path, zip, git URL, or registry ID |
| `fastskill install` | Apply the manifest (`--lock`, `--only`, `--without`) |
| `fastskill update [id]` | Move installed skills forward from their source (`--check`, `--dry-run`) |
| `fastskill remove <id>…` | Uninstall skills and update the manifest + lock |
| `fastskill list` | List installed skills with reconciliation status (`--format`, `--json`) |
| `fastskill read <id>` | Print a skill's `SKILL.md` (`--meta`, `--tree`) |
| `fastskill search <query>` | Search remote catalogs (default) or installed skills (`--local`) |
| `fastskill reindex` | Rebuild the local semantic search index |
| `fastskill repos <cmd>` | Manage repositories & browse catalogs (`list/add/remove/info/update/test/refresh/skills/show/versions`) |
| `fastskill marketplace create` | Generate a `marketplace.json` catalog from a folder of skills |
| `fastskill eval <cmd>` | Skill quality evals (`validate/run/judge/report/score/scorecard`) |
| `fastskill optimize <cmd>` | Text-gradient skill optimization (`run/resume/status/inspect/export`) |
| `fastskill analyze <cmd>` | Similarity `matrix`, `cluster`, and `duplicates` across skills |
| `fastskill serve` | Local HTTP API + web UI (read-only by default; `--enable-write` to mutate) |
| `fastskill mcp <cmd>` | Run/install the MCP server (`serve/install/list`) for agents (tools are read-only by default; `serve --enable-write` to mutate) |
| `fastskill doctor` | Diagnose configuration and environment readiness |

Every command supports `--help`. Run `fastskill <skill-id>` as a shorthand for `fastskill read
<skill-id>` — like `read`, it needs a project (`fastskill init`), or `--global` to read a
globally installed skill.

## Configuration

All project configuration lives in **`skill-project.toml`** at your project root (FastSkill walks up
to find it). A minimal manifest:

```toml
schema_version = "1"

[dependencies]
demo-skill = { origin = { type = "local", path = "./skills/demo-skill", editable = true }, groups = ["dev"] }

[tool.fastskill]
skills_directory = ".claude/skills"

# Only needed for semantic search (reindex / search --local):
[tool.fastskill.embedding]
openai_base_url = "https://api.openai.com/v1"
embedding_model = "text-embedding-3-small"
```

Every dependency names an **origin** — `{ type = "local", path = … }`, `{ type = "git", url = …,
ref = { branch = … } }`, `{ type = "zip-url", url = … }` or `{ type = "repository", repo = …,
skill = … }`. The pre-`Origin` flat shape (`source = "git"` with sibling `url`/`branch`/`path`
keys) is still read and upgraded in memory for older manifests, but it is not written and is
slated for removal — write new manifests with `origin`.

Set `OPENAI_API_KEY` in your environment to enable embedding-based search. See the
[init command guide](webdocs/configuration/init-command.mdx) for `[metadata]`, `[tool.fastskill]`,
embedding settings and schema migration, and [eval setup](webdocs/evals-quality/setup.mdx) for the
`[tool.fastskill.eval]` schema.

## Documentation

- [Welcome](webdocs/welcome.mdx) — the full story and use cases
- [Quick Start](webdocs/quickstart.mdx) · [Installation](webdocs/installation.mdx) · [Cheatsheet](webdocs/cheatsheet.mdx)
- [CLI Reference](webdocs/cli-reference/overview.mdx)
- [Registry & repositories](webdocs/registry/overview.mdx)
- [Evals & quality](webdocs/evals-quality/overview.mdx) · [Optimization](webdocs/optimize/overview.mdx)
- [Integrations: Claude Code](webdocs/integration/claude-code-integration.mdx) · [Cursor](webdocs/integration/cursor-integration.mdx)

## Contributing

FastSkill is a Rust workspace (`fastskill-cli`, `fastskill-core`, `fastskill-evals`). To build from
source, run the test suite, or contribute, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0

# FastSkill

**Package manager and operational toolkit for AI agent skills.**

[![CI](https://github.com/gofastskill/fastskill/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/gofastskill/fastskill/actions/workflows/test.yml)
[![codecov](https://codecov.io/gh/gofastskill/fastskill/branch/main/graph/badge.svg)](https://codecov.io/gh/gofastskill/fastskill)

FastSkill brings package management to AI agent skills. It follows the Claude Code `SKILL.md`
layout and adds a manifest (`skill-project.toml`), a lockfile (`skills.lock`), self-contained team
bundles, semantic search, quality evals, and a local HTTP/MCP server — so you can **install,
organize, discover, and operate skills reproducibly**, alone or across a team. Think `npm`/`uv`,
but for the skills your agents load.

Skills are read directly from your skills directory by Claude Code, Cursor, and other compatible
agents — there is no metadata-file sync step. FastSkill manages the files; your agent reads them.

---

## Who it's for

| You are… | FastSkill gives you… |
|----------|----------------------|
| **A developer** using Claude Code / Cursor | One command to install a skill from git, a folder, a zip, or a registry — and `list`/`read`/`search` to see exactly what your agent will load. |
| **A team** sharing an agent setup | A committed manifest and lockfile for identical installs, plus one-file bundles for versioned, offline-ready team setups. |
| **An organization** operating skills at scale | Private repositories, reproducible locked installs, duplicate/cluster analysis across large skill collections, and a local API/MCP surface for integration. |
| **A skill author** | `init` to scaffold a manifest, `eval` to test that your skill triggers on the cases you care about, `optimize` to refine it automatically, and `marketplace create` to publish a catalog. |

## What you can do

- **Install skills** from a local folder, a git repo (branch/tag/subdirectory), a zip URL, or a registry ID.
- **Keep installs reproducible** with `skill-project.toml` + `skills.lock`; split optional vs production skills into groups.
- **Ship a complete team setup** as a verified, versioned bundle containing its full skill dependency closure.
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
fastskill install                           # restore compatible pins, resolve anything not locked
fastskill list --check                      # verify desired, locked, and installed state
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
fastskill add scope/pptx@1.0.0 --repository team  # exact repository version
fastskill add scope/pptx@latest --repository team # newest stable version
```

For repository skills, an omitted version and `@latest` both mean the newest stable release.
`@1.2.0` is an exact selection; prereleases require an explicit prerelease selector. Use
`--offline` with `add`, `install`, or `update` when the operation must use local and verified
cached inputs without network access or automatic indexing.

**Reproducible install in CI**

```bash
fastskill install --lock --offline # verify and restore exact locked contents
fastskill install --without dev    # skip dev roots, retain their declarations and files
fastskill list --check --without dev # CI check for the same selected closure
```

Project installs, including `--lock`, require `skill-project.toml` because it defines roots,
groups, bundles, and the install destination. Restore `global-skills.lock` with
`fastskill install --global --lock`.

**Build and share a team bundle**

```toml
[bundle]
format = "fastskill-bundle-v1"
id = "platform-team"
version = "1.0.0"

[bundle.members.code-review]

[dependencies]
code-review = "2.1.0"
```

```bash
fastskill bundle build --output dist
fastskill add dist/platform-team-1.0.0.zip
fastskill list --bundles
```

Bundle builds validate each selected skill against its declared version. Installed bundle members
stay protected from ordinary `remove`; remove the owning setup with
`fastskill remove --bundle platform-team --force`. See the [bundle guide](webdocs/cli-reference/bundle-command.mdx)
for updates, lock-based restoration, and personal overrides.

Reset a permitted personal replacement to the packaged member with
`fastskill bundle override <member-id> --reset`. FastSkill keeps a shared member until its last
direct, transitive, bundle, or override owner is removed.

**Use a shared catalog (repository)**

```bash
fastskill repos add team-skills --repo-type git-marketplace https://github.com/org/team-skills.git
fastskill repos add stable-skills --repo-type git-marketplace https://github.com/org/skills.git --tag v1.2.0
fastskill repos list
fastskill search "web scraping" --repository team-skills
fastskill add scope/scraper@latest --repository team-skills
```

Git repository `--branch` and `--tag` selections are saved in `skill-project.toml` and reused by
catalog refresh and installation.

**Test and refine a skill you're authoring**

```bash
fastskill eval validate           # check your eval config
fastskill eval run --all --output-dir ./eval-runs
fastskill optimize run --config optimize.toml     # auto-improve the skill document
```

**Integrate with your agent**

```bash
fastskill mcp install --agent claude --scope project   # expose fastskill as MCP tools
fastskill mcp serve --transport stdio                  # MCP over stdio (read-only tools)
fastskill serve                                        # local HTTP API + web UI (read-only)
```

Both servers are read-only by default, from one table of mutating operations. Without
`--enable-write`, `fastskill mcp serve` omits the mutating tools (`init`, `add`, `install`,
`update`, `remove`, `reindex`, `repos add/remove/update/refresh`, `cache clean`,
`bundle build/override`, `marketplace create`, evaluation or optimization execution, and artifact
exports) from `tools/list` and refuses a `tools/call` naming one with JSON-RPC
`-32005 MCP_TOOL_DENIED`; `fastskill serve` likewise rejects write routes. Pass
`--enable-write` to either one to allow mutation.

## Command reference

| Command | What it does |
|---------|--------------|
| `fastskill init` | Scaffold `skill-project.toml` in the current project or skill |
| `fastskill add <source>` | Add a skill and its required closure (`--repository`, `--offline`) |
| `fastskill install` | Restore the manifest, preferring compatible pins (`--lock`, `--only`, `--without`, `--offline`) |
| `fastskill update [id]` | Resolve deliberate changes (`--to-version`, `--strategy`, `--repository`, `--check`, `--dry-run`, `--offline`) |
| `fastskill remove <id>…` | Uninstall skills and update the manifest + lock |
| `fastskill list` | Compare desired, locked, and installed state (`--check`, `--only`, `--without`, `--json`) |
| `fastskill read <id>` | Print a skill's `SKILL.md` (`--meta`, `--tree`) |
| `fastskill search <query>` | Search remote catalogs (default) or installed skills (`--local`) |
| `fastskill reindex` | Rebuild the local semantic search index |
| `fastskill repos <cmd>` | Manage repositories & browse catalogs (`list/add/remove/info/update/test/refresh/skills/show/versions`) |
| `fastskill cache <cmd>` | Inspect and reclaim the on-disk skill content cache (`info/clean`) |
| `fastskill bundle <cmd>` | Build a self-contained team bundle or declare a permitted personal override |
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
- [Team bundles](webdocs/cli-reference/bundle-command.mdx)
- [Registry & repositories](webdocs/registry/overview.mdx)
- [Evals & quality](webdocs/evals-quality/overview.mdx) · [Optimization](webdocs/optimize/overview.mdx)
- [Integrations: Claude Code](webdocs/integration/claude-code-integration.mdx) · [Cursor](webdocs/integration/cursor-integration.mdx)

## Contributing

To build from source, run the test suite, or contribute, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0

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
| **A developer** using Claude Code / Cursor | One command to install a skill from git, a folder, a zip, or a repository — and `skill list`/`skill read`/`skill search` to see exactly what your agent will load. |
| **A team** sharing an agent setup | A committed manifest and lockfile for identical installs, plus one-file bundles for versioned, offline-ready team setups. |
| **An organization** operating skills at scale | Private repositories, reproducible locked installs, duplicate/cluster analysis across large skill collections, and a local API/MCP surface for integration. |
| **A skill author** | `project init` to scaffold a manifest, `eval` to test that your skill triggers on the cases you care about, `optimization` to refine it automatically, and `marketplace create` to publish a catalog. |

## What you can do

- **Install skills** from a local folder, a git repo (branch/tag/subdirectory), a zip URL, or a registry ID.
- **Keep installs reproducible** with `skill-project.toml` + `skills.lock`; split optional vs production skills into groups.
- **Ship a complete team setup** as a verified, versioned bundle containing its full skill dependency closure.
- **Discover skills** by meaning with semantic search (remote catalogs by default, `--local` for installed skills).
- **Test skill quality** with eval suites (`fastskill eval`) before you ship — each case runs isolated in a scratch workspace with only your skill, so trigger rates are reproducible across machines.
- **Improve skills automatically** with the text-gradient optimizer (`fastskill optimization`).
- **Analyze a collection** for near-duplicates, clusters, and similarity (`fastskill analysis`).
- **Serve locally** — a read-only-by-default HTTP API and web UI (`fastskill server serve`), plus an MCP server for your agent (`fastskill mcp serve`) whose tools are read-only by default too, until you pass `--enable-write`.
- **Diagnose** your setup at any time with `fastskill cli doctor`.

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
fastskill project init                                  # scaffold skill-project.toml
fastskill skill add ./skills/my-skill -e --group dev   # add an editable skill to the dev group
fastskill project install                               # restore compatible pins and resolve new roots
fastskill skill list --check                            # verify desired, locked, and installed state
```

Optional semantic search (needs an embedding provider — set `OPENAI_API_KEY`):

```bash
fastskill index rebuild                           # build the local vector index
fastskill skill search "text processing" --local  # find installed skills by meaning
```

## Common scenarios

**Add a skill from anywhere**

```bash
fastskill skill add ./skills/pptx-helper -e             # local folder, editable (symlink)
fastskill skill add ./skills -r --group dev             # every SKILL.md under a folder
fastskill skill add https://github.com/org/skill.git --branch main
fastskill skill add "https://github.com/org/repo/tree/main/path/to/skill"   # git subdirectory
fastskill skill add scope/pptx@1.0.0 --repository team  # exact repository version
fastskill skill add scope/pptx@latest --repository team # newest stable version
```

For repository skills, an omitted version and `@latest` both mean the newest stable release.
`@1.2.0` is an exact selection; prereleases require an explicit prerelease selector. Use
`--offline` with `skill add`, `project install`, or `skill update` when the operation must use
local and verified cached inputs without network access or automatic indexing.

**Reproducible install in CI**

```bash
fastskill project install --lock --offline # verify and restore exact locked contents
fastskill project install --without dev    # skip dev roots, retain their declarations and files
fastskill skill list --check --without dev # CI check for the same selected closure
```

Project installs, including `--lock`, require `skill-project.toml` because it defines roots,
groups, bundles, and the install destination. Restore `global-skills.lock` with
`fastskill project install --global --lock`.

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
fastskill bundle add dist/platform-team-1.0.0.zip
fastskill bundle list
```

Bundle builds validate each selected skill against its declared version. Installed bundle members
stay protected from `skill remove`; remove the owning setup with
`fastskill bundle remove platform-team --force`. See the
[bundle guide](webdocs/cli-reference/bundle-command.mdx) for updates, lock-based restoration, and
personal overrides.

Reset a permitted personal replacement to the packaged member with
`fastskill bundle override <member-id> --reset`. FastSkill keeps a shared member until its last
direct, transitive, bundle, or override owner is removed.

**Use a shared catalog (repository)**

```bash
fastskill repo add team-skills --repo-type git-marketplace https://github.com/org/team-skills.git
fastskill repo add stable-skills --repo-type git-marketplace https://github.com/org/skills.git --tag v1.2.0
fastskill repo list
fastskill skill search "web scraping" --repository team-skills
fastskill skill add scope/scraper@latest --repository team-skills
```

Git repository `--branch` and `--tag` selections are saved in `skill-project.toml` and reused by
catalog refresh and installation.

**Test and refine a skill you're authoring**

```bash
fastskill eval validate           # check your eval config
fastskill eval run --all --output-dir ./eval-runs
fastskill optimization run --config optimize.toml     # auto-improve the skill document
```

**Integrate with your agent**

```bash
fastskill mcp install --agent claude --scope project   # expose fastskill as MCP tools
fastskill mcp serve --transport stdio                  # MCP over stdio (read-only tools)
fastskill server serve                              # local HTTP API + web UI (read-only)
```

Both servers are read-only by default, from one table of mutating operations. Without
`--enable-write`, `fastskill mcp serve` omits mutating tools for `project init/install`,
`skill add/update/remove`, `bundle build/add/update/remove/override`, `index rebuild`, repository
changes, `cache clean`, `marketplace create`, evaluation or optimization execution, and artifact
exports. It refuses a `tools/call` naming one with JSON-RPC
`-32005 MCP_TOOL_DENIED`; `fastskill server serve` likewise rejects write routes. Pass
`--enable-write` to either one to allow mutation.

## Command reference

| Namespace | Actions | What it does |
| --- | --- | --- |
| `fastskill skill` | `add`, `remove`, `update`, `list`, `read`, `search` | Manage and discover individual skills. |
| `fastskill bundle` | `build`, `add`, `list`, `update`, `remove`, `override` | Build and manage self-contained team bundles. |
| `fastskill project` | `init`, `install` | Create and restore a skill project. |
| `fastskill repo` | `add`, `list`, `info`, `update`, `remove`, `test`, `refresh`, `skills`, `show`, `versions` | Configure repositories and browse catalogs. |
| `fastskill marketplace` | `create` | Generate a `marketplace.json` catalog. |
| `fastskill analysis` | `matrix`, `cluster`, `duplicates` | Analyze similarity across installed skills. |
| `fastskill eval` | `validate`, `run`, `judge`, `report`, `score`, `scorecard` | Run and report skill evaluations. |
| `fastskill optimization` | `run`, `resume`, `status`, `inspect`, `export` | Optimize a skill document and inspect runs. |
| `fastskill index` | `rebuild` | Rebuild the local semantic index. |
| `fastskill cache` | `info`, `clean` | Inspect and reclaim cached content. |
| `fastskill server` | `serve` | Run the local HTTP API and web UI. |
| `fastskill mcp` | `serve`, `install`, `list` | Run and configure the MCP server. |
| `fastskill cli` | `doctor`, `completion`, `spec` | Diagnose FastSkill and export CLI metadata. |

Every command supports `--help`. Skill IDs must follow an explicit action, such as
`fastskill skill read <skill-id>`.

## Configuration

All project configuration lives in **`skill-project.toml`** at your project root (FastSkill walks up
to find it). A minimal manifest:

```toml
schema_version = "1"

[dependencies]
demo-skill = { origin = { type = "local", path = "./skills/demo-skill", editable = true }, groups = ["dev"] }

[tool.fastskill]
skills_directory = ".claude/skills"

# Only needed for semantic search (index rebuild / skill search --local):
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
[project init guide](webdocs/configuration/init-command.mdx) for `[metadata]`, `[tool.fastskill]`,
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

# FastSkill

Package manager and operational toolkit for Agent AI Skills.

[![CI](https://github.com/gofastskill/fastskill/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/gofastskill/fastskill/actions/workflows/test.yml)
[![codecov](https://codecov.io/gh/gofastskill/fastskill/branch/main/graph/badge.svg)](https://codecov.io/gh/gofastskill/fastskill)

## Why FastSkill

FastSkill helps you manage AI skills with a clean, repeatable workflow:

- Install skills from local folders, git repositories, or registries
- Keep installs reproducible with `skill-project.toml` and `skills.lock`
- Discover skills with remote and local search
- Validate and evaluate skill quality before sharing
- Sync installed skills into agent metadata files

## Quick start

```bash
fastskill -V
fastskill init
fastskill add ./skills/my-skill -e --group dev
fastskill install
fastskill list
```

Optional local search flow:

```bash
fastskill reindex
fastskill search "text processing" --local
```

## skill-project.toml (project manifest)

`skill-project.toml` is the project manifest for skill dependencies.
It keeps installs reproducible across teammates and CI together with `skills.lock`.

Minimal example:

```toml
[dependencies]
demo-skill = { source = "local", path = "./skills/demo-skill", editable = true, groups = ["dev"] }
```

Typical workflow:

```bash
fastskill add ./skills/demo-skill -e --group dev
fastskill install
fastskill list
```

## Usage examples

### Add from local folder (editable)

```bash
fastskill add ./skills/pptx-helper -e --group dev
fastskill install
```

### Add from git

```bash
fastskill add https://github.com/org/skill.git --branch main
fastskill install
```

### Add from registry

```bash
fastskill add scope/pptx@1.0.0
fastskill install --lock
```

## Core commands

| Command | What it does |
|---------|--------------|
| `fastskill add <source>` | Add a skill dependency from local, git, zip, or registry |
| `fastskill install` | Apply dependencies from `skill-project.toml` |
| `fastskill list` | List installed skills |
| `fastskill search "<query>"` | Search remote catalog (default) |
| `fastskill search "<query>" --local` | Search installed skills |
| `fastskill eval validate` | Validate eval configuration and checks |
| `fastskill sync --yes` | Sync installed skills to agent metadata |
| `fastskill package` | Package skills for distribution |

## Documentation

- [Welcome](webdocs/welcome.mdx)
- [Quick Start](webdocs/quickstart.mdx)
- [Installation](webdocs/installation.mdx)
- [CLI Reference](webdocs/cli-reference/overview.mdx)
- [Registry Guide](docs/REGISTRY.md)

## Crates

This repository is a Rust workspace with three primary crates:

- [`crates/fastskill-cli`](crates/fastskill-cli): CLI binary and command routing.
- [`crates/fastskill-core`](crates/fastskill-core): reusable service/library layer.
- [`crates/evals-core`](crates/evals-core): standalone evaluation engine primitives.

Each crate has its own docs:

- `crates/fastskill-cli/README.md`
- `crates/fastskill-cli/CONTRIBUTING.md`
- `crates/fastskill-core/README.md`
- `crates/fastskill-core/CONTRIBUTING.md`
- `crates/evals-core/README.md`
- `crates/evals-core/CONTRIBUTING.md`

## License

Apache-2.0

# FastSkill

Package manager and operational toolkit for Agent AI Skills.

[![CI](https://github.com/gofastskill/fastskill/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/gofastskill/fastskill/actions/workflows/test.yml)
[![codecov](https://codecov.io/gh/gofastskill/fastskill/branch/main/graph/badge.svg)](https://codecov.io/gh/gofastskill/fastskill)

FastSkill helps teams install, organize, and operate AI skills with reproducible workflows.

## Highlights

- Install skills from local folders, git repositories, or registries
- Keep installs reproducible with `skill-project.toml` and `skills.lock`
- Discover skills with remote and local search
- Validate and evaluate skill quality before sharing
- Diagnose your environment with `fastskill doctor`
- Publish skills only when you enable the optional registry feature

## Quick start

```bash
fastskill -V
fastskill init
fastskill add ./skills/my-skill -e --group dev
fastskill install
fastskill list
```

Optional local workflow:

```bash
fastskill reindex
fastskill search "text processing" --local
```

## Features

### Projects

`skill-project.toml` defines skill dependencies. `skills.lock` keeps installs reproducible.

```toml
[dependencies]
demo-skill = { source = "local", path = "./skills/demo-skill", editable = true, groups = ["dev"] }
```

```bash
fastskill add ./skills/demo-skill -e --group dev
fastskill install
fastskill list
```

### Discovery

| Command | What it does |
|---------|--------------|
| `fastskill search "<query>"` | Search remote catalog by default |
| `fastskill search "<query>" --local` | Search installed skills on disk |
| `fastskill read <id>` | Print a skill's `SKILL.md` (add `--meta` or `--tree`) |
| `fastskill doctor` | Diagnose configuration and environment |

### Validation and evals

| Command | What it does |
|---------|--------------|
| `fastskill eval validate` | Validate eval configuration and checks |
| `fastskill reindex` | Build or refresh the local search index |

### Distribution

| Command | What it does |
|---------|--------------|
| `fastskill package` | Package skills for distribution |
| `fastskill repos list` | List configured registries and catalogs |
| `fastskill repos skills` | Browse skills from configured sources |

Registry publish is optional and only available in builds that enable `registry-publish`.

## Documentation

- [Welcome](webdocs/welcome.mdx)
- [Quick Start](webdocs/quickstart.mdx)
- [Installation](webdocs/installation.mdx)
- [CLI Reference](webdocs/cli-reference/overview.mdx)
- [Registry Guide](webdocs/registry/overview.mdx)

## Crates

This repository is a Rust workspace with three primary crates:

- [`crates/fastskill-cli`](crates/fastskill-cli): CLI binary and command routing.
- [`crates/fastskill-core`](crates/fastskill-core): reusable service/library layer.
- [`crates/fastskill-evals`](crates/fastskill-evals): standalone evaluation engine primitives.

Each crate has its own docs:

- `crates/fastskill-cli/README.md`
- `crates/fastskill-cli/CONTRIBUTING.md`
- `crates/fastskill-core/README.md`
- `crates/fastskill-core/CONTRIBUTING.md`
- `crates/fastskill-evals/README.md`
- `crates/fastskill-evals/CONTRIBUTING.md`

## License

Apache-2.0

# skill search

FastSkill 0.9.243

Source: https://docs.gofastskill.com/cli-reference/search-command

Release revision: e4ca871058054389e06ffc0b42286eb5b78f48c0

Documentation revision: e4ca871058054389e06ffc0b42286eb5b78f48c0



# `skill search`

`fastskill skill search` searches configured remote catalogs by default. Pass `--local` to search
installed skills. Remote search uses repository catalog metadata; local search uses installed
`SKILL.md` metadata and, when configured, the vector index.

## Usage

```console
$ fastskill skill search <QUERY> [OPTIONS]
```

Omitting both scope flags is equivalent to `--remote`. Use `--local` explicitly when the intent is
to search only installed skills.

## Options

| Option                  | Description                                                        | Default                         |
| ----------------------- | ------------------------------------------------------------------ | ------------------------------- |
| `<QUERY>`               | Required search text.                                              | —                               |
| `--local`               | Search installed skills.                                           | `false`                         |
| `--remote`              | Search configured catalogs.                                        | `true` when `--local` is absent |
| `--repository <NAME>`   | Restrict remote search to one configured repository.               | all repositories                |
| `-l, --limit <NUMBER>`  | Return between 1 and 1,000 results.                                | `10`                            |
| `-f, --format <FORMAT>` | Render `table`, `json`, `grid`, or `xml`.                          | `table`                         |
| `--json`                | Shorthand for `--format json`; mutually exclusive with `--format`. | `false`                         |
| `--embedding <MODE>`    | Local search mode: `true`, `false`, or `auto`.                     | `auto`                          |
| `--paths`               | Return resolved paths as JSON; requires `--local`.                 | `false`                         |
| `--content <MODE>`      | Include `none`, `preview`, or `full`; requires `--local --paths`.  | `none`                          |
| `--skills-dir <PATH>`   | Override the installation directory for this invocation.           | project setting                 |
| `--global`              | Use the global skills directory.                                   | `false`                         |

## Remote search

Configure a repository before searching or adding by skill ID:

```console
$ fastskill repo add team --repo-type git-marketplace https://github.com/your-org/skills.git
$ fastskill skill search "review notes" --repository team
$ fastskill skill search "review notes" --repository team --format json
```

Remote result rows contain `ID`, `Name`, `Description`, `Source`, and `Similarity`. Keyword catalog
hits report a similarity of `1.000`. Human-readable output also prints the repository-qualified add
command supplied by the result.

JSON is an array of result objects. XML always emits a `<skills>` document, including when no
results match. A successful zero-result search exits 0. Partial repository failure returns the
successful results plus diagnostics and exits nonzero.

## Local search

Keyword search does not require an embedding provider:

```console
$ fastskill skill search "pdf" --local --embedding false
```

`--embedding true` requires an embedding provider and fails clearly when none is configured.
`--embedding auto` falls back to keyword search with a warning when embeddings are unavailable.
An omitted mode also falls back, without printing the warning on every ordinary local search.

```console
$ export OPENAI_API_KEY="your-openai-api-key"
$ fastskill index rebuild
$ fastskill skill search "process documents" --local --embedding true
```

## Resolved paths and content

`--paths` always emits JSON, regardless of `--format`, because the response includes structured
path fields and `allowed_roots`:

```console
$ fastskill skill search "presentation" --local --paths
$ fastskill skill search "presentation" --local --paths --content preview
```

Copied installs resolve beneath the configured skills directory. Editable installs resolve through
their managed top-level symlink to the source directory, so `skill_md_path` and `skill_root_path`
remain usable during local development.

## Validation and exit behavior

The command rejects conflicting scopes, an invalid format, an invalid embedding/content mode,
`--repository` with `--local`, and path/content flags outside local scope. It exits 0 for a complete
search, including no matches, and nonzero for invalid input or incomplete repository results.

## See also

* [Configure skill sources](/registry/sources).
* [Skill and project commands](/cli-reference/skill-commands).
* [Reconciliation](/skill-management/reconciliation).


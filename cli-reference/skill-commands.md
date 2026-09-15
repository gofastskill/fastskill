# Skill and project commands

FastSkill 0.9.233

Source: https://docs.gofastskill.com/cli-reference/skill-commands

Release revision: ecd9b1230f1aa44e755628f4ce266b570b8529d0

Documentation revision: ecd9b1230f1aa44e755628f4ce266b570b8529d0



# Skill and project commands

Use `fastskill skill` to manage individual skills. Use `fastskill project` to initialize project
state and restore all declared dependencies. Run a command with `--help` for the generated,
version-matched option list.

## Shared scope options

`--global` selects the user-level skills directory and global lock where the command supports that
scope. `--skills-dir <PATH>` overrides the installation directory for one invocation. In project
mode fastskill warns that subsequent project commands must use the same override to inspect the
same installed state. `-v, --verbose` enables diagnostic logs.

## `skill add`

```console
$ fastskill skill add <SOURCE> [OPTIONS]
```

`SOURCE` can be a local directory, local ZIP, Git URL, remote ZIP URL, or skill ID. An existing
relative path wins over the skill-ID shorthand. Use `./name` when a nonexistent local path would
otherwise look like an ID.

| Option                                         | Description                                          |
| ---------------------------------------------- | ---------------------------------------------------- |
| `--source-type <registry\|github\|git\|local>` | Override source inference.                           |
| `--repository <NAME>`                          | Select a configured repository for a skill ID.       |
| `--branch <BRANCH>`                            | Select a Git branch; exclusive with `--tag`.         |
| `--tag <TAG>`                                  | Select a Git tag; exclusive with `--branch`.         |
| `-f, --force`                                  | Replace an existing direct declaration.              |
| `-e, --editable`                               | Link a local directory for in-place development.     |
| `--group <GROUP>`                              | Add the root to a nonempty group.                    |
| `-r, --recursive`                              | Add every skill below a local directory.             |
| `--offline`                                    | Use verified cached/local inputs only.               |
| `--reindex` / `--no-reindex`                   | Override automatic vector indexing.                  |
| `--dry-run`                                    | Validate and preview without changing managed state. |
| `--json`                                       | Emit one lifecycle JSON document.                    |

```console
$ fastskill skill add ./skills/reviewer --editable --group dev
$ fastskill skill add ./skills --recursive
$ fastskill skill add https://github.com/your-org/reviewer.git --branch main
$ fastskill skill add reviewer@1.2.0 --repository team
```

For a skill below a Git repository root, declare a clean clone URL plus `subdir`; see
[Configure skill sources](/registry/sources). A Git repository with multiple candidate skills and
no selected subdirectory is rejected instead of installing whichever directory is visited first.

## `skill remove`

```console
$ fastskill skill remove <SKILL_ID>... [OPTIONS]
```

| Option                       | Description                                                          |
| ---------------------------- | -------------------------------------------------------------------- |
| `-f, --force`                | Skip confirmation; required for non-interactive removal.             |
| `--reindex` / `--no-reindex` | Override automatic vector indexing.                                  |
| `--dry-run`                  | Validate and preview without changing managed state.                 |
| `--json`                     | Emit one lifecycle JSON document; requires `--force` unless dry-run. |

Validation and ownership planning happen before an interactive prompt. Missing targets are reported
without asking for confirmation. A requested install that exists on disk but is missing from a
damaged/stale manifest or lock can still be removed. Locally modified managed installs remain
protected; `--force` skips only the prompt.

Declining the prompt or reaching end-of-input cancels removal without changing files and exits
with status 1. Non-interactive input requires `--force` or `--dry-run`.

## `skill update`

```console
$ fastskill skill update [SKILL_ID] [OPTIONS]
```

| Option                                     | Description                                                        |
| ------------------------------------------ | ------------------------------------------------------------------ |
| `--check`                                  | Resolve and validate without applying; exclusive with `--dry-run`. |
| `--dry-run`                                | Preview without changing state; exclusive with `--check`.          |
| `--to-version <VERSION>`                   | Exact version for one repository-origin `SKILL_ID`.                |
| `--strategy <latest\|patch\|minor\|major>` | Strategy for one repository-origin `SKILL_ID`.                     |
| `--repository <NAME>`                      | Repository for one repository-origin `SKILL_ID`.                   |
| `--source <NAME>`                          | Deprecated alias for `--repository`.                               |
| `--offline`                                | Use verified cached/local inputs only.                             |
| `--reindex` / `--no-reindex`               | Override automatic vector indexing.                                |
| `--json`                                   | Emit one lifecycle JSON document.                                  |

See [`skill update`](/cli-reference/update-command) for version and closure behavior.

## `skill list`

```console
$ fastskill skill list [OPTIONS]
```

| Option                              | Description                                               |
| ----------------------------------- | --------------------------------------------------------- |
| `--format <table\|json\|grid\|xml>` | Select the output format.                                 |
| `--json`                            | Shorthand for `--format json`; exclusive with `--format`. |
| `--details`                         | Include version, source, and managed-state details.       |
| `--check`                           | Exit nonzero when selected state needs reconciliation.    |
| `--only <GROUP>...`                 | List and check roots in the selected groups.              |
| `--without <GROUP>...`              | Exclude roots in the selected groups.                     |

JSON rows expose the actual reconciliation schema, including `reconciliation`, `source_type`,
`source_path`, `in_manifest`, `in_lock`, `installed`, `mutable`, and `extraneous`. `mutable` is the
machine-readable field for an editable install; human-readable flags say `editable`.

## `skill read`

```console
$ fastskill skill read <SKILL_ID> [OPTIONS]
```

With no options, stdout is the exact installed `SKILL.md` content, so redirection creates a faithful
copy.

| Option                              | Description                                       |
| ----------------------------------- | ------------------------------------------------- |
| `--meta`                            | Show structured metadata instead of file content. |
| `--tree`                            | Show the installed dependency tree.               |
| `--format <table\|json\|grid\|xml>` | Select metadata output; requires `--meta`.        |
| `--json`                            | Metadata JSON shorthand; requires `--meta`.       |
| `--locked`                          | Read locked metadata; requires `--meta`.          |

Invalid formats are rejected. Installed dependency data is loaded from `SKILL.md` frontmatter, so
`--tree` works without `--locked`.

## `skill search`

```console
$ fastskill skill search <QUERY> [OPTIONS]
```

Remote scope is the default. `--local` searches installed skills. Options include
`--repository <NAME>`, `-l, --limit <NUMBER>`, `-f, --format <FORMAT>`, `--json`,
`--embedding <true|false|auto>`, `--paths`, and `--content <none|preview|full>`. See
[`skill search`](/cli-reference/search-command) for scope, path, and output contracts.

## Project commands

`fastskill project init` creates `skill-project.toml`. `fastskill project install` restores the
selected dependency closure and writes `skills.lock`. See the dedicated
[`project init`](/cli-reference/init-command) and
[`project install`](/cli-reference/install-command) references.

## Migrating from older versions

The former root verbs moved below the `skill` namespace:

| Before                   | Now                            |
| ------------------------ | ------------------------------ |
| `fastskill add SOURCE`   | `fastskill skill add SOURCE`   |
| `fastskill remove ID`    | `fastskill skill remove ID`    |
| `fastskill update [ID]`  | `fastskill skill update [ID]`  |
| `fastskill list`         | `fastskill skill list`         |
| `fastskill read ID`      | `fastskill skill read ID`      |
| `fastskill search QUERY` | `fastskill skill search QUERY` |

Running an old verb prints the corresponding migration command rather than an unrelated fuzzy
suggestion.


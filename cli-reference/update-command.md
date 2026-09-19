# skill update

FastSkill 0.9.237

Source: https://docs.gofastskill.com/cli-reference/update-command

Release revision: 5b80b2c114e7507b8e2af92ec7e725d9d68fe3f2

Documentation revision: 5b80b2c114e7507b8e2af92ec7e725d9d68fe3f2



# `skill update`

`fastskill skill update` re-resolves selected roots from their recorded origins, validates each
affected dependency closure, applies the result, and updates the lockfile. Exact repository pins
remain exact unless one named root is changed with a version or strategy control.

## Usage

```console
$ fastskill skill update [SKILL_ID] [OPTIONS]
```

## Options

| Option                   | Description                                                                                     | Default             |
| ------------------------ | ----------------------------------------------------------------------------------------------- | ------------------- |
| `[SKILL_ID]`             | Update one declared root; omit it to update every root.                                         | all roots           |
| `--check`                | Resolve and validate without applying; exclusive with `--dry-run`.                              | `false`             |
| `--dry-run`              | Preview without changing managed state; exclusive with `--check`.                               | `false`             |
| `--to-version <VERSION>` | Set one repository-origin root to an exact version; requires `SKILL_ID`.                        | —                   |
| `--strategy <STRATEGY>`  | Use `latest`, `patch`, `minor`, or `major` for one repository-origin root; requires `SKILL_ID`. | recorded constraint |
| `--repository <NAME>`    | Select a configured repository for one repository-origin root; requires `SKILL_ID`.             | recorded repository |
| `--source <NAME>`        | Deprecated alias for `--repository`.                                                            | —                   |
| `--offline`              | Use verified local sources and cached artifacts only.                                           | `false`             |
| `--reindex`              | Rebuild the search index after applying.                                                        | configuration       |
| `--no-reindex`           | Skip rebuilding the search index.                                                               | configuration       |
| `--json`                 | Emit one lifecycle JSON document.                                                               | `false`             |
| `--skills-dir <PATH>`    | Override the installation directory for this invocation.                                        | project setting     |
| `--global`               | Update the global skill set and `global-skills.lock`.                                           | `false`             |

`--to-version`, `--strategy`, and `--repository` apply only to repository-origin skills. The
command rejects these controls without one `SKILL_ID`, controls applied to local/Git/ZIP origins,
conflicting `--source` and `--repository` values, and `--check` with `--dry-run`.

## Preview and check

Both preview modes resolve and validate the same candidates as an update, without changing files,
the manifest, lockfile, timestamps, repository indexes, or the vector index.

```console
$ fastskill skill update --check
  • alpha: 1.0.0 -> 1.0.0 (unchanged)
[INFO] No changes were applied

$ fastskill skill update alpha --dry-run --json
{
  "scope": "project",
  "outcome": "unchanged",
  "dry_run": true,
  "targets": [
    {
      "id": "alpha",
      "outcome": "unchanged",
      "current_revision": "1.0.0",
      "target_revision": "1.0.0",
      "changes": [],
      "retained": []
    }
  ],
  "diagnostics": [],
  "resolution": {
    "source": "cached",
    "refreshed_repositories": []
  },
  "indexing": {
    "outcome": "skipped",
    "count": 0,
    "diagnostic": "preview only; indexing was not attempted"
  }
}
```

`--check` and `--dry-run` report the same validated target information; `--check` is the concise
inspection form and `--dry-run` is the explicit lifecycle preview form.

## Apply an update

```console
$ fastskill skill update alpha
Updating skills...

  • alpha: 1.0.0 -> 1.1.0 (upgrade)

[OK] Updated 1 skill(s)
   Updated skills.lock
   Indexing skipped: no embedding provider configured
```

The exact change labels can include `origin changed`, `resolved content changed`, `groups changed`,
and `dependency ownership changed`. Ownership changes are calculated per selected root, so an
unrelated root is not reported as updated when another root's closure changes.

## Version controls

```console
$ fastskill skill update alpha --to-version 1.4.2 --dry-run
$ fastskill skill update alpha --strategy patch
$ fastskill skill update alpha --repository team --strategy minor
```

An exact target can be a deliberate downgrade. `patch` stays within the current major/minor,
`minor` stays within the current major, and `latest`/`major` use the recorded constraint. An exact
pin is never widened implicitly.

## Global updates

```console
$ fastskill skill update --global --check
$ fastskill skill update alpha --global
```

Global JSON uses the same lifecycle keys as project JSON: `scope`, `outcome`, `dry_run`, `targets`,
`diagnostics`, `resolution`, and `indexing`. A missing global lock returns an unchanged document
with explicit skipped-indexing diagnostics rather than `null` lifecycle fields.

## Failure and recovery

Before mutation, fastskill verifies selected managed content and refuses to replace local edits.
Each affected closure is staged and validated. A failed application restores its prior files and
state, reports the failed target, and exits nonzero.

## See also

* [Project install](/cli-reference/install-command).
* [Skill and project commands](/cli-reference/skill-commands).
* [Reconciliation](/skill-management/reconciliation).


# Reconcile project skill state

FastSkill 0.9.256

Source: https://docs.gofastskill.com/skill-management/reconciliation

Release revision: 045bbbf81a9212b9129e560e7fc8276117c1a6c3

Documentation revision: 045bbbf81a9212b9129e560e7fc8276117c1a6c3



# Reconcile project skill state

fastskill compares three sources of truth:

1. `skill-project.toml` records direct intent, groups, and source constraints.
2. `skills.lock` records the resolved closure, versions, origins, ownership, and integrity data.
3. The skills directory contains the installed files an agent reads.

`fastskill skill list` joins those sources into one row per skill. `fastskill skill list --check`
uses the same rows and returns nonzero when selected managed state needs reconciliation.

## Human-readable checks

```console
$ fastskill skill list

ID     Name   Description  Flags
---------------------------------
alpha  alpha  Example      -

$ fastskill skill list --details

ID     Name   Description  Version  Manifest  Lock  Installed  Source Path  Type   Flags
-----------------------------------------------------------------------------------------
alpha  alpha  Example      1.0.0    Y         Y     Y          ./alpha     local  -
```

The detailed source path is the recorded origin path or URL, not the installed destination.
Editable installs use the human-readable `editable` flag.

## Reconciliation values

The `reconciliation` field is the canonical machine-readable status. Values can include:

| Value                    | Meaning                                                             |
| ------------------------ | ------------------------------------------------------------------- |
| `ok`                     | Selected state agrees and managed content passes integrity checks.  |
| `excluded`               | A group filter excluded the owning root.                            |
| `missing-lock`           | Declared intent has no resolved lock entry.                         |
| `missing-content`        | Managed content is absent from the skills directory.                |
| `intent-mismatch`        | Manifest intent and locked origin differ.                           |
| `revision-mismatch`      | Installed and locked versions differ.                               |
| `content-mismatch`       | Installed managed content differs from its digest.                  |
| `integrity-error`        | Installed content could not be verified.                            |
| `insufficient-integrity` | The lock lacks evidence required for a reliable check.              |
| `ownership-conflict`     | Bundle owners disagree about the expected content.                  |
| `extraneous`             | Installed content has no manifest, lock, bundle, or override owner. |

## JSON schema

```console
$ fastskill skill list --format json
[
  {
    "id": "alpha",
    "name": "alpha",
    "description": "Example",
    "version": "1.0.0",
    "in_manifest": true,
    "in_lock": true,
    "installed": true,
    "source_path": "./alpha",
    "source_type": "local",
    "missing_from_folder": false,
    "missing_from_lock": false,
    "missing_from_manifest": false,
    "desired_constraint": null,
    "locked_version": "1.0.0",
    "actual_version": "1.0.0",
    "reconciliation": "ok",
    "owners": ["alpha"],
    "groups": ["default"],
    "mutable": false,
    "override_active": false,
    "extraneous": false
  }
]
```

Do not test a nonexistent `.status` field. Use `reconciliation` for reports and use the
`--check` exit status for pass/fail automation.

## Group-scoped checks

`--only` and `--without` affect both displayed rows and the selected check closure. They are
mutually exclusive.

```console
$ fastskill skill list --check --only production
$ fastskill skill list --check --without dev
```

The implicit group for an ungrouped root is `default`. `--only default` is valid even for an empty
project and produces an empty successful result.

## CI example

```yaml title=".github/workflows/skill-health.yml"
name: Skill health
on: [push, pull_request]

jobs:
  reconcile:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install fastskill
        run: |
          curl -fsSL https://github.com/gofastskill/fastskill/releases/latest/download/install.sh | FASTSKILL_UNMANAGED=1 sh
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - name: Check managed skill state
        run: fastskill skill list --check --format json > skills-status.json
```

The last command fails the step when reconciliation is required. Keep the JSON artifact for
diagnostics; no `jq` predicate is required to decide success.

For a readable failure report while preserving the command's exit status:

```bash
fastskill skill list --check --format json > /tmp/skills-status.json
check_exit=$?

if [ "$check_exit" -ne 0 ]; then
  jq '[.[] | select(.reconciliation != "ok" and .reconciliation != "excluded") |
      {id, reconciliation}]' /tmp/skills-status.json
  exit "$check_exit"
fi
```

## Repair workflow

Use the row status to choose the repair:

```console
$ fastskill project install          # resolve/restore declared state
$ fastskill project install --lock   # restore the committed selection
$ fastskill skill update alpha       # deliberately refresh one origin
$ fastskill skill remove alpha       # detach one direct requirement
```

fastskill refuses to overwrite locally modified managed content during update/removal. Preserve or
revert the local change before retrying. Editable installs are intentionally mutable and are checked
by link shape rather than copied-content digest.

## Global state

Use `--global` to reconcile `global-skills.lock` and the global skills directory:

```console
$ fastskill skill list --global --check
```

Global checks treat an extraneous on-disk skill as a reconciliation failure, matching project
checks.


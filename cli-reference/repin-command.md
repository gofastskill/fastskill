# project repin

FastSkill 0.9.265

Source: https://docs.gofastskill.com/cli-reference/repin-command

Release revision: 2fd5a2e0f067c02a7b5fee0342d7842ae79c0d5c

Documentation revision: 2fd5a2e0f067c02a7b5fee0342d7842ae79c0d5c



# `project repin`

Rewrite legacy content digests in a project's records from the installed content. A legacy digest
is 64 bare hex characters, written by fastskill releases before the current `sha256-tree-v2` form.
fastskill still accepts one with a warning, but a future minor release will refuse it
([ADR-0017](https://github.com/gofastskill/fastskill/blob/main/docs/adr/0017-versioned-content-digests.md)).

`project repin` never changes skill content and never re-resolves anything. It only replaces a
legacy value whose installed content still matches it.

## Usage

```bash
fastskill project repin [OPTIONS]
```

## Options

| Option    | Description                                                  | Default |
| --------- | ------------------------------------------------------------ | ------- |
| `--check` | Change nothing; exit non-zero when any legacy digest remains | `false` |
| `--json`  | Emit one machine-readable JSON result                        | `false` |

The global `--skills-dir` option selects the skills directory to check. The global `--global` option
re-pins `global-skills.lock` in the user configuration directory instead of a project's records.

## What it rewrites

| Record         | File                                   | Value                                                   |
| -------------- | -------------------------------------- | ------------------------------------------------------- |
| Skill          | `skills.lock`                          | `[[skills]]` `resolved.checksum`                        |
| Bundle         | `skills.lock`                          | `[[bundles]]` release `digest` and each member `digest` |
| Override       | `skills.lock`                          | `[[overrides]]` `digest`                                |
| Bundle history | `.fastskill/bundle-history.toml`       | Each `[releases]` digest                                |
| Global skill   | `global-skills.lock` (with `--global`) | `[[skills]]` `resolved.checksum`                        |

For each legacy value, the command checks the installed content against it, computes the current
form from that same content, and writes it. Every other byte of each file is kept, including
comments and formatting.

* A skill or override is checked against its installed directory.
* A bundle is re-pinned only when every legacy member digest matches: an overridden member is
  checked against the cached bundle artifact, every other member against its installed directory.
  The release digest is recomputed from the new member digests.
* A bundle history release is checked against its cached artifact in `.fastskill/bundles/`, or,
  when that is missing, against the bundle's Lock entry.

A legacy value whose content is not installed, or whose installed content no longer matches it, is
left as it is and reported with the reason. Run `fastskill project install` to restore the content,
then run `project repin` again.

Bundle artifacts are immutable, so the command does not rewrite them. It reports each artifact
that still declares legacy member digests (in `.fastskill/bundles/`, in the Lock, or in the
manifest's `[bundles]`). Rebuild each one with `fastskill bundle build`.

## Examples

### Re-pin a project

```bash
fastskill project repin
```

```text
Re-pinned 2 legacy content digest(s):
  skill code-review (skills.lock)
  bundle team@1.0.0 (skills.lock)
```

### Find legacy digests in CI

```bash
fastskill project repin --check
```

`--check` reports what a re-pin would do and changes nothing. It exits non-zero while any legacy
value remains, so a CI job fails until the records are re-pinned and committed.

### Re-pin the global Lock

```bash
fastskill --global project repin
```

### Machine-readable output

```bash
fastskill project repin --check --json
```

The result uses the lifecycle envelope shared by the other project commands: `scope`, `outcome`
(`changed`, `unchanged`, or `blocked`), `dry_run`, and one `targets` entry per legacy value with its
`record`, `file`, `current_revision` (the legacy value), `target_revision` (the current form, when it
can be re-pinned), `changes`, and `retained` reasons. `legacy_artifacts` and `diagnostics` list the
bundle artifacts to rebuild.

## Exit codes

| Code | Meaning                                                                                      |
| ---- | -------------------------------------------------------------------------------------------- |
| `0`  | No legacy digest remains (after a re-pin, or none was found)                                 |
| `1`  | A legacy value remains: under `--check`, or an entry left as is, or a legacy bundle artifact |
| `2`  | No `skill-project.toml` was found, or a file could not be read or written                    |

A project without `skills.lock` has nothing to re-pin and exits `0`.

## See Also

* [project install](/cli-reference/install-command) - Restore the content a legacy value names
* [bundle commands](/cli-reference/bundle-command) - Rebuild a bundle artifact with `bundle build`


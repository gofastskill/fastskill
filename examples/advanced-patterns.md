# Advanced Patterns

FastSkill 0.9.229

Source: https://docs.gofastskill.com/examples/advanced-patterns

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



## Overview

These are real, source-backed workflows for teams managing many skills. Every command below exists in
the CLI.

## Find duplicate and overlapping skills

As a collection grows, skills drift into overlap. `fastskill analysis` surfaces that:

```bash
# Similarity matrix across installed skills
fastskill analysis matrix

# Cluster related skills
fastskill analysis cluster

# List likely duplicate/overlapping pairs, with a merge suggestion per pair
fastskill analysis duplicates
```

`analysis duplicates` supports filtering and output control:

```bash
fastskill analysis duplicates --threshold 0.85 --limit 20 --severity high --format json
```

| Flag          | Purpose                                                         |
| ------------- | --------------------------------------------------------------- |
| `--threshold` | Minimum similarity to report                                    |
| `--limit`     | Maximum number of pairs to show                                 |
| `--severity`  | Filter by minimum severity: `critical`, `high`, `medium`, `all` |
| `--format`    | `table`, `json`, `grid`, or `xml` (`--json` is shorthand)       |

## Improve a skill with `optimization`

`fastskill optimization` runs a text-gradient loop to iteratively improve a skill's content:

```bash
fastskill optimization run --config ./optimize.toml                    # start a run
fastskill optimization status ./optimize-runs/<run> --watch            # check progress
fastskill optimization resume ./optimize-runs/<run>                    # resume an interrupted run
fastskill optimization inspect ./optimize-runs/<run> --step 3          # inspect one step
fastskill optimization export ./optimize-runs/<run> --out ./SKILL.md   # export the best skill
```

## Install only part of a collection with groups

Skills can be tagged into groups; installs can include or exclude them (poetry-style):

```bash
# Only install skills in the "docs" and "core" groups
fastskill project install --only docs --only core

# Install everything except the "dev" group
fastskill project install --without dev
```

## Editable and recursive local skills

For local skill development, add without copying and add whole trees at once:

```bash
# Editable install: symlink/reference so in-place edits apply without reinstall (local only)
fastskill skill add ./dev-skill -e

# Recursively add every subdirectory containing a SKILL.md (local trees only)
fastskill skill add ./skills -r
```

After editing an editable skill in place, run `fastskill index rebuild` so semantic search and resolve
reflect the changes.

## Project vs. global scope

Most commands default to the current project. Use `--global` to operate on the shared user-level
skills directory (`~/.config/fastskill/skills`, tracked by `global-skills.lock`):

```bash
fastskill skill add pptx --global
fastskill skill list --global
fastskill project install --global --lock
```

Use `analysis` to keep a collection tidy, `optimization` to sharpen individual skills, and group
filters
plus `--global` to control what gets installed where.


# managed Command

FastSkill 0.9.272

Source: https://docs.gofastskill.com/cli-reference/managed-command

Release revision: aaf72a3635a29208e6b04d1062cf1e290941025f

Documentation revision: aaf72a3635a29208e6b04d1062cf1e290941025f



# managed Command

Follow a **managed state**: a signed list of the skills one user's agents on this machine
should have, decided somewhere else. FastSkill checks its signature against keys pinned in
trusted settings, then deploys the listed skills into each agent's user skills folder, removes
the ones it put there that are no longer listed, and moves blocked content into a quarantine
instead of deleting it.

Only FastSkill's own entries are changed. A skill folder that FastSkill didn't place and that
holds other content than the state lists is reported as a collision and left alone.


## Usage

```bash
fastskill managed <SUBCOMMAND>
```

## Settings

The source and the signing keys come only from two files, never from a project:

* the system file, deployed by device management: `/etc/fastskill/managed.toml` (Linux),
  `/Library/Application Support/FastSkill/managed.toml` (macOS) or
  `%ProgramData%\FastSkill\managed.toml` (Windows). It must be owned by the administrator and
  writable by nobody else;
* the user file, `managed.toml` in FastSkill's configuration folder.

A value in the system file wins over the same value in the user file.

```toml
source = "/srv/fastskill/state.dsse"

[[keys]]
id = "k1"
public_key = "BASE64-ED25519-PUBLIC-KEY"

# Optional: the agents to cover instead of those detected for this user.
targets = ["claude"]
# Optional, system file only: refuse to unenroll.
required = true
```

## Sources

The source is either a local file or an `https://` URL.

* **A file source** reads the state from a path. The skill archives it lists are read from paths
  next to it, or from absolute paths.
* **An `https://` source** fetches the state from the URL, and each listed skill archive from
  its `https://` URL. Redirects to anything but `https://` are refused. The state can be at most
  8 MiB and each archive at most 512 MiB.

When the source needs a sign-in, name a credential command:

```toml
source = "https://skills.example.com/state"
credential_command = ["example-login", "token"]
```

The command runs without a shell, from FastSkill's configuration folder, for at most 30
seconds. The first line it prints, at most 16 KiB, is the token, sent as
`Authorization: Bearer <token>`. The token is never stored or printed, and it is sent only to
the source's origin (scheme, host and port): a request to another origin, including a redirect
to one, goes without it. When there is a terminal, the command can prompt on it; without one,
such as from an agent hook, `FASTSKILL_INTERACTIVE=0` is set so it can fail at once instead of
waiting.

When the command fails, or the source answers `401`, apply uses the last accepted state and
reports **sign-in needed**; run `fastskill managed apply` in a terminal to sign in again.

### Fields only an `https://` source honors

| State field   | What it does                                                                                                                         |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `report_url`  | Where each apply sends its report. Must be on the source's origin.                                                                   |
| `request_url` | A link template with `{digest}`, shown when an install is refused, so people can ask for the skill. Must be on the source's origin.  |
| `editable`    | `blocked-only` (default) lets editable local skills skip the allow-list; `refused` refuses them when only listed skills are allowed. |

A `report_url` or `request_url` on another origin is ignored with a warning. A file source
ignores all three, and `fastskill managed status` names each one it ignores.

## The report

When the state names a `report_url`, each apply sends a JSON report to it, signed in with the
same token. The body is fixed and holds no prompts, file contents, paths, usernames, project
names or repository URLs:

* `format_version`, `machine_id` and `sequence`, which goes up by one with each report;
* `issued_at` of the state applied and whether the apply `completed`;
* the covered `agents`, by name;
* every skill entry in the agent folders: its id, digest, whether FastSkill placed it
  (`origin_kind`), whether it is an editable local skill, and its outcome (`deployed`,
  `adopted`, `collision`, `quarantined` or `unmanaged`);
* the `quarantine`, and what this apply quarantined (`quarantined_now`), by id, digest and
  reason;
* `refusals`: installs the state refused since the last accepted report, by digest and command
  name, never its arguments;
* `project_skills`: the ids and digests in the current project's skills folder.

`fastskill managed status --json --report` prints the body the next report would carry, without
sending it.

## Subcommands

### enroll

Enroll this user with the configured source and apply its first state. Enrolling again with the
same source keeps the machine id.

```bash
fastskill managed enroll
```

### apply

Apply the newest signed state. When the system file names the source, the first apply enrolls
this user. Without network access to the source, the last accepted state is used and the report
says so. An expired state adds, changes and removes nothing, but blocked content is still
quarantined.

```bash
fastskill managed apply
fastskill managed apply --json
```

Only one apply runs at a time for a user. A second apply started from a terminal says so; one
started without a terminal, such as from an agent hook, exits quietly. An apply that couldn't
complete every step exits with an error after its report.

### status

Show the source, the machine id, the state's subject and expiry, whether a sign-in is needed,
the fields this source ignores, where reports go, the last apply's targets, collisions and
warnings, and the quarantine. Nothing is changed.

```bash
fastskill managed status
fastskill managed status --report          # include everything the last apply did, and the report body
fastskill managed status --json
fastskill managed status --json --report   # only the body the next report carries
```

### unenroll

Remove what managed state put on this machine for this user: the skills FastSkill deployed (a
deployed copy that was changed is quarantined instead), the stored skills, the cached state,
the records and the user settings file. The quarantine is kept. Refused when the system file
sets `required = true`, and while an apply runs.

```bash
fastskill managed unenroll
```

**Options** (every subcommand):

* `--json`: Print the result as JSON

## Installs and other commands

While this user follows a state, every command that adds or replaces skill content checks it
first, before anything changes: `skill add`, `skill update`, `bundle add`, `bundle update`,
`bundle override` (and resetting one), `project install` and restoring personal overrides. The
same check applies to `server serve` and `mcp serve`. Content whose digest is blocked is
refused, and when the state allows only the listed skills, so is anything it doesn't list. An
editable local skill changes with every edit, so it skips the list, but never the block.

Other commands that read or change skills follow the state too:

| Situation                         | `managed`, `cache`, `repo`, `cli` | Commands that read skills | Commands that change content |
| --------------------------------- | --------------------------------- | ------------------------- | ---------------------------- |
| Expired state                     | run                               | run, with a warning       | refused                      |
| No valid state, `required = true` | run                               | refused                   | refused                      |
| No valid state, not required      | run                               | run, with a warning       | run, with a warning          |

`skill list` reports installed content the state blocks as `managed-blocked`, and content it
doesn't allow as `managed-not-allowed`; `skill list --check` fails on both. See
[reconciliation](/skill-management/reconciliation).

## The quarantine

Content is moved, not deleted, when its digest is blocked, when the state allows only the
listed skills and claims the agent folders exclusively, or when a copy FastSkill deployed was
changed and is no longer listed. Each item keeps its former location, its digest and the
reason, and `fastskill managed status` lists them.


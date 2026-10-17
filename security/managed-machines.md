# Hardening managed machines

FastSkill 0.9.274

Source: https://docs.gofastskill.com/security/managed-machines

Release revision: 605bd729da6c4407ae8c7d9807a0f4ca6045adb6

Documentation revision: 605bd729da6c4407ae8c7d9807a0f4ca6045adb6



# Hardening managed machines

[`fastskill managed`](/cli-reference/managed-command) is cooperative configuration management
with integrity checks. It is not a sandbox. This page says where FastSkill enforces a
[managed state](/skill-management/managed-state), where it can't, and what an administrator pins so
that following the state is the default path on every machine.

## What FastSkill enforces

FastSkill enforces the state at its own operations, and nowhere else:

* **`managed apply`** deploys the listed skills into each agent's user skills folder, removes the
  ones it placed that are no longer listed, and moves blocked content into the quarantine. With
  `allowed = listed` and `exclusive_targets = true`, content in an agent folder that the state
  doesn't allow is quarantined too.
* **Installs** are checked by digest before anything changes: `skill add`, `skill update`,
  `bundle add`, `bundle update`, `bundle override`, `project install` and restoring personal
  overrides, and the same through `server serve` and `mcp serve`. Blocked content is refused;
  with `allowed = listed`, so is anything the state doesn't list.
* **`skill list --check`** fails on `managed-blocked` and `managed-not-allowed`.

What this protects against:

* a state that was altered, replayed, or issued for another source or another subject, whether
  it arrives over the network, from a cache, from a file or git source, or from a mirror
* a repository that tries to change the managed source, or run a command, through project
  configuration: managed settings in a project file are refused, with an error naming the file
* other users on the same machine who aren't its administrator
* a user installing blocked content by mistake, directly or through a project

What it doesn't protect against:

* **The machine's administrator.**
* **The user acting deliberately.** They can copy files into an agent folder between applies,
  move the clock, or start an agent without hooks. `required = true` makes following the state
  the default path and refuses FastSkill's own operations until a valid state is cached. It
  doesn't stop a determined user. Hard enforcement means configuring the agent itself, through
  device management, to load only the folders FastSkill manages; few agents can be configured that
  way today (see [Agents](#agents)).
* **Whoever holds a pinned signing key.** A key holder decides what enrolled machines install.

## How fast a block lands

A blocked digest reaches a machine at its next successful apply: at login, hourly, and at every
agent session start where a hook exists. A machine that is on and can sign in applies it within
about 70 minutes, and its agents drop the skill from their next session. A machine that stops
applying keeps what it has, however long that lasts: off, offline, or waiting for a sign-in.
Expiry limits how long FastSkill keeps allowing installs under an old state, not how long
installed content stays. The [report](/cli-reference/managed-command#the-report) tells a source
which machines haven't applied recently.

## The system file

Device management deploys the system file at a fixed location:

| Platform | Path                                                  |
| -------- | ----------------------------------------------------- |
| Linux    | `/etc/fastskill/managed.toml`                         |
| macOS    | `/Library/Application Support/FastSkill/managed.toml` |
| Windows  | `%ProgramData%\FastSkill\managed.toml`                |

FastSkill honors it only when the file and its folder are owned by the administrator account
(root, or Administrators or SYSTEM) and nobody else can write to them; otherwise it refuses to run
and names the problem. A value in the system file wins over the same value in the user file, and
no managed setting is ever read from a project, an environment variable or a flag.

```toml
source = "https://skills.example.com/state"
credential_command = ["/usr/local/bin/example-login", "token"]

[[keys]]
id = "k1"
public_key = "BASE64-ED25519-PUBLIC-KEY"

# Refuse to unenroll, and refuse commands that read or change skills until a valid state is cached.
required = true

# Optional: cover these agents instead of the ones detected for each user.
targets = ["claude", "gemini"]
```

When the system file names the source, the first apply enrolls each user: nobody has to run
`fastskill managed enroll`. With `required = true`, `fastskill managed unenroll` is refused.

## Keys

Pin the verification keys in the system file, so that no user can add one. Pin a new key before
the source signs with it, and unpin a key to revoke it: FastSkill verifies the cached state every
time it uses it, so a state signed only by an unpinned key stops counting at the next command.
Keep the private key where the machines never reach it. FastSkill never needs it; only the source
signs.

## The credential command

When the source needs a sign-in, name the credential command in the system file by absolute
path, pointing at a program only the administrator can change. It runs without a shell, and the
token it prints goes only to the source's origin. From a hook or the timer there is no terminal,
so `FASTSKILL_INTERACTIVE=0` is set and the command should fail at once instead of prompting; the
user's next `fastskill managed apply` from a terminal signs in again.

## Administrator-level hooks

`fastskill managed hooks --system` prints, for each agent that runs a command at session start,
the entry to merge into the agent's administrator-level settings. Device management places those
files; FastSkill never writes them. When an administrator-level FastSkill hook is present for an
agent, apply registers no user-level one, so there is no hook for the user to approve.

| Agent       | Linux                                    | macOS                                                           | Windows                                             |
| ----------- | ---------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------- |
| Claude Code | `/etc/claude-code/managed-settings.json` | `/Library/Application Support/ClaudeCode/managed-settings.json` | `C:\Program Files\ClaudeCode\managed-settings.json` |
| Gemini CLI  | `/etc/gemini-cli/settings.json`          | `/Library/Application Support/GeminiCli/settings.json`          | `C:\ProgramData\gemini-cli\settings.json`           |

```bash
fastskill managed hooks --system
fastskill managed hooks --system --json
```

## The machine-wide timer

The same command prints the machine-wide timer: the files to place, and the commands to run
after placing them. When a machine-wide timer is present, apply installs no per-user one.

| Platform | What device management places                                                                        |
| -------- | ---------------------------------------------------------------------------------------------------- |
| Linux    | `/etc/systemd/user/fastskill-managed-apply.timer` and `.service`, a systemd user unit for every user |
| macOS    | `/Library/LaunchAgents/dev.fastskill.managed-apply.plist`, loaded at each user's next login          |
| Windows  | `C:\ProgramData\fastskill\managed-apply-task.xml`, registered with `schtasks`                        |

Each run applies at login and then every hour, after a random wait of up to ten minutes, so a
fleet doesn't fetch all at once.

## Agents

Hard enforcement lives in the agent: one configured, through device management, to load skills
only from the folders FastSkill manages. What each agent allows, as checked against its
documentation on 2026-09-24. "Shared folder" says whether the agent reads `~/.agents/skills`;
"same session" says whether a skill written by its session-start hook shows up in that session.

| Agent                    | Shared folder | Command hook at session start            | Administrator-level hooks       | Same session |
| ------------------------ | ------------- | ---------------------------------------- | ------------------------------- | ------------ |
| Claude Code              | no            | yes; output reaches the model            | managed settings                | no           |
| Cursor (IDE and CLI)     | yes           | yes, not awaited; output not shown       | `/etc/cursor/hooks.json`        | no           |
| Codex CLI                | yes           | yes; user hooks need approval            | managed hooks, no approval      | later        |
| Gemini CLI               | yes           | yes; output must be JSON                 | `/etc/gemini-cli/settings.json` | no           |
| Qwen Code                | yes           | yes                                      | `/etc/qwen-code`                | later        |
| GitHub Copilot (VS Code) | yes           | yes; only JSON context reaches the model | GitHub policy                   | no           |
| GitHub Copilot CLI       | yes           | yes; 30 s timeout                        | `/etc/github-copilot/policy.d`  | no           |
| Devin Desktop            | yes           | yes                                      | `/etc/devin/hooks.json`         | no           |
| Auggie                   | yes           | yes                                      | `/etc/augment/settings.json`    | no           |
| Kiro, CodeBuddy, Qoder   | no            | yes                                      | not checked                     | no           |
| IBM Bob                  | no            | yes; 10 s timeout                        | enforced-hooks policy           | no           |
| OpenCode, Amp, Pi, Kilo  | yes           | only through a plugin                    | not checked                     | no           |
| Roo Code (archived)      | yes           | none                                     | none                            | no           |

"Later" means the agent watches its skills folders and picks the skill up during the session.
FastSkill registers session-start hooks where it has verified support (today Claude Code and
Gemini CLI); every other agent is kept up to date by the timer. Agents add hooks and folders
often, so check the agent's current documentation before relying on a row.

## Several users on one machine

The system file is machine-wide. Everything else is per user: the managed store, the machine id,
the cached state, the quarantine, the hooks and the timer. Each user enrolls and reports
separately.


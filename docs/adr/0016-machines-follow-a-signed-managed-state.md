# Machines can follow a signed managed state; FastSkill applies it and never decides it

Status: proposed. Date: 2026-09-24. Revised: 2026-09-24. Amended: 2026-09-24.

Related: [ADR-0001](0001-remove-sync-command.md),
[ADR-0003](0003-serve-trust-boundary-and-edge-auth.md),
[ADR-0005](0005-install-seam-and-origin-model.md),
[ADR-0006](0006-skill-deployment-boundary.md),
[ADR-0008](0008-bundle-ownership-and-local-changes.md),
[ADR-0010](0010-command-taxonomy.md),
[ADR-0014](0014-skill-identity-comes-from-skill-content.md),
[ADR-0015](0015-manifest-composition-and-bundle-exports.md), and
[team-skill-presets](../requirements/team-skill-presets.md).

**Revision.** An adversarial review of the first version found gaps that this version closes:

- It never said what a signature covers. An older state, or a state issued for someone else, would
  have been accepted.
- It read managed settings from configuration that a repository controls.
- `required = true` had no way to bootstrap.
- It had no rules for collisions, concurrent applies, interrupted applies or path safety.
- It claimed a withdrawal bound that expiry doesn't give.
- It left `bundle override` outside the install gate.
- It changed ADR-0001, ADR-0003 and ADR-0008 without saying so. Those amendments are now explicit,
  and each of those ADRs records them.

**Amendment.** A later design pass added what a source serving many users needs, and tightened
one rule for every source:

- Blocked content is refused and quarantined even when it is an editable local skill, whatever the
  source (decisions 11 and 13).
- Three additions apply only when the managed source is an `https://` URL (decision 21): an
  `editable` setting (decision 22), a request link printed on refusals (decision 23), and two
  report additions (decision 15). A file source ignores them, and with no managed source nothing
  changes.
- Legacy content digests get a way to be rewritten and a date after which they are refused,
  recorded in ADR-0017.

## Context

team-skill-presets started from one need: a team keeps using incorrect or outdated skills, and
keeping it current means asking each person to update. Bundles and Manifest composition reduced
that work, but a skill still reaches a machine only when someone there runs a command. ADR-0006
gave FastSkill the desired skill setup, its installation, verification, updates and rollback, and
explicitly left "fleet rollout mechanisms" unsettled.

Three things are still missing:

- **Rollout.** A set of skills decided somewhere else doesn't reach machines by itself.
- **Withdrawal.** When a skill turns out to be harmful, nothing stops it being installed again,
  and nobody can tell which machines have it.
- **Agent visibility.** Agents read skills from per-user folders such as `~/.claude/skills` and
  `~/.cursor/skills`. By default the global installation scope installs into FastSkill's own
  folder, which no agent reads. `--skills-dir` can point one installation at one agent's folder,
  but nothing keeps several agents' folders current.

Earlier decisions constrain the answer:

- ADR-0003: FastSkill holds no identity and is not a security boundary.
- team-skill-presets Q9 and Q14: login and credential management stay outside FastSkill.
- team-skill-presets Q4: a managed launcher can't be relied on for enforcement, and execution
  restrictions belong to the agent runtime.

Who decides what a machine should have varies a lot. It may be one person editing a file in git,
a CI job, device management, or a service that knows every user. FastSkill shouldn't need to know
which, and shouldn't have to change when that decision-maker's rules change.

## Threat model

This feature is cooperative configuration management with integrity checks. It is not a sandbox.

It protects against:

- a managed state that was altered, replayed, or issued for another source or another subject,
  whether it arrives over the network, from a cache, from a file or git source, or from a mirror
- a repository that tries to change the managed source, or run a command, through project
  configuration
- other users on the same machine who aren't its administrator
- a user installing blocked content by mistake, directly or through a project

It doesn't protect against:

- **The machine's administrator.**
- **The user acting deliberately.** They can copy files into an agent folder between applies,
  move the clock, or start an agent without hooks. `required = true` makes following the state
  the default path and refuses FastSkill's own operations. It doesn't stop a determined user.
  Hard enforcement means configuring the agent runtime, through device management, to load only
  managed folders.
- **Whoever holds a pinned signing key.** A key holder decides what enrolled machines install.
  Keeping that key safe is the managed source operator's responsibility.

## Decision

### The managed state

1. **A managed state is a signed, resolved description of what one user's agents on one machine
   should have.** It contains:
   - a format version
   - `issued_at` and `expires_at`
   - `source`: the managed source it was issued for (decision 3)
   - `subject`: an opaque label that FastSkill displays and compares for equality, but never
     interprets
   - `skills`: each entry has an id, a content digest and an artifact location
   - `allowed`: either `listed`, meaning only digests in `skills` or `also_allowed` may be
     installed, or `any`
   - `also_allowed`: extra digests that may be installed but aren't installed by default
   - `blocked`: digests that must not be present, each with an optional message for the user
   - `report_url`: optional (decision 15)
   - `exclusive_targets`: a flag (decision 11)
   - `editable`: optional, `blocked-only` or `refused` (decision 22)
   - `request_url`: optional, a link template (decision 23)

   Entries are resolved content, like a Lock, not intent, like a Manifest. Applying a managed
   state never resolves versions, reads repositories or runs dependency resolution. The state is
   closed: `managed apply` installs exactly its `skills`. A skill whose declared dependencies
   aren't in the state is still installed, and apply reports the missing dependencies.

   FastSkill refuses a whole managed state when any of these is true:
   - its format version is one FastSkill doesn't support
   - a digest is in `blocked` and also in `skills` or `also_allowed`
   - two `skills` entries share an id
   - an id isn't a safe skill id (one path component, under ADR-0014's rules)
   - a digest isn't in FastSkill's current content digest format
     ([ADR-0017](0017-versioned-content-digests.md)). The original digest didn't
     length-frame file contents, so two different trees could share it. It is never accepted
     here.
   - an artifact location, `report_url` or `request_url` isn't `https://` (a file source may use
     local paths)
   - `editable` has a value other than `blocked-only` or `refused`

   When it installs a skill, apply requires that the downloaded content's digest equals the
   entry's digest and that its own identity (ADR-0014) equals the entry's id.
2. **Every managed state is signed, and the keys are pinned locally.**
   - **Envelope.** A managed state travels in a DSSE envelope with the payload type
     `application/vnd.fastskill.managed-state+json`. The signature covers the exact payload bytes
     and the payload type, through DSSE's pre-authentication encoding. No JSON is canonicalized.
   - **Keys.** Signatures are Ed25519. Each one names a key id, and a pinned key is a key id plus
     a public key. FastSkill accepts a state if at least one signature verifies against a pinned
     key.
   - **Key source.** Verification keys come only from trusted configuration (decision 7). They are
     never fetched from the managed source.
   - **Rotation and revocation.** Several keys may be pinned at once, so a new key can be pinned
     before the source starts using it. Unpinning a key revokes it. FastSkill verifies the cached
     state every time it uses it, so a cached state signed only by an unpinned key stops counting
     at once.

   The signature is what lets a state be cached, carried offline, or served from a git repository
   or object storage without trusting the transport.
3. **A managed state is bound to its source and subject, and never goes backwards.**
   - **Source.** `source` must equal the configured managed source. A state issued for another
     source is refused, even when it is signed by a pinned key.
   - **Subject.** FastSkill records the `subject` of the first state it accepts after enrollment.
     Later states with a different subject are refused until `managed enroll` runs again. A state
     issued for one user therefore can't be swapped in for another's.
   - **No rollback.** FastSkill records the highest `issued_at` it has accepted and refuses any
     state issued earlier. Receiving the same state again is fine.
   - **Clock skew.** FastSkill refuses a state whose `issued_at` is more than five minutes ahead
     of the local clock.
4. **A managed state expires.**
   - FastSkill keeps the last valid state and uses it offline until it expires.
   - After expiry, FastSkill refuses operations that add or change skill content.
   - It never removes installed skills because of expiry alone.
   - Expiry is checked against the local clock.

   What each command does, by situation:

   | Situation | `managed` commands | Commands that read skills | Commands that add or change skill content |
   |---|---|---|---|
   | No managed source configured | not enrolled | as today | as today |
   | Valid, unexpired state | run | run | gated (decision 13) |
   | State expired | run | run, with a warning | refused |
   | No valid state, `required = true` | run | refused | refused |
   | No valid state, not required | run | run, with a warning | as today, with a warning |

   The `managed` commands always run, because `managed apply` is how a machine recovers.
   `managed enroll` fetches and caches the first state before it reports success. If the source
   can't be reached, enroll still saves the configuration, and `managed status` shows that no
   state is cached.

### Getting it

5. **A managed source is one location**: an `https://` URL or a local file path. FastSkill fetches
   it and nothing more. Whatever produces the state (a static signed file, a CI job, a service) is
   outside FastSkill.
6. **Credentials come from an external command.** When the source needs authorization, trusted
   configuration names a credential command.
   - **How it runs.** The command is an executable and an argument list, run directly, without a
     shell. It runs from FastSkill's configuration directory, not from the current project, and
     has a 30-second timeout.
   - **The token.** It is the first line of the command's standard output, which may be at most
     16 KiB. FastSkill uses it for the requests of that one command run (the state, its artifacts
     and its report). It never obtains, refreshes, stores, logs, prints or inspects the token.
   - **Standard error** goes to the terminal when there is one, so the command can prompt for a
     sign-in. FastSkill never records it.
   - **Where the token goes.** It is sent only to the managed source's origin (scheme, host and
     port). Artifact locations on that origin get it. Artifact locations on other origins get no
     token and must carry their own authorization, such as a pre-signed URL. A redirect to another
     origin drops the token, and a redirect from `https` to `http` is refused.
   - **Reports.** `report_url` must be on the source's origin. If it isn't, apply sends no report
     and warns. A file source has no origin, so it never gets a report.
   - **Request links.** `request_url` must be on the source's origin too. If it isn't, FastSkill
     prints no link and apply warns.
   - FastSkill gains no login command.
7. **Managed settings come only from trusted configuration.**
   - **The settings:** the source, the pinned keys, the credential command, `required`, and the
     Agent target list (decision 8).
   - **Where they are read.** They are read from two files only: the system configuration file
     and the user's FastSkill configuration file.
   - **Where they are never read:** a project's `skill-project.toml`, environment variables and
     flags. A project file that contains managed settings is refused, with an error naming the
     file, so a repository can't redirect the source or choose a command to run.
   - **The system file.** Device management deploys it at a fixed platform location:
     - `/etc/fastskill/managed.toml` on Linux
     - `/Library/Application Support/FastSkill/managed.toml` on macOS
     - `%ProgramData%\FastSkill\managed.toml` on Windows

     FastSkill honors it only when both the file and its folder are owned by the administrator
     account (root, or Administrators or SYSTEM) and nobody else can write to them. Otherwise
     FastSkill refuses to run and names the problem.
   - **Precedence.** A setting in the system file overrides the same setting in the user file.
     With `required = true` in the system file, commands that read or change skills refuse to run
     until a valid, unexpired managed state is cached (decision 4).

### Applying it

8. **Agent targets come from `aikit-sdk`.**
   - An Agent target is one agent's per-user skills folder.
   - By default, an agent is a target when `aikit-sdk` knows it and its per-user configuration
     folder exists.
   - The `targets` setting can list agents explicitly, replacing detection.
   - Agents `aikit-sdk` doesn't know are never targets.
9. **`managed apply` installs through a private staging area and deploys only what it owns.**
   - **Download and verify.** Each listed skill whose digest isn't already in the user-level
     **managed store** is downloaded into a staging folder only that user can access.
   - **Extract.** It is extracted under FastSkill's existing archive rules: no links, no paths
     outside the folder, and size limits. Its digest and identity are then checked (decision 1).
   - **Publish.** Only after those checks is it renamed into the store under its digest. Store
     entries are never modified after publication, and apply re-verifies an entry's digest before
     it deploys it.
   - **Deploy.** Deploying puts the skill at `<target>/<id>`. On Unix it is a symbolic link. On
     Windows it is a symbolic link where the user may create one, and otherwise a verified copy.
     Apply never writes through an existing link at the destination: it builds the new entry
     beside the old one and renames it into place.
   - **Ownership record.** FastSkill records every target entry it creates, with its digest. It
     replaces or removes only entries in that record.
10. **Collisions and removal are resolved without guessing.**
    - **A folder that FastSkill doesn't own.** If `<target>/<id>` exists but isn't in the
      ownership record:
      - If its digest equals the listed digest, apply adopts it into the record.
      - If decision 11 requires quarantining it, apply quarantines it, then deploys the managed
        skill.
      - Otherwise apply leaves it where it is, doesn't deploy the managed skill to that target,
        and reports a collision.
    - **A skill no longer listed.** Apply removes it from every target and from the store.
    - **A modified managed copy.** A copied entry that was modified locally is quarantined before
      it is replaced or removed. It is never silently overwritten or deleted. This amends ADR-0008
      for managed deployments only.
    - **Project skills.** The current project's skills folder is not an Agent target, and apply
      deploys nothing there. If a project skill shares an id with a managed skill, apply reports
      it. Which one an agent loads is the agent's rule.
11. **Quarantine moves content aside and never deletes it.**
    - **What is quarantined:**
      - content in an Agent target, or in the current project's skills folder, whose digest is
        `blocked`
      - with `allowed = listed` and `exclusive_targets = true`, content in an Agent target whose
        digest isn't allowed
    - **Editable local skills** (ADR-0005) are quarantined only when their digest is `blocked`.
      Blocking is a judgement about content, and a local path doesn't change it. Any other editable
      local skill is reported, never quarantined, whatever `editable` says (decision 22).
    - **Where it goes.** Quarantine moves content into a timestamped folder in FastSkill's
      per-user data folder, which only that user can access. Each item carries a record of its
      digest, its former location and the reason.
    - **Seeing and restoring it.** `managed status` lists what is currently in quarantine.
      Restoring an item is a documented manual move. Restored content that is still blocked is
      quarantined again at the next apply.
12. **Applies don't overlap, and an interrupted apply converges.**
    - **One at a time.** Only one apply runs per user at a time, guarded by a lock in the managed
      store. A second apply started from a hook exits successfully without doing anything. One
      started from a terminal says that an apply is already running.
    - **Never half-written.** Each skill changes from old to new by a rename, so a single skill is
      never half-written.
    - **Not atomic across skills or agents.** An interrupted apply can leave some skills or agents
      on the previous state. The next apply finishes the work.
    - **Reporting it.** The report says which state was applied and whether the apply completed.
13. **Installs are gated by digest at the install seam.**
    - **Where.** The gate lives in the core install seam (ADR-0005), so the CLI, `server serve`
      and `mcp serve` all inherit it.
    - **What it covers.** Every operation that adds or replaces skill content: `skill add`,
      `skill update`, `bundle add`, `bundle update`, `bundle override`, restoring personal
      overrides, and `project install`.
    - **What it refuses.** Content whose digest is `blocked`, or, under `allowed = listed`, not
      allowed. It refuses before changing any state and names the digest and its message.
    - **Editable local skills.** A blocked digest is refused for an editable local skill too,
      whatever the source. Whether `allowed = listed` applies to one is decision 22.
    - **What a refusal says.** A refusal for a digest that isn't allowed includes the request link
      (decision 23). A refusal for a blocked digest never does. The refusal is recorded for the
      next report (decision 15).
    - **Manifests and Locks.** Committed Manifests and Locks keep working unchanged. The gate only
      decides whether their pinned content may be installed on this machine.
14. **The reconciliation vocabulary grows by two statuses.**
    - `managed-blocked` and `managed-not-allowed` are emitted only while a managed state is
      active.
    - A skill still gets exactly one status. `managed-blocked` takes precedence over
      `managed-not-allowed`, and both take precedence over the eleven existing statuses.
    - `skill list --check` fails on both.
    - Agent target contents are shown by `managed status`, not by `skill list`.
    - With no managed state, the existing statuses and their meanings don't change.
15. **Reporting is fixed and inspectable.** If the state names a `report_url`, `managed apply`
    sends it one report and nothing else. The report contains:
    - a report format version
    - a random machine id, created for the user at enrollment and discarded at unenrollment, never
      derived from hardware
    - a sequence number that increases with each report from that machine id
    - the `issued_at` of the applied state, and whether the apply completed
    - the Agent targets, by agent name rather than by path
    - for each skill in an Agent target: its id, digest, Origin kind, whether it is editable, and
      its outcome (deployed, adopted, collision, quarantined or unmanaged)
    - what is currently in quarantine, and the quarantine actions taken in this run
    - the refusals since the last accepted report: for each, the digest and the command that was
      refused (`skill add`, `bundle update`, `project install` and so on), never its arguments.
      They are kept per user, deduplicated by digest and command, and cleared when the source
      accepts a report.
    - the skills in the current project's skills folder when apply ran: each one's id and digest.
      The current project is the one found from apply's working directory; with none, the list is
      empty. Its path, its name and its repository URL are never sent.

    Prompts, file contents, file paths, usernames, project names, repository URLs and usage are
    never sent. The last two items are part of the report because a source that blocks content
    needs to know which projects still carry it and what people asked for and were refused.
    `managed status --json --report` prints the exact report body without sending it. A report is
    the machine's own claim, not proof. Whoever receives it should treat it that way.
16. **Session start is a convenience, not an enforcement point.**
    - `managed enroll` registers a session-start hook in each Agent target's agent that supports
      one.
    - The hook runs `managed apply` within a short time budget. When the budget runs out, apply
      stops at the next skill boundary (decision 12).
    - The hook always exits successfully and writes no agent metadata files.
    - Agents without hooks get the state on explicit `managed apply` or a scheduled run.

### Commands

17. A `managed` namespace joins ADR-0010's tree with four actions:
    - **`enroll`**
    - **`apply`**
    - **`status`** shows the source, subject, expiry, Agent targets, collisions and quarantine
      contents, and with `--report` the report body.
    - **`unenroll`** removes the hook registrations, the target entries in the ownership record,
      the managed store, the cached state, the recorded subject, `issued_at` and machine id, and
      the managed settings in the user configuration. A modified target entry is quarantined
      rather than removed. The quarantine itself stays. `unenroll` refuses when the system file
      sets `required = true`.

    All four are classified for `server serve` and `mcp serve` under ADR-0003. `enroll`, `apply`
    and `unenroll` are write operations: they change skill folders and run the credential command.

### What FastSkill does not do

18. **FastSkill never decides.** It doesn't know why a skill is listed, allowed or blocked. It
    has no users, roles or groups, and it only displays `subject` and compares it for equality.
19. **FastSkill ships no managed source.** `server serve` doesn't serve managed states. No
    FastSkill component accepts or validates identity. The one change to ADR-0003 is on the
    outbound side, recorded in its amendment: the CLI may pass along a token that a trusted
    credential command gave it.

### Where the code lives

20. Knowledge about agents belongs in `aikit-sdk`. Today it knows each agent's project skills
    folder. It needs four additions:
    - each agent's per-user skills and configuration folders
    - detecting that an agent is present, whether or not `aikit-sdk` can run it (today's list of
      installed agents covers only agents it can run)
    - deploying one skill into a folder by link or verified copy, without touching other entries
      (today's deployment replaces whole folders)
    - registering and unregistering a session-start hook, persistently

    FastSkill owns the rest: the managed state format and its verification, trusted
    configuration, `managed apply`, the ownership record, the install gate, quarantine, and the
    report. Both are libraries that any tool producing managed states can depend on.

### What only an `https://` source turns on

21. **Some fields and report items apply only when the managed source is an `https://` URL.**
    - **What they are:** the `editable` setting (decision 22), the request link (decision 23), and
      the refusals and project skills in the report (decision 15).
    - **With a file source,** FastSkill ignores `editable` and `request_url`, and it never sends
      a report (decision 6). `managed status` names each field it ignored and says why.
    - **With no managed source,** none of this exists and nothing changes.
    - **Blocking isn't one of them.** Blocked content is refused and quarantined, editable or not,
      whatever the source (decisions 11 and 13). That is part of what `blocked` means.

    Each of these serves a source that knows its users: one that can set a rule per user, take a
    request, and read a report. A file or git source is a state someone wrote or a CI job built.
    It can't take a request or receive a report, so its behavior stays fixed and a static state
    stays simple to write.
22. **`editable` decides whether editable local skills skip the allow-list.** It matters only
    under `allowed = listed`.
    - **`blocked-only`,** the default: editable local skills (ADR-0005) skip the allow-list. Their
      digests change with every edit, so no list could name them. Blocked digests are still
      refused and quarantined.
    - **`refused`:** adding an editable local skill (`skill add --editable`) is refused. Editable
      skills already installed are reported, never quarantined, so nobody loses local work.
    - There is no value that lets editable skills bypass `blocked`.
    - A state from a file source always behaves as `blocked-only`.
23. **A refusal can point to where access is requested.** `request_url` is a URL template with
    one `{digest}` placeholder.
    - When the install gate refuses content that isn't allowed, the refusal includes the link,
      with the refused digest substituted and percent-encoded.
    - FastSkill prints the link and never opens or fetches it. `server serve` and `mcp serve`
      include it in the refusal they return.
    - A refusal for blocked content never includes it. Blocked content isn't something to ask for.

## Consequences

- **Enforcement has limits**, as the threat model says. FastSkill enforces at its own operations:
  `managed apply`, installs and `--check`. A file copied into an agent folder between applies is
  caught at the next apply.
- **Nothing bounds withdrawal time for a machine that stops applying.**
  - A blocked digest reaches a machine at its next successful apply, which happens at every
    agent session start where hooks exist.
  - A machine that stops applying keeps what it has, however long that lasts. That includes a
    machine that is offline, has failing hooks, or runs agents without hooks.
  - Expiry limits how long FastSkill keeps allowing installs under an old state. It doesn't limit
    how long installed content stays.
  - A source can see from reports which machines haven't applied recently.
- **Three earlier ADRs are amended**, each in a section that points here:
  - **ADR-0001:** propagation gains a third member, `managed apply`.
  - **ADR-0003:** the CLI may run a trusted credential command and send the token it prints to
    the managed source's origin. Nothing inbound changes.
  - **ADR-0008:** for managed deployments, a local modification is quarantined instead of
    stopping for an explicit decision.
- **Multi-user machines.** The system file is machine-wide. Everything else is per user: the
  managed store, the machine id, the cached state and the quarantine. Each user enrolls and
  reports separately.
- **Nothing changes unless configured.** With no managed source, FastSkill does no new network
  activity, emits no new statuses, and every current command behaves as before.
- **A file source gets the core, not the service features.** Signing, expiry, the install gate,
  blocking and quarantine work the same for every source. The `editable` setting, request links
  and reports need an `https://` source (decision 21).
- **The report says a little more about projects.** It carries the ids and digests of the
  current project's skills, but no path or project identity. A source can tell that a blocked
  digest is still in some project on a machine, not which project.
- **A static file is enough.** A signed managed state in a git repository or a storage bucket is
  a complete managed source. Tools to build and sign a state from a Manifest are out of scope
  here and may follow.
- **The formats are a compatibility surface.** The envelope, the managed state and the report
  are versioned and documented, like the reconciliation vocabulary. Fields are added only in
  backward-compatible ways within a format version.
- **ADR-0006's open question is settled** for fleet rollout. Preset composition and session
  verification stay open.
- **CONTEXT.md gains** managed state, managed source, managed store, Agent target, quarantine,
  and the two new reconciliation statuses.
- **aikit changes come first.** The additions in decision 20 land in `aikit-sdk` before
  `managed apply` can deploy to agents.

## Considered alternatives

- **A built-in login command** was rejected. It would pull credential handling into FastSkill,
  against ADR-0003 and team-skill-presets Q14. A credential command covers identity provider
  CLIs, cloud CLIs and CI tokens without FastSkill holding any of them.
- **Selection rules in the client**, such as which skills are mandatory or optional for whom,
  were rejected. Each rule change on the deciding side would need a FastSkill release and a
  format change. A resolved state keeps the client the same whatever produced it.
- **A remote Manifest applied with `project install`** was rejected. A Manifest carries intent
  (ranges, repositories) that each machine would resolve differently over time, and it has no
  blocked list, expiry or signature. A managed state carries resolved content, like a Lock, so
  every machine applies the same thing.
- **An unsigned state trusted over HTTPS** was rejected. The cached copy, file sources and git
  sources need integrity that doesn't depend on the transport.
- **Signing canonicalized JSON** was rejected. A DSSE envelope signs the exact bytes, which
  avoids canonicalization mismatches between producers and FastSkill.
- **Reading managed settings from project configuration or the environment** was rejected. A
  repository or a shell profile could then redirect the source or choose the command FastSkill
  runs.
- **Binding each state to a machine** was rejected. The source would need a machine identity it
  can trust, which FastSkill can't provide. Binding the state to its source and subject, with the
  token authorizing each fetch, fits the threat model.
- **Deleting disallowed content** was rejected. Quarantine loses nothing and can be reversed.
- **An `editable = allowed` value** that exempts editable skills from `blocked` too was rejected.
  A blocked digest is harmful content, and installing it from a local path doesn't change that.
- **Applying `editable` and request links to file sources** was rejected. A file source can't
  set them per user or act on a request, so they would only add ways for a static state to be
  wrong.
- **Reporting project paths or repository URLs** was rejected. They identify private work, and a
  digest is enough to find where blocked content still is.
- **Reviving `sync`** was rejected. ADR-0001 removed a command of that name with a different
  meaning, so the new actions are `managed apply` and `managed enroll`.

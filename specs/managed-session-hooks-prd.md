# PRD: Managed session hooks for Claude Code, Codex, Cursor and Pi

Status: implemented (2026-10-17).
Date: 2026-10-17.

This PRD plans the third amendment of
[ADR-0016](../docs/adr/0016-machines-follow-a-signed-managed-state.md) (decisions 16, 17 and 20).
It depends on the `aikit-sdk` change planned in aikit's
`docs/plans/session-hooks-codex-cursor-pi.md` and decided in aikit ADR 0025. MUST, MUST NOT,
SHOULD and MAY express requirement strength.

## Problem

`managed enroll` and every `managed apply` register a session-start hook that starts a background
apply, for each covered agent `aikit-sdk` can hook. Today that is Claude Code and Gemini CLI. The
agents people on managed machines use are Claude Code, Codex, Cursor and Pi. For Codex, Cursor
and Pi, a new managed state arrives only from the timer, up to an hour later, and a lapsed sign-in
shows only in `managed status`, which nobody runs unprompted.

## Solution

FastSkill hooks exactly four agents: Claude Code, Codex, Cursor and Pi. Every other agent is
reached by the timer alone. A user on a machine they manage runs `managed enroll` once, approves
the hook where the agent asks, and from then on each new session in any of the four agents starts
an apply and shows a notice when they must act. Device management gets the administrator-level
entry for each of the four agents that has one from `managed hooks --system`.

## User stories

1. As a user of Codex, I want each new or resumed session to start a managed apply, so that a
   changed managed state is in place by my next session instead of within an hour.
2. As a user of Cursor, I want each new conversation to start a managed apply, so that Cursor
   picks up managed skills as soon as the other agents do.
3. As a user of Pi, I want each started, new or resumed session to start a managed apply, so that
   Pi isn't left on the timer only.
4. As a user of Claude Code, I want my hook to keep working unchanged, so that this change costs
   me nothing.
5. As a user, I want the hook to add no wait to my session start, so that managed skills never
   slow me down.
6. As a user, I want the hook to cost no model tokens when nothing needs me, so that it is free in
   normal use.
7. As a user whose sign-in lapsed, I want a notice in the agent I'm using, so that I know to run
   `fastskill managed apply` in a terminal.
8. As a Codex user, I want `managed enroll` to tell me that Codex runs the hook only after I trust
   it in `/hooks`, so that I don't think the hook is broken.
9. As a Codex user, I want to be told again when the hook changes and needs trust again, so that
   an upgrade that moves the program doesn't silently stop the hook.
10. As a Cursor user, I want to know that Cursor doesn't show me the notice, so that I check
    `managed status` when skills seem stale.
11. As a Pi user, I want the notice shown in Pi's interface and kept out of the model's context,
    so that it costs no tokens.
12. As a Pi user running Pi without an interface, I want the hook to stay silent, so that print
    and RPC output aren't polluted.
13. As a Gemini CLI user enrolled before this change, I want the FastSkill hook removed from my
    Gemini settings on the next apply, so that nothing is left behind that FastSkill no longer
    maintains.
14. As a Gemini CLI user, I want the timer to keep my skills current, so that dropping the hook
    loses at most an hour.
15. As a user with my own hooks in an agent's file, I want FastSkill to leave them exactly as they
    were, so that enrolling never breaks my setup.
16. As a user with a Pi extension that happens to have FastSkill's file name, I want FastSkill to
    refuse and leave it alone, so that my code is never overwritten or deleted.
17. As a Codex user who keeps hooks in `config.toml`, I want FastSkill to warn me and write
    nothing, so that Codex doesn't warn about hooks in two files.
18. As a user, I want `managed hooks` to list the four agents and whether each hook is registered,
    so that I can see what runs at session start.
19. As a user, I want `managed unenroll` to remove the hook from all four agents, so that nothing
    of FastSkill stays behind.
20. As an administrator, I want `managed hooks --system` to print Codex's entry as TOML for
    `requirements.toml`, so that Codex trusts it by policy and users approve nothing.
21. As an administrator, I want `managed hooks --system` to print Cursor's entry for its
    administrator-level `hooks.json`, so that I can place it with device management.
22. As an administrator, I want `managed hooks --system` to say Pi has no administrator-level
    location and the timer covers it, so that I don't look for one.
23. As an administrator, I want apply to register no user-level hook for an agent whose
    administrator-level FastSkill hook is present, so that the hook doesn't run twice.
24. As a user, I want `managed hooks --json` to carry the same facts, so that scripts can check
    them.
25. As a maintainer, I want agent file formats to live only in `aikit-sdk`, so that FastSkill
    knows nothing about how each agent stores hooks.

## Implementation decisions

### The hooked agents

- FastSkill MUST keep its own list of hooked agents: Claude Code, Codex, Cursor and Pi, by their
  `aikit-sdk` keys.
- Hook sync MUST register the hook only for an agent that is in that list, supported by
  `aikit-sdk`, and covered by the current apply, unless an administrator-level FastSkill hook is
  present for it.
- Hook sync MUST remove FastSkill's hook from every other agent `aikit-sdk` supports, Gemini CLI
  included. This is how existing Gemini hooks go away, so `aikit-sdk` keeps its Gemini row.
- `managed unenroll` MUST remove FastSkill's hook from every agent `aikit-sdk` supports, not only
  from the four.
- `managed hooks` and `managed hooks --system` list the four agents only.

### The hook itself

- The hook MUST be built from an argument vector: the absolute path of the running FastSkill
  program, then `managed`, `apply`, `--hook` and the hook id `fastskill-managed-<agent>`. The
  shell command form other agents store is derived from it by `aikit-sdk`.
- What `managed apply --hook` does is unchanged: it starts a detached apply, returns at once,
  always exits successfully, and prints at most one notice through `aikit-sdk`'s notice
  formatting for the agent named in the hook id.
- Pi passes the notice to its interface only when one exists, so print, JSON and RPC modes stay
  clean.

### Enroll and apply messages

- When a hook is newly added, enroll and apply MUST print the agent's approval note when
  `aikit-sdk` gives one: Claude Code and Codex.
- When `aikit-sdk` reports the Codex hook as changed rather than unchanged, the note MUST be
  printed again, because Codex asks for trust again.
- A refusal from `aikit-sdk` (a file of the wrong shape, a Codex `config.toml` with hooks, a Pi
  file FastSkill didn't write) MUST become a warning in the apply result, never a failed apply.

### Administrator-level entries

- `managed hooks --system` MUST print each entry in the format `aikit-sdk` gives: JSON for Claude
  Code and Cursor, TOML for Codex, with the file it belongs in.
- For Pi it MUST print that there is no administrator-level location and that the machine-wide
  timer covers Pi.
- `managed hooks --system --json` carries each entry's format and text as strings.

### Documentation

- The CLI reference for `managed` and the managed-machines security page list the four agents,
  each one's approval step, where its notice shows, and its administrator-level location.
- Gemini CLI is listed as reached by the timer.

### Known gaps

- Pi installs that move the agent folder with `PI_CODING_AGENT_DIR` get no hook; the timer still
  reaches them.
- Whether Codex's IDE extension and app run hooks is not documented. Only Codex CLI is claimed.
- Cursor shows the notice to the model, not the user.
- The Cursor CLI (checked on 2026.10.16-e3cb207) runs the hook only when a process starts a new
  chat; a chat started with `/clear`, `--resume` or `--continue` runs none, and the timer still
  reaches it. Cursor's IDE wasn't checked.

## Testing decisions

- Good tests check what the commands do to a temporary home directory and what they print, not
  how the code is arranged.
- Hook sync is tested at its existing seam, the hook tests in `fastskill-core`'s managed module:
  the four agents are registered when covered, Gemini's existing hook is removed, an agent outside
  the list is never registered, an administrator-level hook suppresses the user-level one, and a
  refusal becomes a warning.
- `managed hooks`, `managed hooks --system` and `managed apply --hook` are tested at the existing
  CLI seam, the managed hooks tests in `fastskill-cli`: the four agents listed, Codex's TOML
  entry, Pi's "no location" line, and the notice form for each agent.
- The approval-note rule is tested for added and changed Codex hooks.
- The file formats themselves are `aikit-sdk`'s to test.

## Verification

Before release, on real installs of each of the four agents, record the agent version and check:

1. `managed enroll` registers the hook and prints the approval step where there is one;
2. after approval, a new session and a resumed session each start an apply (the last-apply time in
   `managed status` moves);
3. with the sign-in lapsed, the notice shows where this PRD says it does;
4. `managed unenroll` removes the hook;
5. on a machine enrolled before this change with Gemini CLI, the next apply removes the Gemini
   hook.

## Out of scope

- Hooks for any agent other than the four, including GitHub Copilot and Gemini CLI.
- FastSkill writing administrator-level files.
- Making a skill visible in the session whose hook deployed it.
- Any change to the managed state, its verification or the timer.

## Further notes

Sources checked on 2026-10-17: Cursor's hooks documentation, OpenAI's Codex hooks and managed
configuration documentation, and the documentation and source of Pi's coding agent
(`badlogic/pi-mono`, `packages/coding-agent`).

# Semantic indexing is conditional on an optional embedding provider

## Status

accepted

## Context & decision

`index rebuild` builds a vector index from `SKILL.md` content, and `skill search --local` plus
`analysis` consume it. All of this requires an **embedding provider** that may not be configured.

We decide that semantic indexing is a **conditional capability, never an unconditional step**:

- `index rebuild` runs only when an embedding provider is configured. With no provider it is
  skipped silently.
- After `skill add`, `project install`, `skill update`, `skill remove`, and bundle mutations,
  indexing may auto-run only when embeddings are enabled. Configuration and `--reindex` /
  `--no-reindex` override that policy.
- `cli doctor` reports embedding-provider readiness so users know whether `index rebuild`,
  `skill search --local`, or `analysis` will do anything.

## Consequences

- FastSkill is fully usable with **no** embedding provider (keyword/non-semantic paths only); semantic features degrade gracefully rather than erroring.
- Every consumer of the vector index (`skill search --local`, `analysis`) inherits the same
  provider precondition and `cli doctor` visibility.
- Auto-run behavior is environment-dependent, so docs must describe the *condition*, not a fixed "always reindexes" contract.

## Considered alternatives

- *Always rebuild or hard-fail without a provider* — rejected: breaks environments that
  legitimately have no LLM access and turns an optional enhancement into a hard dependency.
- *Never auto-run; keep it fully manual* — rejected: repeated manual rebuilding made lifecycle
  operations incomplete by default.

[ADR-0010](0010-command-taxonomy.md) assigns these operations to their current command paths.

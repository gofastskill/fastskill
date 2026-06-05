# Agent Runtime Selection Primitives

Canonical requirements document for RFC-055: Add Core `--agent` and `--all` Runtime Selection Primitives.

## Resolved Design Decisions

### §4.3 — Command-specific default policies

When neither `--agent` nor `--all` is provided, each command applies the following policy:

| Command | Default policy | Rationale |
|---------|----------------|-----------|
| `eval run` | Fail with `RUNTIME_NO_SELECTION` (exit code `2`) | Eval execution always requires an explicit target; silent fallback to an unintended runtime is unsafe |
| `eval validate` | Proceed without agent-availability check (validate config only) | Preserves existing behavior; agent check is optional |
| `sync` | Fall back to aikit auto-detection via `instruction_file_with_override()` | Preserves backward-compatible behavior for users who rely on single-file auto-detect today |

The `RUNTIME_NO_SELECTION` error code is triggered only when the command's default policy is strict error. Currently this applies to `eval run` only; `sync` falls back to auto-detection.

### §4.4 — Multi-agent execution semantics for `eval run`

**Decision: Option A — sequential per-agent execution.**

When multiple agents are resolved (via `--agent a --agent b` or `--all`), `eval run`:

1. Executes the full suite sequentially for each agent.
2. Writes artifacts to a per-agent subdirectory: `<output-dir>/<run-id>/<agent>/`.
3. Returns non-zero if any agent's suite fails.

Parallel execution (Option B) is deferred to a follow-on spec.

## Error Codes

| Code | Trigger | Commands |
|------|---------|---------|
| `RUNTIME_CONFLICTING_FLAGS` | `--agent` and `--all` both provided | `sync`, `eval run`, `eval validate` |
| `RUNTIME_UNKNOWN_ID` | `--agent` value not in `aikit_sdk::runnable_agents()` | `sync`, `eval run`, `eval validate` |
| `RUNTIME_EMPTY_SET` | `--all` but `runnable_agents()` returns empty list | `sync`, `eval run`, `eval validate` |
| `RUNTIME_NO_SELECTION` | No flags provided and command policy is strict | `eval run` only |

## Ordering Guarantees

- `--all`: alphabetical sort of `runnable_agents()` output (deterministic across aikit-sdk versions).
- `--agent`: first-occurrence order (respects explicit user intent).
- Duplicate `--agent` values are silently deduplicated (POSIX convention).

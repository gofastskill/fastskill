# Semantic indexing is conditional on an optional embedding provider

## Status

accepted

## Context & decision

`reindex` builds a vector index from `SKILL.md` content, and `search --local` / `analyze` consume it. All of this requires an **embedding provider** (an LLM/embeddings backend such as OpenAI) that **may not be configured** in a given environment.

We decide that semantic indexing is a **conditional capability, never an unconditional step**:

- `reindex` runs only when an embedding provider is configured. With no provider it is **skipped silently**, not failed.
- After mutating commands (`add`/`install`/`update`/`remove`), `reindex` may **auto-run, but only when embeddings are enabled** (overridable via config and `--reindex` / `--no-reindex`). This removes the "did you forget to reindex?" footgun without forcing network/embedding calls on users who have no provider.
- A new `doctor` command reports embedding-provider readiness so users know up front whether `reindex` / `search --local` / `analyze` will do anything.

## Consequences

- FastSkill is fully usable with **no** embedding provider (keyword/non-semantic paths only); semantic features degrade gracefully rather than erroring.
- Every consumer of the vector index (`search --local`, `analyze`) inherits the same provider precondition and the same `doctor` visibility — they must not assume the index exists.
- Auto-run behavior is environment-dependent, so docs must describe the *condition*, not a fixed "always reindexes" contract.

## Considered alternatives

- *Always reindex / hard-fail without a provider* — rejected: breaks environments that legitimately have no LLM access and turns an optional enhancement into a hard dependency.
- *Never auto-run, keep it fully manual* — rejected: the docs already had to repeatedly nag "remember to reindex," the classic sign the step should be a side-effect.

# A bare version constraint means an exact pin, not caret

## Status

accepted

## Context & decision

A Manifest dependency's **version constraint** filters which candidate versions may be selected
during resolution (`resolver.rs`, `constraint.satisfies(candidate)`). The `Lock` separately pins the
one resolved version, so reproducibility comes from the Lock, **not** from the constraint.

We decide that a **bare version string (`1.2.3`) means an exact match** (`=1.2.3`) — not a
compatible-range as npm (`^`) and Cargo (bare-is-caret) interpret it. Range semantics are **opt-in**
via explicit operators (`^`, `~`, `>=`, `<=`, or comma ranges). This matches the current hand-rolled
parser's behavior (bare → `Exact`) and is a deliberate product stance: a skill package manager
should default to determinism and no surprise upgrades; a user who wants a range types one.

## Consequence for the `semver::VersionReq` migration (spec 002 BUG-2)

BUG-2 replaces the buggy hand-rolled constraint logic with `semver::VersionReq`. **`VersionReq::parse`
treats a bare `1.2.3` as caret** (`>=1.2.3,<2.0.0`). To preserve the decision above, the migration
**must normalize a bare `MAJOR.MINOR.PATCH` to `=MAJOR.MINOR.PATCH` before handing it to
`VersionReq::parse`.**

⚠ **Do not remove that normalization.** It looks like a pointless special-case; it is not. Deleting
it silently *widens every bare pin* in every committed Manifest from "exactly X" to "X and any
compatible newer version" — a silent resolution change with no error. This ADR exists so that
future contributor doesn't "clean up" the line. A regression test must assert bare-pin equality.

## Considered alternatives

- *Adopt caret for bare versions* (drop the normalization, use `VersionReq`'s native meaning) —
  rejected: it is silent (resolves differently rather than erroring), it changes the meaning of
  manifests already committed, and the ergonomic argument is weak here because the Lock already
  provides reproducibility, so exact-by-default costs little. Standardness alone did not outweigh a
  silent behavior change to a persisted format.

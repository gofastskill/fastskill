# Blind doc-walk harness (spec 005, P3)

Runs a webdoc's own code blocks **exactly as written** and checks whether the doc's
claimed output still matches reality — without ever letting the thing doing the
checking also decide what to run.

```
extract.py    deterministic  -- pull fenced code blocks out of the .mdx, in order,
                                 verbatim; classify + pre-filter, never edit
runner.py     deterministic  -- execute each runnable block exactly as written, one
                                 sandbox per doc, capture output
classify.py   LLM            -- ONLY compares doc-claimed output vs actual output and
                                 labels the divergence (match/drift/broken)
```

## Why the split, specifically

The value of "blind" is a reader who runs the doc verbatim and reports divergence
**without self-correcting**. An LLM is the worst actor at that — it "knows" the right
command and silently fixes the doc's mistake, hallucinating success. So the LLM here
**never decides what runs**. `classify.py`'s prompt has no field for a corrected
command; it can only emit a verdict and a quote. If you find yourself wiring a command
suggestion into this pipeline, you have broken the design — see spec 005 section 2.

Per spec 005 Q3, this — like `ci/quality/error_quality` — can never become a PR gate:
anything with an LLM in the verdict path stays in the nightly soft tier, permanently.

## Files

| | |
|---|---|
| `extract.py` | Fence parser (handles 2-4 space indented fences), pre-filters, anchor detection |
| `runner.py` | Executes runnable blocks in one persistent per-doc sandbox |
| `classify.py` | Gateway prompt + majority vote over `judge.py`'s `Gateway` |
| `docs.json` | The settled 4-doc set (spec 005 Q10) |
| `run_docwalk.py` | CLI entry point tying the three stages together |

`classify.py` reuses `error_quality/judge.py`'s `Gateway`, `strip_reasoning`, and
`extract_json` rather than reimplementing the gateway client or the `<think>`-block
handling — same gateway, same reasoning-model quirks.

## Running it

```bash
LLM_GATEWAY_URL=... LLM_GATEWAY_KEY=... LLM_GATEWAY_MODEL=... \
    python3 ci/quality/docwalk/run_docwalk.py --json report.json
```

`--capture-only` runs the extractor and the sandboxed commands and prints what came
back, **without calling the gateway at all** — this is how you check the harness
itself, or see what a doc's blocks actually do, for free. It needs no gateway
credentials and no network beyond whatever the doc's own commands touch.

Exit codes: **0** always, except a harness that genuinely cannot run — no binary, or a
gateway that fails a one-request connectivity preflight — which exits **2**. A doc
full of `broken` verdicts still exits 0: this tier reports, it never gates (spec 005
Q3). `--max-requests` (default 300) caps gateway calls; classifications past the cap
are skipped **and named** in both the console output and the summary, never silently
dropped.

## Extraction rules (what gets run, what gets flagged, and why)

- Fences are found by scanning for opening/closing backtick runs at **any**
  indentation, not just column 0 — a `^```` scanner misses blocks nested inside list
  items (see `webdocs/quickstart.mdx`, "Add standard-compliant skills").
- A ```bash/sh/shell/zsh/console``` block is a **command** candidate. It is
  pre-filtered and flagged `human-review` — never run, never silently skipped — if it:
  - contains a `$ `-prefixed line (illustrative prompt+output, e.g.
    `webdocs/skill-management/reconciliation.mdx`'s status tables),
  - contains a placeholder (`<...>`, `/path/to`, or the `your-*` family — `your-org`,
    `your-token`, and the `your-key-here` / `your-gateway-key` shapes found in
    `configuration/init-command.mdx` during this build),
  - or is a network installer (`curl ... | bash`, `sudo`,
    `brew`/`scoop`/`cargo install`) — banned even inside a sandboxed container.
- ```toml/json/yaml/yml``` blocks are **file bodies**, never executed. If the fence's
  info string names a file (```toml skill-project.toml```), the body is written
  verbatim to the sandbox before later commands run; an untagged file body (an
  illustrative fragment, e.g. `configuration/init-command.mdx`'s `schema_version`
  snippets) is recorded but not written anywhere.
- Everything else (untagged fences, `mermaid`, etc.) is **prose**: never executed. A
  bare fence directly under a command block is treated as that command's claimed
  output instead of an orphan (see anchors, below).
- Heading sections with **zero** code blocks anywhere under them (e.g.
  `validation.mdx`'s "Best Practices" essay) are flagged separately, at the section
  level — this is the "prose-only step… still needs a human" limit from spec 005,
  generalized from one named example to every such section, found mechanically. A
  parent heading and a code-free child heading both get flagged independently, which
  reads verbose but is honest: no code exists anywhere in either scope.

## Sandboxing: one sandbox per *doc*, not per block

Unlike `error_quality` (fresh sandbox per case), a doc-walk uses **one persistent
sandbox per doc**, walked top to bottom. A real reader works in one directory; a doc
that creates `skill-project.toml` in step 2 is *supposed* to have it still there in
step 4. Isolating every block would make an internally consistent multi-step doc look
broken for no reason.

The sandbox starts **empty** — the runner never pre-seeds a skills directory or
manifest fields a doc doesn't create itself. If a doc's own steps don't create
something a later step needs, that's a real doc finding, not a harness bug to paper
over.

Commands run with `stdin=DEVNULL`: an interactive command (`fastskill init` with no
`--yes`) hits EOF immediately, exactly as it would for anyone who actually piped the
doc into a non-interactive shell. Blocks run without `set -e`, matching how a person
pastes lines one at a time — one failed line doesn't stop the rest of the block, which
matters for blocks that present several alternative examples in one fence (spec's
"verbatim" is the fence's contents, not an implied `&&`-chain).

## Expected-output anchors

There is no clean command/output pairing in the corpus — that's exactly why the
comparison step is LLM, not diff. Three anchor kinds, all attached automatically:

- **`output-comment`** — the inline `` # Output: `` convention, single- or multi-line
  (`webdocs/skill-management/reconciliation.mdx` is the densest: 6 of the corpus's
  known ~14 occurrences).
- **`next-block`** — a bare fence immediately following a command block
  (`configuration/init-command.mdx`'s interactive-prompt example).
- **`prose-hint`** — a "you'll see…" / "you should see…" sentence outside any fence,
  attached to whichever block is nearest by line distance, in **either** direction
  (validation.mdx's hint precedes its example; quickstart's follows it).

A block that ran but has **no** anchor is reported as `no_anchor` and never sent to
the gateway — nothing to compare, so no request is spent. A block that has an anchor
but wasn't run (flagged `human-review`) still reports the anchor text, for a human.

## Classification

3 trials per block (spec 005 Q5), majority vote. With three verdict categories instead
of `error_quality`'s pass/fail, "majority" is stricter here: only a **unanimous 3/3**
counts as decisive. Any split — 2-1 or 1-1-1 — is `inconclusive`, but still reports the
leading label so a human has a lead, not just a shrug.

## First live run (2026-08-18, judge `claude-haiku-4-5`, 22 requests)

7 of 45 blocks across the 4 docs had a claimed-output anchor and were actually run, so
7 were sent to the judge: **0 match, 1 drift, 6 broken, 0 inconclusive.**

Two independent, genuine doc bugs, not seeded or hand-picked:

- **`webdocs/quickstart.mdx`** — the manifest the doc tells you to create in step 2
  (`skill-project.toml` with only `[dependencies]`) has no `[tool.fastskill]` /
  `skills_directory`. Every later command in the walkthrough (`install`, `search`)
  fails with `Configuration error: project-level skill-project.toml requires
  [tool.fastskill] with skills_directory`.
- **`webdocs/skill-management/reconciliation.mdx`** — 4 of its `broken` verdicts share
  one root cause, not four independent bugs: the doc is written as a *reference* (what
  do the reconciliation states mean) rather than a self-contained walkthrough, so its
  `fastskill list` examples assume a project that was never set up earlier **in this
  file**. A blind, literal walk of just this doc correctly reports that as broken; a
  human reading it in context understands it's describing an existing project. Worth
  a decision (add a "assumes an existing project" note, or make it self-contained) but
  not a crash.
- `configuration/init-command.mdx`'s one `drift`: the doc's interactive-prompt
  transcript vs. what a headless, EOF-on-stdin run of `fastskill init` (no `--yes`)
  actually prints — expected, not a red flag; a genuinely blind non-interactive run of
  that exact command can't reproduce a human's typed answers.

Extraction/flag counts, per doc (`--capture-only`, no gateway calls, verified separately):

| Doc | Blocks | Command blocks run | Flagged human-review (command) | File bodies | Prose sections flagged | Anchors found |
|---|---|---|---|---|---|---|
| `quickstart.mdx` | 8 | 7 | 0 | 1 | 2 | 1 |
| `skill-management/validation.mdx` | 3 | 1 | 2 | 0 | 13 | 1 |
| `configuration/init-command.mdx` | 12 | 2 | 2 | 6 | 10 | 1 |
| `skill-management/reconciliation.mdx` | 22 | 13 | 5 | 3 | 9 | 6 |

`validation.mdx` and `init-command.mdx` run far fewer of their command blocks than
`quickstart.mdx`/`reconciliation.mdx` — most of their bash fences are either
`$`-prefixed illustrative transcripts or contain a placeholder (`<skill-id>`,
`your-key-here`), which the pre-filter is designed to catch, not a harness gap.

## Extending the doc set

The 4-doc set is deliberately settled (spec 005 Q10) — do not add docs here without
re-opening that decision. `installation.mdx` (global installers), `manifest-system.mdx`
(mostly illustrative TOML), and the registry/tool-calling docs (placeholder-dense) were
excluded on purpose for v1.

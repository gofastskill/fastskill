# Context Resolution API

FastSkill 0.9.228

Source: https://docs.gofastskill.com/progressive-loading/performance

Release revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d

Documentation revision: 0e67bc11940a7ab7c7362b16d7fd132aff169c9d



## Overview

Progressive loading in FastSkill means **context resolution**: an agent sends a prompt and gets back
the most relevant installed skills, so it can load only what it needs instead of every skill. This is
served **over HTTP only** by `fastskill server serve` — there is no CLI subcommand for it.

## `POST /api/v1/resolve`

Start the server and call the resolve endpoint:

```bash
fastskill server serve
```

```bash
curl -X POST http://localhost:8080/api/v1/resolve \
  -H 'Content-Type: application/json' \
  -d '{"prompt": "generate a pptx deck from these notes", "limit": 5}'
```

Request body:

| Field    | Type    | Notes                                                                              |
| -------- | ------- | ---------------------------------------------------------------------------------- |
| `prompt` | string  | The task text to resolve against. Must not be empty (`RESOLVE_EMPTY_PROMPT`).      |
| `limit`  | integer | Maximum number of skills to return. Must be greater than 0 (`RESOLVE_LIMIT_ZERO`). |

The response ranks installed skills by relevance to the prompt so the agent can pull just the top
matches into context. `resolve` is a **read** endpoint — it works whether or not `server serve` was started
with `--enable-write`.

## How ranking works

* **Embeddings power ranking when available.** With `OPENAI_API_KEY` set (and
  `[tool.fastskill.embedding]` configured), FastSkill ranks skills semantically using the embedding
  index.
* **Keyword fallback otherwise.** If no embedding provider is configured, semantic ranking silently
  skips and resolution falls back to keyword matching — the endpoint still returns results, it just
  ranks them without embeddings.

You can confirm whether an embedding provider is active from `GET /api/v1/status`
(`embeddingProvider` flag) or with `fastskill cli doctor`.

## Performance characteristics

* **Local vector index.** Skill embeddings are stored in a local SQLite vector index (path set by
  `index_path` under `[tool.fastskill.embedding]`), so resolution queries run against on-disk vectors
  with no external service call at query time.
* **Incremental index rebuilding.** A change-detection cache (`.fastskill/build-cache.json`) lets
  FastSkill
  re-embed only what actually changed, so keeping the index current is cheap.
* **Automatic rebuilding keeps the index fresh.** After `skill add`, `project install`,
  `skill update`, or `skill remove`, FastSkill
  reindexes automatically (unless `auto_reindex = false` or `--no-reindex`, and only when an
  embedding provider is available), so `resolve` reflects the current skill set.

There are no cache-size, eviction, or tuning knobs to configure. Resolution is driven by the prompt,
the `limit`, and the embedding index — not by a configurable runtime cache.


## See also

* [Loading strategies](/progressive-loading/strategies) — when to rebuild and search locally.
* [`server serve`](/cli-reference/serve-command) — full endpoint reference.


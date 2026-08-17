#!/usr/bin/env python3
"""Connectivity smoke for the spec-005 LLM gateway path.

Asserts, in order, that CI can:

1. reach the gateway on the Tailnet at all (``/v1/models``),
2. authenticate with the virtual key,
3. see both pinned models in the key's allow-list,
4. get a real embedding from ``LLM_GATEWAY_EMBEDDING_MODEL``,
5. get a real completion from ``LLM_GATEWAY_MODEL``.

Failing loudly here is the point: every later phase of the quality tier assumes
this chain works, and a mis-scoped key or a repointed alias is invisible until
something actually calls the gateway.

Stdlib only, on purpose -- this must run before any project dependency exists.

Secret hygiene: the key is read from the environment and sent in a header. It is
never printed, and error bodies are truncated, since LiteLLM echoes a prefix of
the presented key in auth errors.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request

TIMEOUT = 120


def fail(msg: str) -> None:
    print(f"FAIL  {msg}")
    sys.exit(1)


def env(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if not value:
        fail(f"{name} is empty -- the workflow guard should have caught this")
    return value


def call(url: str, key: str, payload: dict | None = None) -> dict:
    """GET (payload=None) or POST JSON, returning the decoded body.

    Raises RuntimeError with a truncated body on any non-2xx or non-JSON reply.
    """
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
        },
        method="POST" if data else "GET",
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as exc:
        body = exc.read().decode(errors="replace")[:300]
        raise RuntimeError(f"HTTP {exc.code}: {body}") from exc
    except urllib.error.URLError as exc:
        # Most likely the Tailnet join failed or MagicDNS did not resolve.
        raise RuntimeError(f"could not reach {url}: {exc.reason}") from exc
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"non-JSON reply from {url}: {exc}") from exc


def main() -> None:
    base = env("LLM_GATEWAY_URL").rstrip("/")
    key = env("LLM_GATEWAY_KEY")
    chat_model = env("LLM_GATEWAY_MODEL")
    embed_model = env("LLM_GATEWAY_EMBEDDING_MODEL")

    print(f"gateway   {base}")
    print(f"chat      {chat_model}")
    print(f"embedding {embed_model}")
    print("-" * 60)

    # 1-3. Reachable, authenticated, and both pins are in the key's allow-list.
    try:
        models = call(f"{base}/v1/models", key)
    except RuntimeError as exc:
        fail(f"/v1/models -- {exc}")
    available = sorted(m.get("id", "") for m in models.get("data", []))
    if not available:
        fail(f"/v1/models returned no models: {json.dumps(models)[:200]}")
    print(f"OK    /v1/models -> {len(available)} model(s): {', '.join(available)}")

    for label, name in (("chat", chat_model), ("embedding", embed_model)):
        if name not in available:
            fail(
                f"{label} pin {name!r} is not in the key's allow-list "
                f"({', '.join(available)}). Re-mint the virtual key with this "
                f"model, or correct the LLM_GATEWAY_{label.upper()}_MODEL secret."
            )
    print("OK    both pinned models are permitted by this key")

    # 4. A real embedding. Dimensionality is reported, not asserted, so swapping
    #    the embedding model does not break this smoke -- but note that changing
    #    it DOES invalidate any existing fastskill index (3840-dim kalm-embed vs
    #    1536-dim text-embedding-3-small), which is a reindex, not a config flip.
    try:
        emb = call(
            f"{base}/v1/embeddings",
            key,
            {"model": embed_model, "input": "fastskill gateway smoke"},
        )
    except RuntimeError as exc:
        fail(f"/v1/embeddings -- {exc}")
    vectors = emb.get("data") or []
    if not vectors or not vectors[0].get("embedding"):
        fail(f"/v1/embeddings returned no vector: {json.dumps(emb)[:200]}")
    print(f"OK    /v1/embeddings -> {len(vectors[0]['embedding'])} dimensions")

    # 5. A real completion.
    #
    #    Assert on completion_tokens, NOT on non-empty content: the pinned model
    #    may be a reasoning model that emits a <think> block, and it can spend
    #    its entire budget there and return empty visible content. That is a
    #    working gateway, so treating it as a failure would be wrong. Budget is
    #    generous for the same reason.
    try:
        chat = call(
            f"{base}/v1/chat/completions",
            key,
            {
                "model": chat_model,
                "messages": [{"role": "user", "content": "Reply with the word OK."}],
                "max_tokens": 512,
            },
        )
    except RuntimeError as exc:
        fail(f"/v1/chat/completions -- {exc}")
    produced = (chat.get("usage") or {}).get("completion_tokens", 0)
    if produced <= 0:
        fail(f"/v1/chat/completions produced no tokens: {json.dumps(chat)[:200]}")
    content = ((chat.get("choices") or [{}])[0].get("message") or {}).get("content", "")
    print(f"OK    /v1/chat/completions -> {produced} completion tokens")
    print(f"      visible content: {content.strip()[:80]!r}")
    if not content.strip():
        print("      note: empty visible content is expected for a model whose")
        print("            reasoning block consumed the token budget.")

    print("-" * 60)
    print("PASS  gateway path is healthy end-to-end from CI")


if __name__ == "__main__":
    main()

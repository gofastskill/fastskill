# fastskill → cli-framework `api-server` adoption spec

Status: draft (2026-05-25). Goal: replace fastskill's hand-rolled Axum host with cli-framework's shipped `api-server` (+ optional `api-swagger`) host, so fastskill follows the one enforced API structure (`/api/{version}/...`, fixed `/healthz`+`/readyz`, `X-API-Version`, graceful shutdown). cli-framework is consumed as a **library only** — fastskill keeps its clap CLI; only the serve path changes.

References (verified against source 2026-05-25):
- cli-framework host API: `aroff/cli-framework` `src/api/mod.rs` (`ApiServerBuilder`, `ApiServer`), `skill/examples/with_api/src/main.rs`. Features `api-server`, `api-swagger`.
- fastskill server: `crates/fastskill-core/src/http/server.rs`; serve cmd `crates/fastskill-cli/src/commands/serve.rs`; worker `crates/fastskill-core/src/core/registry/validation_worker.rs`.

---

## 0. Prerequisite — dependency alignment (VERIFY FIRST)

cli-framework `api-server` pins **axum 0.8, tower 0.5, tower-http 0.6**, and `pub use`s its axum as `cli_framework::axum`. Because the host's `version(...)`/`mount(...)`/`root_fallback(...)` accept `axum::Router`, fastskill's routers MUST be built with the **same axum 0.8** the framework links (a semver-incompatible axum will not compile across the boundary).

**Verified 2026-05-25 (post-pull):**
- ✅ **axum 0.8** — done (`Cargo.toml:61` `axum = "0.8"`; bumped 0.7.9 → 0.8.9 in PR #138).
- ❌ **tower-http still 0.5** (`Cargo.toml:63`) — **must bump to 0.6** so the `tower_http::cors::CorsLayer` passed to `.cors(...)` is the same major cli-framework links. Keep features `cors, trace, compression-gzip, fs`.
- ⚠️ **axum 0.8 path-param syntax.** axum 0.8 replaced `:param` / `*rest` with `{param}` / `{*rest}` and panics on the old form. fastskill route literals still use the 0.7 form (`/api/skills/:id`, `/v1/skills/:skill_id`, `/index/*skill_id`, …). Confirm whether #138 migrated these; regardless, when routes are re-registered under the version router / mounts they MUST use `{id}` / `{*skill_id}`.
- tower 0.5 — ok.

Steps:
1. Bump `tower-http` 0.5 → 0.6 (keep its feature list); fix any remaining axum-0.8 `{param}` route syntax. cli-framework enables only `cors`; cargo unions features, so fastskill keeps `compression-gzip` + `fs` at 0.6.
2. Add the dependency:
   ```toml
   cli-framework = { git = "https://github.com/aroff/cli-framework", rev = "<>=29f0bb0; incl. #74 root_fallback + #76 health_version>", features = ["api-server"] }
   ```
   `api-swagger` is added by the separate Phase-2 issue (§7), not now.
3. Ensure a single axum 0.8 in the tree (`cargo tree -i axum`). Prefer constructing host-facing routers via `cli_framework::axum` to guarantee the exact version; fastskill's own `axum = "0.8"` dep unifies with it.

---

## 1. Adoption model

- **Library-only.** Use `cli_framework::api::ApiServerBuilder` / `ApiServer` directly inside fastskill's existing `execute_serve`. Do **not** adopt cli-framework's `AppBuilder`/command system — clap stays.
- `FastSkillServer::serve()` (`server.rs:457-495`) is rewritten to compose an `ApiServer` and call `ApiServer::serve(addr)`. The TcpListener bind + `axum::serve` boilerplate is deleted (the host owns it, and adds SIGINT/SIGTERM graceful shutdown fastskill lacks today).

The real host API (from `src/api/mod.rs`, do not invent other methods):

```rust
let server = ApiServerBuilder::new()
    .version(ApiVersion {                       // app API → /api/v1/...
        name: ApiVersionName::parse("v1")?,
        router: v1_router,                      // axum::Router (state already injected)
        stability: Stability::Stable,
        deprecation: None,
        // openapi field exists ONLY with feature "api-swagger" (Phase 2)
    })
    .default_version(DefaultVersion::Pinned(ApiVersionName::parse("v1")?))
    .mount("/v1", claude_api_router)            // external Anthropic-compat surface (§4)
    .mount("/index", registry_index_router)     // raw registry index files (§4)
    .cors(fastskill_cors_layer)                 // build_cors_layer(config) result
    .root_fallback(ui_router)                   // SPA / static console (§5)
    .health_version(env!("CARGO_PKG_VERSION"))  // /healthz reports fastskill's version
    .readiness_check(readiness)                 // optional (§6)
    .build();

let shutdown = server.shutdown_token();         // bind the worker to it (§6)
// start ValidationWorker bound to `shutdown`
server.serve(&addr).await?;                      // owns listener + graceful shutdown
```

Builder surface available: `version`, `mount`, `default_version`, `cors`, `auth(BoxCloneLayer<Router>)`, `mcp_router` (unused — fastskill has no MCP), `readiness_check`, `protect_health`, `root_fallback`, `health_version`, `reserved_prefixes`, `build`. `ApiServer`: `serve(&str)`, `into_router()`, `shutdown_token()`.

**State note:** the host takes stateless `axum::Router`. fastskill's `create_*_routes()` return `Router<AppState>`; call `.with_state(state.clone())` on each family router (or on the merged family router) so the type becomes `Router` before passing to `version()`/`mount()`/`root_fallback()`.

---

## 2. Route inventory & target mapping

Current families (`server.rs:314-423`) and where each goes:

| Current routes | Today | Target |
|---|---|---|
| `/api/skills*`, `/api/project` (skills) | `create_skill_routes` | **drop `/api`** → `version("v1")` → `/api/v1/skills*`, `/api/v1/project` |
| `/api/search`, `/api/resolve`, `/api/reindex*` | `create_search_routes` | → `version("v1")` → `/api/v1/...` |
| `/api/status` | `create_status_routes` | → `version("v1")` → `/api/v1/status` (app status, NOT a health check — see §6) |
| `/api/manifest/skills*` | `create_manifest_routes` | → `version("v1")` → `/api/v1/manifest/...` |
| `/api/registry/*`, `/api/registry/publish*` | `create_registry_routes` | → `version("v1")` → `/api/v1/registry/...` (multipart publish included — §8) |
| `/index/*skill_id` | `create_registry_routes` | **mount** `"/index"` (raw index files; external/CDN-like path, keep unversioned) |
| `/v1/skills*` (Claude-API) | `create_claude_api_routes` | **mount** `"/v1"` (external Anthropic-compat contract — DO NOT version-shift, §4) |
| `/`, `/index.html`, `/app.js`, `/styles.css`, `/dashboard` | `create_ui_routes` | **root_fallback** (§5) |

To build the `v1` router: merge skills+search+status+manifest+registry sub-routers with their paths rewritten to drop the leading `/api`, then `.with_state(state)`. The host prepends `/api/v1`.

---

## 3. Breaking change (accepted)

Every fastskill `/api/...` route literal (~30 across `server.rs:314-421`) loses the `/api/` prefix and is re-served at `/api/v1/...`. This breaks:
- the embedded console UI's fetch calls (`src/http/static/app.js`) — update to `/api/v1/...`;
- integration tests under `tests/` that hit `/api/...`;
- any external clients of the internal API.

The Claude-API `/v1/...` surface and the `/index/*` surface are preserved verbatim via `mount(...)`, so external consumers of those are unaffected.

---

## 4. Auxiliary mounts (not versioned)

- **Claude-API compatibility (`/v1/skills...`).** `crates/.../handlers/claude_api.rs` explicitly mirrors Anthropic's skill API ("matching Anthropic's API specification"). Forcing it under `/api/v1/...` would break that contract. Mount it verbatim: `.mount("/v1", claude_api_router.with_state(state))`. Note this coexists with fastskill's own `/api/v1/...` — two distinct surfaces, intentional; document it.
- **Raw registry index (`/index/*skill_id`).** A CDN-like artifact path consumed by skill installers; keep its URL stable via `.mount("/index", index_router.with_state(state))` rather than versioning it. (Decision: preserve URL > consistency, because external installers may hardcode it. If no external consumer relies on it, folding into `/api/v1/registry/...` is the alternative.)

`mount()` paths participate in host collision checks; `/v1` and `/index` do not overlap `/api`, `/healthz`, `/readyz`, `/api/docs`, so they pass.

---

## 5. Static console UI → `root_fallback`

fastskill serves an embedded console (`server.rs:28-55`, `create_ui_routes` `366-374`) at `/`, `/index.html`, `/app.js`, `/styles.css`, plus dynamic `/dashboard`. The host owns the root router; PR #74 added `ApiServerBuilder::root_fallback(router)` for exactly this — "any path not matched by a versioned API, health, MCP, or Swagger route … serving a SPA or static assets at the root", wired last so all host routes win.

Action: collect the UI routes (the `serve_embedded_static` handler + `/dashboard`) into one `Router`, `.with_state(state)`, pass to `.root_fallback(...)`. The `include_dir!` embedding is unchanged. Verify `/dashboard` (dynamic HTML via `status::root`) still resolves through the fallback router.

---

## 6. Health, readiness, shutdown

- **Health/readiness are framework-owned and fixed:** `/healthz` (liveness) + `/readyz` (readiness), always on. Set `.health_version(env!("CARGO_PKG_VERSION"))` so `/healthz` reports fastskill's version, not cli-framework's.
- fastskill's existing `/api/status` is an **application status** payload (skills_count, uptime, hot_reload, storage_path) — it is NOT a health probe. Keep it as a normal versioned endpoint (`/api/v1/status`); do not try to replace `/healthz` with it.
- **Optional `readiness_check`:** supply a closure reporting `ReadinessReport { ready, checks }` once the skill index/storage is loaded; default is always-ready. Worthwhile if index build is async at boot.
- **ValidationWorker shutdown (net-new behavior).** Today `ValidationWorker::start()` (`validation_worker.rs:64-94`) spawns a `tokio::spawn` poll loop gated by `running: Arc<AtomicBool>` that nothing ever flips — it leaks on shutdown. Bind it to the host lifecycle: take `server.shutdown_token()` before `serve()`, and either (a) `tokio::spawn` a task that awaits `token.cancelled()` then sets `running=false`/calls `stop()`, or (b) pass the token into the worker and check `token.is_cancelled()` in the loop alongside the sleep (`tokio::select!` on `token.cancelled()` vs `sleep`). This gives clean drain on SIGINT/SIGTERM.

---

## 7. Middleware: compression, CORS, tracing

- **CompressionLayer.** The host deliberately applies NO response compression (its streaming-safety rule). fastskill *wants* gzip and has **no SSE/WebSocket/streaming** (verified — all handlers are JSON), so compression is safe — but fastskill must apply it to **its own** routers, not expect the host to. Apply `CompressionLayer` on the `v1` router (and on mounts if desired) before handing them to the builder. Do not apply it to the host's health/swagger routes (they aren't fastskill's to wrap, and don't need it).
- **CORS.** Replace fastskill's global `build_cors_layer(config)` application with `.cors(layer)` — pass the same `tower_http::cors::CorsLayer` (must be tower-http 0.6). The host applies it across the whole composed router. Keep `ServiceConfig`-driven origin logic unchanged.
- **TraceLayer.** Keep on fastskill's own routers if its tracing config matters; otherwise rely on the host. (Host does not expose a trace toggle; it's safe to layer your own.)

---

## 8. Multipart upload

`POST /api/registry/publish` (`handlers/registry_publish.rs:45-186`) uses `axum::extract::Multipart`. It works unchanged inside the versioned router; the path becomes `/api/v1/registry/publish`. Requires axum's `multipart` feature (already enabled). No host-level body-limit is imposed; if a cap is wanted, add a per-route `RequestBodyLimitLayer` on fastskill's router (tower-http `limit`).

---

## 9. Phase 2 — `api-swagger` (separate follow-up issue)

> Per project decision, a dedicated GitHub issue for `api-swagger` is created **after this migration is implemented** — it is not part of this migration. Outline below for context only.

fastskill ships no OpenAPI document today. With `api-server` only, omit the `openapi` field (it's `#[cfg(feature = "api-swagger")]`). To get runtime docs at `/api/docs` + `/api/v1/openapi.json`:
1. Enable feature `api-swagger` on the cli-framework dep (pulls `utoipa-swagger-ui`).
2. Produce an OpenAPI `serde_json::Value` for the v1 surface (hand-written or via utoipa) and set `openapi: Some(value)` on the `ApiVersion`.
3. Embedded Swagger UI works offline (no CDN). The Claude-API `/v1` mount is not auto-documented (it's a mount, not a version) — document it separately if needed.

---

## 9.5 Documentation updates (REQUIRED — part of this migration)

Documentation is in-scope for this migration, not a follow-up. Update everything that describes the API surface or how to run the server:

- **Console UI** (`crates/fastskill-core/src/http/static/app.js`, `index.html`): rewrite all fetch/XHR calls from `/api/...` to `/api/v1/...`. The Claude-API `/v1/...` and `/index/*` paths are unchanged.
- **README / usage docs**: update any documented endpoints to `/api/v1/...`; document the new health endpoints `/healthz` + `/readyz`, the `X-API-Version` header, and the no-version → 308 redirect behaviour.
- **`webdocs/`** (the docs site): update the HTTP API reference/base path to `/api/v1/...`; note that `/v1/skills...` remains the Anthropic-compatible surface (a separate `mount`, not versioned).
- **`docs/`**: keep this adoption spec as the record; add/adjust any API how-to docs.
- **CHANGELOG**: entry noting the breaking move to `/api/v1/...`, the added `/healthz`/`/readyz`, graceful shutdown, and that `/v1/...` (Claude-compat) and `/index/*` are unchanged.
- Migrate any `tests/` (integration) that hit `/api/...` to `/api/v1/...`.

## 10. Step-by-step plan

1. **Deps (§0):** axum 0.8 is done; **bump tower-http 0.5 → 0.6** and fix any axum-0.8 `{param}` route syntax; add `cli-framework` (`api-server`).
2. **Build the `v1` router:** copy `create_skill/search/status/manifest/registry` routers, strip the `/api` prefix from every literal, merge, `.with_state(state)`.
3. **Mounts:** Claude-API router → `mount("/v1")`; `/index/*` → `mount("/index")` (both `.with_state`).
4. **UI:** UI routes → `root_fallback(...)`.
5. **Rewrite `FastSkillServer::serve()`** to build `ApiServerBuilder` (version + default_version + mounts + cors + root_fallback + health_version), `build()`, grab `shutdown_token()`, start the worker bound to it, `server.serve(&addr).await`.
6. **Delete** the old listener bind, `axum::serve`, manual router-merge/middleware-stack, and `/health`-style status-as-health assumptions.
7. **Documentation (§9.5):** update console UI `app.js`, README, `webdocs/` API reference, CHANGELOG, and integration tests for the `/api/v1/...` move.
8. **Worker shutdown:** wire `running`/`stop()` to the token.
9. (separate follow-up issue) add `api-swagger` + an OpenAPI doc.

---

## 11. Acceptance criteria

1. `fastskill serve` brings up: `/api/v1/skills`, `/api/v1/search`, `/api/v1/status`, `/api/v1/manifest/...`, `/api/v1/registry/...` (incl. multipart publish); `/v1/skills...` (Claude-API, unchanged); `/index/*`; the console UI at `/`; and framework `/healthz` + `/readyz`.
2. `/healthz` returns `{status, version}` with **fastskill's** crate version; `/readyz` reflects the readiness check (or always-ready).
3. `X-API-Version: v1` present on `/api/v1/...` responses.
4. `GET /api/...` with no version → 308 redirect to `/api/v1/...` (default Pinned) — or, if `DefaultVersion::None` is chosen, a 404 listing versions.
5. SIGINT/SIGTERM drains gracefully: `/readyz` flips to 503, in-flight requests finish, and the ValidationWorker loop exits (no leaked task).
6. gzip still applied to fastskill responses (compression on its own routers); no regression for the console UI download size.
7. Builds with `--features api-server`; `cargo tree -i axum` shows a single axum 0.8.
8. Console UI loads and its API calls succeed against `/api/v1/...`.
9. Documentation is updated (§9.5): console UI fetches, README, `webdocs/` API reference, CHANGELOG, and integration tests all reflect `/api/v1/...` and the new health endpoints; no doc still advertises the old `/api/...` (unversioned) paths.

---

## 12. Risks & decisions

| Item | Decision / risk |
|---|---|
| Two `v1` surfaces (`/api/v1/...` own + `/v1/...` Claude-compat) | Intentional; the Claude-compat one is an external Anthropic contract kept via `mount("/v1")`. Document clearly to avoid confusion. |
| Dep state (verified) | axum 0.8 ✅ done (#138). tower-http still 0.5 → **bump to 0.6**. axum 0.8 `{param}` path syntax — fix old `:id`/`*skill_id` literals. |
| CompressionLayer ownership | Host won't apply it; fastskill applies on its own routers. Safe only because there is no streaming (verified). If SSE is ever added, scope compression to exclude `text/event-stream`. |
| `/index/*` versioning | Kept unversioned via `mount` to preserve installer URLs; revisit if no external consumer depends on it. |
| Breaking `/api/...` → `/api/v1/...` | Accepted; update console UI + tests + any docs/published clients. |
| Single axum in tree | Use `cli_framework::axum` for host-facing routers or ensure 0.8 unification. |

## 13. Out of scope

- Adopting cli-framework's CLI/command framework (clap stays).
- MCP serving (fastskill has none; `mcp_router` unused).
- Changing storage, search, or registry/S3 logic.
- The webhook-style or any second-listener concerns (fastskill has none).

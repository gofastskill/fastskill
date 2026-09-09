# PRD: Consistent lifecycle behavior through HTTP and MCP

Status: accepted and implemented.
Date: 2026-09-08. Implemented: 2026-09-09. Depends on shared state, resolution and command contracts.

This PRD repairs the accepted shared-core and write-gate behavior in
[ADR-0003](../docs/adr/0003-serve-trust-boundary-and-edge-auth.md) and
[ADR-0005](../docs/adr/0005-install-seam-and-origin-model.md). It does not add an
authentication system or new HTTP bundle endpoints.

## Problem and outcome

At audited main `2b649bf`, HTTP deletion removes bundle-required members outside
the CLI ownership checks. Manifest PUT returns requested group/editable values
without persisting them and replaces structured origin data during version edits.
A read-only MCP session can call optimize export and write its chosen file.

Users must receive the same constraints, persistence and truthful outcomes through
the browser, REST clients and agents as through the CLI. Read-only exposure must
enforce the published capability boundary.

## User stories

1. As a browser user, my edits persist exactly as reported and preserve provenance.
2. As a bundle recipient, an API mutation cannot bypass the ownership checks that
   protect my environment in the CLI.
3. As an MCP operator, omitting `--enable-write` prevents exposed mutation and
   execution tools, including artifact writers.
4. As a client author, I receive structured partial failures and can identify which
   changes committed without scraping console output.

## Requirements

### Shared operations and HTTP

- **P-01:** Existing HTTP install/update/delete and Manifest mutators MUST use the
  shared core context, validation, ownership and persistence operations. They MUST
  NOT implement their own directory deletion or serialize a partial Manifest.
  An enabled write gate MUST NOT bypass any lifecycle invariant.
- **P-02:** HTTP context MUST remain bound to the configured served project or
  supported global selection, independent of process working directory. A route
  that has no meaning in global scope, such as a project Manifest edit, MUST return
  an explicit unsupported-scope error before mutation. Unsupported bundle global
  combinations follow S-03.
- **P-03:** Manifest editing changes desired intent; it MUST NOT silently claim
  installed files were updated. Responses MUST return the persisted representation
  and indicate whether reconciliation is required. Version changes MUST preserve
  repository identity; non-repository origins MUST reject inapplicable version edits.
- **P-04:** In Manifest PUT, omitted fields MUST remain unchanged; a provided empty
  groups list MUST clear groups; provided editable false MUST persist false.
  Editable applies only to local directories. Invalid types, null where no null
  semantics exist, and conflicting fields MUST fail before persistence. Responses
  MUST NOT echo values that were ignored or discarded.
- **P-05:** Desired-state edits MUST NOT fabricate new resolved facts. A version or
  origin edit leaves old resolved contents identifiable as needing reconciliation;
  restore MUST NOT treat the old Lock as compatible. Removing direct intent MUST
  retain bundle and transitive ownership records and MUST NOT delete shared files.
  Installed deletion follows the same remove-root behavior as the CLI.
- **P-06:** Existing install requests MUST use the common origin parser. Add an
  optional repository selector for repository references, equivalent to CLI
  `--repository`; it MUST reject inapplicable origin combinations. Existing update
  requests MUST apply the same exact-version and constraint rules as C-04/C-05.
  Check/preview requests MUST validate the requested target, not ignore its version.
- **P-07:** HTTP responses MUST distinguish validation, not-found, ownership/integrity
  conflict, and application failure. Use appropriate non-success status for wholly
  failed operations. Existing batch response envelopes may remain, but MUST include
  truthful per-target outcomes and overall partial/failed status; clients MUST NOT
  interpret transport 200 alone as installation success. Existing compatibility
  routes MUST use the same implementation and outcomes.

### MCP effect classification

- **P-08:** Every registered MCP command MUST have an explicit effect classification
  derived from one authoritative definition. Unknown/unclassified tools MUST NOT
  be exposed as read-only; registration/coverage checks MUST detect omissions.
  The shared write-operation table remains the authority for HTTP mutation routes.
- **P-09:** Without `--enable-write`, all write/execution tools MUST be absent from
  tools/list, and direct tools/call attempts MUST return `MCP_TOOL_DENIED` (`-32005`)
  before dispatch. With writes enabled, they remain subject to core lifecycle rules.
  If any supported argument variant writes or executes, gate the whole tool until
  argument-aware checks are implemented and tested before dispatch.
- **P-10:** The effect inventory MUST cover every exported command, including the
  following easily missed cases. No command may delegate a prohibited effect from
  an otherwise allowed read path.

| Effect | Classification requirement |
| --- | --- |
| Add/install/update/remove, bundle override/reset | Write: managed files, intent, ownership, and Lock. |
| Init, repository mutations/refresh, cache clean, reindex | Write: configuration, cache/index updates, or provider activity. |
| Bundle build, marketplace create, optimize export | Write: caller-selected artifact creation/replacement. |
| MCP client install | Write: external client configuration. |
| Eval run/judge and optimize run/resume | Write/execution: run artifacts, agents or evaluation/judging providers. |
| Eval score/scorecard/report | Gate any tool supporting persistent output/artifact updates; classify actual effects, not its read-sounding name. |
| List/read/search, repo browsing, doctor, analysis, eval validate, optimize inspect/status | Allow only documented inspection effects; classify provider calls, persistent cache writes or output-file variants explicitly before admitting them. |
| MCP serve/HTTP serve | Keep long-running server startup outside request/response auto-registration. |

- **P-11:** Protocol responses/stdout and internal diagnostic logging do not by
  themselves require write permission. Documented remote read requests may remain
  reads. Artifact files, persistent catalog/index mutations, client edits, agent
  execution and evaluation/judging calls are not incidental logging. Denied tools
  MUST NOT produce those effects even when their chosen output is outside FastSkill
  storage. A generic write flag does not authorize bypassing bundle protections.
- **P-12:** CLI, HTTP and MCP MUST expose the same domain result for equivalent
  requests. Wrappers may change transport/status formatting, but MUST preserve
  changed/unchanged/blocked/failed/partial outcomes, diagnostics and affected IDs.
  Buffered CLI progress MUST NOT replace structured errors in MCP responses.

## Acceptance scenarios

| ID | Scenario | Required observable result |
| --- | --- | --- |
| P-A01 | HTTP delete a required bundle/transitive member | Same ownership conflict or root-detach result as CLI; no shared content loss. |
| P-A02 | PUT groups and editable, then GET and inspect Manifest | Exactly persisted fields returned; unrelated origin/settings/bundles survive. |
| P-A03 | PUT repository version, then inspect and restore | Repository identity preserved; unresolved desired change visible; no false locked compatibility. |
| P-A04 | PUT invalid local version, null, bad field type, unsupported scope | Non-success validation response before changing any state. |
| P-A05 | Serve a project with another cwd; invoke mutations | Only served state changes, never the ambient project's files. |
| P-A06 | HTTP batch update with one failed target | Structured partial failure; client accurately distinguishes committed and failed targets. |
| P-A07 | MCP read-only list and direct calls for every write/execution tool | Hidden and denied before dispatch; no files, client edits, processes or provider calls. |
| P-A08 | Read-only optimize export aimed at a harmless output and a skill path | Both denied; destination/sentinel content unchanged. |
| P-A09 | Enable MCP writes and invoke conflicting add/remove | Tool is callable but ownership policy still blocks the conflicting mutation. |
| P-A10 | Register a new command without effect classification | Coverage/registration failure; tool cannot silently become read-only. |
| P-A11 | Equivalent CLI, HTTP and MCP install/update/remove requests | Same selected state, result, errors and restoration behavior. |
| P-A12 | Plain read versus output-file/evaluation variant | Classification matches actual effects; gate precedes provider or output dispatch. |

Use real stdio JSON-RPC and localhost HTTP handlers with disposable projects. For
denied execution tests, use instrumented fake providers/runners and assert zero
calls; do not run paid models. Successful setup and an ungated control establish
that a denial is due to the gate rather than a malformed fixture. Compare Manifest,
Lock, files and structured responses after each operation.

## Implementation and delivery

Primary paths: [HTTP skills](../crates/fastskill-core/src/http/handlers/skills.rs),
[HTTP Manifest](../crates/fastskill-core/src/http/handlers/manifest.rs),
[models](../crates/fastskill-core/src/http/models.rs),
[write operations](../crates/fastskill-core/src/write_ops.rs),
[command registration](../crates/fastskill-cli/src/main.rs), and
[MCP server](../crates/fastskill-cli/src/commands/mcp.rs).

Update HTTP/browser consumers when response semantics change, MCP command metadata,
server references, webdocs and shipped skill guidance with implementation. Include
the full effect inventory in coverage, not a second manually maintained deny list.
This PRD does not require a new command framework or a second authentication layer.

No new HTTP bundle endpoints, cloud credentials, per-user authorization, fleet
control plane, implicit evaluations during installation, or protocol merger.

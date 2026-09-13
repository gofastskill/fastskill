# Eval-authoring verification record

Status: implementation evidence in progress. Updated: 2026-09-13.

## Current evidence (supersedes historical status tables below)

- Pinned FastSkill 0.9.230 built successfully. Six documented contract tests, four CLI asset tests, six documentation parity tests, and formatting passed before the latest example metadata assertion; that assertion still requires execution.
- Both authoring tools ran: Codex CLI 0.153.4 with `gpt-5.6-luna`, and Claude Code 2.1.269 with `claude-sonnet-4-5`. Generated suites validated against the pinned binary. Initial Claude attempts required trigger-scope, judge-path, schema, and template repairs.
- Claude continued the Codex-authored suite using files, repaired extra CSV columns and answer leakage, validated four cases/four checks, and recorded why changed prompts require new target evidence. Handoff cost reported by Claude: USD 0.2934362. Raw local record: `work/claude-handoff.json` beside this checkout.
- A real Claude Sonnet 4.5 defect pilot consulted the target, returned `15.00`, and failed the independent `17.00` expectation with exit code zero and no execution error. Changing only the target to include the final item produced a passing rerun with the same criterion. Both runs and the failed workspace are retained locally under `work/pilot-defect-claude-runs/2026-09-12T20-23-38Z/claude` and `work/pilot-corrected-claude-runs/2026-09-12T20-24-46Z/claude`.
- The conversion pilot passed the numeric outcome check, failed mandatory consultation, and failed the scratch-file check without gating because it was advisory. This demonstrates distinct outcome/adherence results, not a fully passing conversion suite. Evidence: `work/pilot-conversion-claude-runs/2026-09-12T20-23-45Z/claude`.
- Runtime prerequisite checks exposed `EVAL_ISOLATION_NO_SKILL` for missing metadata and `RUNTIME_UNKNOWN_ID` for codex in the current registry (available: aikit, claude). Standalone authoring-tool availability does not establish FastSkill target-runtime availability. The shipped example now includes metadata required for isolation.
- Strict clippy failed on a recursion-depth diagnostic in unchanged `crates/fastskill-cli/src/main.rs`. A new test panic lint was repaired. Full workspace regression was restarted with two compilation jobs after the unconstrained build impaired responsiveness; completion remains pending.
- No production Rust files changed. Numeric production line/branch coverage is not claimed; changed tests require execution, and skill/fixture behavior requires the inventory below.

Final local checks on 2026-09-13: the latest four asset tests passed, including the metadata assertion; formatting passed; source-size checking passed after removing CR characters from the script input stream (the checkout uses CRLF). The workspace command stopped at the CLI test binary with 457 passed and seven failed. Two isolation tests failed with missing project configuration during parallel execution and both passed when rerun with `--test-threads=1`. Two other failures report Windows privilege error 1314 during symlink operations; remaining add/update failures concern project state and require separate diagnosis. These are failures in unchanged files, not established regressions from this change, but the full gate is not green.

Rescoring the corrected pilot's saved artifacts passed without another model call. No relevant native-judge API key was present in this process, and the example endpoint/model remain placeholders; live calibration is unverified.

`cargo clippy -p fastskill-evals --tests --locked -- -D warnings` passed after the test lint repair. `git diff --check` passed. These focused results do not replace the failing workspace-wide gate.

Remaining release gaps: full workspace regression, strict clippy resolution/baseline evidence, remaining fixture and prerequisite branches, judge calibration/credential availability, and final per-file evidence inventory. No PR has been created. Historical environment blockers below are retained as attempt history, not current claims.

This record keeps deterministic checks, live authoring exercises, and release gaps distinct. It must not be read as a release attestation while any mandatory row is incomplete.

## Compatibility and confirmed validation behavior

- Intended supported baseline: FastSkill 0.9.230 at `dd983e89e5fee977325b77ae386b879e8310ed26`, with `aikit-evals` pinned to `eb50f2c5509d4345cab65af0f7faed012ab0e557`.
- Local installed binary: FastSkill 0.9.208. It parsed the shipped invoice example with `fastskill eval validate --json`, reporting three cases and one deterministic check, but it predates the declared baseline and therefore does not establish 0.9.230 behavioral compatibility.
- Confirmed source contracts at the pin: per-case implicit trigger checks from `should_trigger`; exact `cases` selectors; explicit applicable `skill_invoked` precedence; contradiction rejection; required/advisory behavior; path-reference consultation proxy.
- No new validation defect requiring production-source changes was identified. The existing engine and CLI already enforce the material contracts; this change repairs maintained guidance and adds contract tests.

## Requirement-to-test mapping

| Requirements | Evidence | Status |
| --- | --- | --- |
| WF1, A1, AT1, ST4, ST6 | `invoice-no-evals`; skill compatibility/help instructions; package validator | deterministic assets complete; live workflow incomplete |
| WF2, A2, AT3 | scripted workflow and unnecessary-step author decisions | fixture assertions complete; live workflow incomplete |
| WF3-WF4, A3-A5, AT4-AT5, ST1-ST5 | shipped example; semantics/examples references; documented contract tests | authored; Rust execution blocked by environment |
| WF5, A6-A7, AT6 | judge contrast examples, real `eval validate`, invalid contradiction test | parse evidence present; executed judge calibration unavailable |
| WF6, A8, AT9-AT10, ST8 | explicit pilot-scope instructions and scripted authorized/unauthorized branches | instructions/fixtures present; live exercise incomplete |
| WF7, A9, AT7-AT8, ST7 | defective target, environment variants, evidence categories | fixtures present; live target run incomplete |
| WF8, A10, AT11 | existing suite/user note, rescore/rejudge/rerun guidance, handoff template | fixture assertion authored; live agent switch incomplete |
| ST2-ST3 | shipped suite validation and `documented_check_contract.rs` trigger/selector/contradiction tests | local 0.9.208 parse passed; pinned Rust test not yet executed |

## Changed-file coverage classes

- `skills/eval-authoring/SKILL.md`: every normative workflow section maps to WF1-WF8 and AT1-AT11 above; structural validation passed with `quick_validate.py`.
- `skills/eval-authoring/references/*.md`: linked from the entrypoint and exercised by the asset integration test; command and semantic claims map to the maintained docs and pinned contract tests.
- `skills/eval-authoring/examples/invoice-extraction/**`: every config/prompt/fixture/note is parsed or asserted by `eval_authoring_assets_test`; the suite also passed file validation under local FastSkill 0.9.208, with the compatibility limitation above.
- `tests/fixtures/eval-authoring/**`: the asset integration test checks discovery and independent ground truth; the existing suite itself is parseable; live behavioral use remains mandatory.
- `crates/fastskill-evals/tests/documented_check_contract.rs`: focused assertions cover positive/negative implicit checks, exact selectors, explicit precedence, contradiction, required/advisory behavior, and trace-only matching.
- `tests/cli/eval_authoring_assets_test.rs` and `tests/cli/mod.rs`: Cargo discovery is wired, meaningful assertions and a controlled contradiction failure are present; execution is currently blocked before compilation.
- `webdocs/evals-quality/setup.mdx`: repaired claims map to the documented contract test and shipped example; link/parity regressions remain to run.
- `.gitignore`: controlled check is that only `skills/eval-authoring/**` is unignored while another `skills/*` path remains ignored.
- `CONTEXT.md`, ADR-0011 through ADR-0013, and the three authoring requirement documents: accepted design handoff transferred unchanged; their requirements map through this record.

No production executable Rust file is modified, so the 90% line and 80% branch thresholds do not apply to production source in this change. Test-source execution and meaningful assertion evidence are still mandatory.

## Commands and observed results

| Command or exercise | Result |
| --- | --- |
| `quick_validate.py skills/eval-authoring` | passed (`Skill is valid!`) |
| `cargo fmt --all -- --check` | initially reported formatting-only diffs; `cargo fmt --all` applied them |
| local FastSkill 0.9.208 `eval validate --json` in shipped example | passed; three cases, one explicit check; version is outside declared compatibility |
| pinned Cargo test invocation | blocked before compilation: sandboxed Windows Cargo cannot read the installed Git system config; local path patches bypassed Git dependencies, then crates.io TLS failed with `SEC_E_NO_CREDENTIALS`; offline cache lacks `ammonia` |
| Codex CLI 0.153.4, `gpt-5.6-luna`, isolated authoring exercise | attempted twice; first rejected incompatible CLI flags, second stopped before model invocation because the sandbox account has no resolvable Codex home |
| second authoring tool | unavailable: `pi` and `claude` not installed; Cursor 3.17.8 exposes the editor launcher, not a headless authoring-agent/model interface |

## Release gaps

- Execute the focused tests, full relevant regression suite, command/document parity checks, source-size check, clippy, and coverage instrumentation in a Rust environment with usable registry access.
- Complete WF1-WF8, AT1-AT11, and ST1-ST8 behavioral exercises with at least two distinct capable authoring tools, including preservation and handoff.
- Verify actual selected model IDs and authentication for both tools; record all attempts and sanitized artifacts.
- Exercise a real target pilot and, where supported, judge contrasts. Missing standalone calibration remains an explicit product limitation.
- Produce final changed-file coverage results. Branch coverage is unavailable until instrumentation runs.

Until these gaps close or the owner explicitly waives a gate, no pull request may be created.

# Eval-authoring verification record

Status: implementation and repository-consolidation local gates verified; draft PR CI tracked separately. Updated: 2026-09-15.

## Repository consolidation

The installable authoring workflow belongs to `gofastskill/skill`, inside its existing
`fastskill/` package. The CLI PR #333 removes the former `skills/eval-authoring/` tree,
authoring fixtures, and Rust asset-test module after moving them to the skills repository.
There, `fastskill/SKILL.md` routes to `fastskill/references/eval-authoring.md`; references
and the invoice example are packaged with it. Scenario fixtures are under
`evals/authoring/fixtures/`, and executable asset checks are under `scripts/`.

The existing skills-repository `evals/` content measures the FastSkill skill itself.
Its useful suites, generator, metrics, fixtures, and historical baseline are preserved.
The companion assessment records obsolete claims and harness defects separately from
verified behavior. No historical baseline is relabeled as a measurement of this new package.

Companion [skills PR #30](https://github.com/gofastskill/skill/pull/30) contains the
relocated package and its exact 42-file coverage inventory. On server02, 12 Python tests,
namespace checks, regenerated-suite parity, and the payload vacuity guard passed.
A real ZIP installed as version 2.1.0; its copied invoice example and all three existing
v2 suites validated. Claude Sonnet 4.5 extended a suite through the explicitly supplied
installed entrypoint; caller validation passed and original files/positive row were preserved.
All relocated fixture bytes and the installed payload were checked unchanged. This scoped
exercise is not a new full live baseline or proof of automatic skill discovery.

The repaired Bash runner measured 93.1% line coverage; staging measured 100% Bash lines
and its exact embedded Python measured 100% statements. Bash branch instrumentation is
unavailable, not represented as passing. Six engine contract tests and 13 CLI
documentation/related tests passed again after relocation, as did formatting, source-size,
and diff checks. Earlier full regression, CLI coverage, two-tool authoring, and native
judge evidence remain applicable to unchanged implementation and assets. Raw relocation
evidence is retained beside this checkout under `work/`, including
`relocation-authoring-result.json`, `package-smoke-hHkVYA/`, `shell-coverage-v2/`, and
`embedded-coverage-6f4x9rtk/`.

The inventory below records the original implementation paths and evidence. Moved-file
coverage now belongs to the skills repository's consolidation record. This CLI PR retains:

| Current changed file | Verification |
| --- | --- |
| `CONTEXT.md` | existing authoring vocabulary and requirement mapping |
| `crates/fastskill-cli/src/main.rs` | recorded 91.22% line / 96.00% branch coverage; recursion-limit-only change |
| `crates/fastskill-evals/tests/documented_check_contract.rs` | six engine contract tests remain in the owning repository |
| `docs/adr/0011-skill-first-eval-authoring.md` | original skill-first workflow evidence |
| `docs/adr/0012-separate-outcomes-from-adherence.md` | original conversion/defect contrasts |
| `docs/adr/0013-skill-driven-eval-authoring.md` | original two-agent authoring and file handoff |
| `docs/requirements/eval-authoring-proposal.md` | original requirement mapping |
| `docs/requirements/eval-authoring-skill-prd.md` | original workflow evidence; package integration checked in companion repository |
| `docs/requirements/eval-authoring-support-prd.md` | ownership clarified; link and documentation checks |
| `docs/qa/eval-authoring-verification.md` | historical evidence retained; current ownership and limitations explicit |
| `webdocs/evals-quality/setup.mdx` | corrected semantics, credential boundary, and distribution links |

## Current evidence (supersedes historical status tables below)

- The imported Linux worktree was verified on `server02` at branch `feat/eval-authoring-skill`, checkpoint `11e673c24c1dc5233f4d5162788f18886d2ca168`. Fetching origin showed the local branch and `origin/feat/eval-authoring-skill` at the same commit with a clean worktree.
- Both authoring tools ran: Codex CLI 0.153.4 with `gpt-5.6-luna`, and Claude Code 2.1.269 with `claude-sonnet-4-5`. Generated suites validated against pinned FastSkill 0.9.230. Initial Claude attempts required trigger-scope, judge-path, schema, and template repairs.
- Claude continued the Codex-authored suite using files, repaired extra CSV columns and answer leakage, validated four cases/four checks, and recorded why changed prompts require new target evidence. Handoff cost reported by Claude: USD 0.2934362. Raw record: `work/claude-handoff.json` beside this checkout.
- A real Claude Sonnet 4.5 defect pilot consulted the target, returned `15.00`, and failed the independent `17.00` expectation with exit code zero and no execution error. Changing only the target to include the final item produced a passing rerun with the same criterion. Rescoring the corrected pilot's saved artifacts also passed without another model call.
- The conversion pilot passed the numeric outcome check, failed mandatory consultation, and failed the scratch-file check without gating because it was advisory. This demonstrates distinct outcome/adherence results, not a fully passing conversion suite.
- On `server02`, pinned FastSkill now reports `aikit`, `claude`, and `codex` available for the shipped example. This supersedes the earlier Windows `RUNTIME_UNKNOWN_ID` result for Codex while preserving it as historical prerequisite evidence.
- The six documented contract tests and four CLI asset tests pass on Linux. The four asset tests exercise the shipped suite, references and links, every purpose-built fixture branch, and the controlled contradiction failure.
- Repository-recommended process-isolated regression passed all 1,872 tests with one configured skip before the Clippy fix. After the one-line fix, 1,871 of 1,872 passed in one run; the sole failure read a stale shared `market-success` cache and passed when rerun with a fresh documented `FASTSKILL_CACHE_DIR`. The ordinary in-process Cargo runner produced current-directory cascades, while the same first failure passed alone; nextest is the valid repository runner.
- Strict workspace Clippy initially failed because current nightly promotes the recursion-depth diagnostic in unchanged `crates/fastskill-cli/src/main.rs` under `-D warnings`. Adding the compiler-recommended crate recursion limit fixed the gate. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, `cargo fmt --all -- --check`, `bash scripts/check-source-size.sh`, and `git diff --check` now pass.
- Instrumented FastSkill CLI coverage ran 897 tests with one configured skip and no failures. The changed production entrypoint has 686/752 covered lines (91.22%) and 48/50 covered branches (96.00%), satisfying the PRD's 90% line and 80% branch thresholds. Raw summary: `work/coverage-fastskill-cli.json`.
- Server02 uses Python 3.10 while the docs checker imports Python 3.11's `tomllib`. A `tomli` compatibility package was supplied only through `work/python310-compat`; the previously blocked documentation parity test then passed without repository-source changes.
- Native judge calibration used the owner-approved OpenAI-compatible gateway at `https://llm-gateway.tailb1f947.ts.net/v1`, credential variable `OPENAI_API_KEY`, and the gateway's sole advertised model, `tools-advanced`. With the same criterion and a 4,096-token response allowance, the preserved `15.00` defect scored 0.0 and failed, while the corrected `17.00` result scored 1.0 and passed; both completed with zero judge errors. Evidence is retained under `work/native-judge/` beside this checkout.
- The saved summaries recorded an absolute Windows skill-project path. Direct Linux judging failed with `EVAL_CONFIG_MISSING`; the global `--skills-dir` option does not relocate the recorded project root. Calibration therefore used untouched originals plus copied run artifacts whose `skill_project_root` alone was repaired to the preserved Linux project. The initial 1,024-token corrected attempt was retained as a judge-error example (`finish_reason` `length`) and was not counted as passing evidence.

Draft PR #333 contains the CLI work. Historical environment blockers below are retained as attempt history, not current claims.

This record keeps deterministic checks, live authoring exercises, and release gaps distinct. It must not be read as a release attestation while any mandatory row is incomplete.

## Compatibility and confirmed validation behavior

- Intended supported baseline: FastSkill 0.9.230 at `dd983e89e5fee977325b77ae386b879e8310ed26`, with `aikit-evals` pinned to `eb50f2c5509d4345cab65af0f7faed012ab0e557`.
- Local installed binary: FastSkill 0.9.208. It parsed the shipped invoice example with `fastskill eval validate --json`, reporting three cases and one deterministic check, but it predates the declared baseline and therefore does not establish 0.9.230 behavioral compatibility.
- Confirmed source contracts at the pin: per-case implicit trigger checks from `should_trigger`; exact `cases` selectors; explicit applicable `skill_invoked` precedence; contradiction rejection; required/advisory behavior; path-reference consultation proxy.
- No new validation defect requiring production-source changes was identified. The existing engine and CLI already enforce the material contracts; this change repairs maintained guidance and adds contract tests.

## Requirement-to-test mapping

| Requirements | Evidence | Status |
| --- | --- | --- |
| WF1, A1, AT1, ST4, ST6 | `invoice-no-evals`; compatibility/help inspection; generated Claude and Codex suites; pinned validator | passed |
| WF2, A2, AT3 | scripted outline/clarification record and unnecessary-step author decision; asset assertions | passed |
| WF3-WF4, A3-A5, AT4-AT5, ST1-ST5 | shipped example; suite-level review notes; semantics/examples references; six executed contract tests | passed |
| WF5, A6-A7, AT6 | pinned `eval validate`, malformed-suite repair, controlled contradiction, and executed native-judge contrast | passed; 15.00 rejected and 17.00 accepted by the same criterion |
| WF6, A8, AT9-AT10, ST8 | explicit pilot scope; saved authorized pilot; scripted unauthorized branch | passed |
| WF7, A9, AT7-AT8, ST7 | real defective and corrected target runs; prerequisite variants; separate outcome/adherence/missing evidence | passed |
| WF8, A10, AT11 | Claude continued and repaired Codex-authored files; corrected rerun and saved-evidence rescore; handoff record | passed |
| ST2-ST3 | pinned suite validation; six contract tests; four asset tests including intended contradiction failure | passed |

## Original changed-file coverage inventory (before consolidation)

Every path below is relative to the repository root. `assets` means the four executed `eval_authoring_assets_test` tests; `contracts` means the six executed `documented_check_contract` tests; `parity` means the executed documentation command/config/link checks; `workflow` means the saved Codex/Claude authoring, continuation, defect/correction, pilot, and rescore evidence summarized above.

| Changed file | Coverage evidence | Result or gap |
| --- | --- | --- |
| `.gitignore` | assets verify the eval-authoring tree is unignored while an unrelated `skills/*` path remains ignored | passed |
| `CONTEXT.md` | parity plus requirement mapping | passed |
| `crates/fastskill-cli/src/main.rs` | 897 instrumented CLI tests; 91.22% lines and 96.00% branches; strict Clippy | passed |
| `crates/fastskill-evals/tests/documented_check_contract.rs` | Cargo/nextest discovery; six meaningful positive and negative contract tests | passed |
| `docs/adr/0011-skill-first-eval-authoring.md` | WF1/AT1 workflow evidence and local-link parity | passed |
| `docs/adr/0012-separate-outcomes-from-adherence.md` | conversion pilot and contract evidence distinguish outcome/adherence | passed |
| `docs/adr/0013-skill-driven-eval-authoring.md` | two-tool authoring/continuation and CLI runtime evidence | passed |
| `docs/qa/eval-authoring-verification.md` | reconciled against saved raw evidence and server02 commands | passed; native judge evidence added |
| `docs/requirements/eval-authoring-proposal.md` | WF/AT mapping and parity | passed |
| `docs/requirements/eval-authoring-skill-prd.md` | WF1-WF8 and AT1-AT11 mapping above | passed |
| `docs/requirements/eval-authoring-support-prd.md` | ST1-ST8 mapping; assets, contracts, parity, judge contrast | passed |
| `skills/eval-authoring/SKILL.md` | structural validation, assets, workflow, prerequisite/preservation branches, judge contrast | passed |
| `skills/eval-authoring/references/examples.md` | assets, pinned suite validation, saved defect/correction contrasts | passed; example contrast remains honestly qualitative |
| `skills/eval-authoring/references/handoff.md` | assets and Claude-from-Codex continuation record | passed |
| `skills/eval-authoring/references/semantics.md` | assets, six contracts, pinned validation/runtime probe | passed |
| `skills/eval-authoring/examples/invoice-extraction/SKILL.md` | assets and pinned suite validation | passed |
| `skills/eval-authoring/examples/invoice-extraction/skill-project.toml` | assets assert isolation metadata; pinned validation accepts all three runtimes | passed |
| `skills/eval-authoring/examples/invoice-extraction/evals/checks.toml` | assets, pinned parser, judge path/schema assertions, evidence-only live endpoint/model override | passed; portable placeholders remain intentional |
| `skills/eval-authoring/examples/invoice-extraction/evals/judge-prompt.md` | assets assert output contract and expected-fact rendering; native contrast used the same contract shape | passed |
| `skills/eval-authoring/examples/invoice-extraction/evals/prompts.csv` | assets exercise all three rows and referenced fixtures | passed |
| `skills/eval-authoring/examples/invoice-extraction/evals/review-notes.md` | assets plus workflow evidence | passed |
| `skills/eval-authoring/examples/invoice-extraction/fixtures/basic.txt` | assets parse/reference it and pinned suite validates it | passed |
| `skills/eval-authoring/examples/invoice-extraction/fixtures/missing-number.txt` | assets parse/reference it and assert the independent null expectation | passed |
| `tests/cli/eval_authoring_assets_test.rs` | Cargo/nextest discovery; four tests pass; controlled invalid case fails for intended contradiction | passed |
| `tests/cli/mod.rs` | nextest discovers and runs all four asset tests | passed |
| `tests/fixtures/eval-authoring/defective-total/SKILL.md` | assets plus real 15.00-vs-17.00 failure and corrected rerun | passed |
| `tests/fixtures/eval-authoring/defective-total/ground-truth.md` | assets assert independent ground truth; real failure/passing correction | passed |
| `tests/fixtures/eval-authoring/environment-variants.md` | assets assert CLI/runtime/judge prerequisite branches; server02 runtime and native-judge probes | passed; unavailable and configured branches observed |
| `tests/fixtures/eval-authoring/existing-suite/SKILL.md` | assets and cross-agent continuation | passed |
| `tests/fixtures/eval-authoring/existing-suite/skill-project.toml` | assets and pinned validation | passed |
| `tests/fixtures/eval-authoring/existing-suite/evals/checks.toml` | assets and cross-agent repair/validation | passed |
| `tests/fixtures/eval-authoring/existing-suite/evals/prompts.csv` | assets and cross-agent repair/validation | passed |
| `tests/fixtures/eval-authoring/existing-suite/evals/user-note.md` | assets assert unrelated user note preservation | passed |
| `tests/fixtures/eval-authoring/existing-suite/fixtures/release.md` | assets and cross-agent suite validation | passed |
| `tests/fixtures/eval-authoring/invoice-no-evals/SKILL.md` | assets and two-tool from-scratch authoring | passed |
| `tests/fixtures/eval-authoring/invoice-no-evals/fixtures/ambiguous-date.txt` | assets and workflow coverage outline | passed |
| `tests/fixtures/eval-authoring/invoice-no-evals/fixtures/basic.txt` | assets and workflow generated suite | passed |
| `tests/fixtures/eval-authoring/invoice-no-evals/fixtures/missing-number.txt` | assets assert clarified null behavior | passed |
| `tests/fixtures/eval-authoring/invoice-no-evals/fixtures/multiple-currencies.txt` | assets and workflow coverage outline | passed |
| `tests/fixtures/eval-authoring/scripted-workflow.md` | assets assert inspect/outline/review/authorization/preservation branches | passed |
| `tests/fixtures/eval-authoring/unnecessary-step/SKILL.md` | assets assert outcome/adherence conflict fixture | passed |
| `tests/fixtures/eval-authoring/unnecessary-step/scripted-answers.md` | assets retain author decision | passed |
| `tests/fixtures/eval-authoring/unsupported-order/SKILL.md` | assets assert unsupported expectation is reported rather than weakened | passed |
| `webdocs/evals-quality/setup.mdx` | parity, six contracts, shipped example | passed |

## Commands and observed results

| Command or exercise | Result |
| --- | --- |
| `quick_validate.py skills/eval-authoring` | passed (`Skill is valid!`) |
| pinned FastSkill 0.9.230 `eval validate --all --json` in shipped example | passed; three cases, one check, and `aikit`, `claude`, `codex` available |
| `cargo test -p fastskill-evals --test documented_check_contract --locked` | six passed |
| `cargo test -p fastskill-cli --test cli_tests eval_authoring --locked -- --test-threads=1` | four passed |
| `cargo nextest run --workspace --all-features --locked --no-fail-fast` | pre-fix: 1,872 passed, one configured skip; post-fix: 1,871 passed and one stale-cache failure |
| isolated stale-cache failure with a fresh `FASTSKILL_CACHE_DIR` | passed; confirms shared cache contamination rather than branch behavior |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | passed after crate recursion-limit fix |
| `cargo fmt --all -- --check`; `bash scripts/check-source-size.sh`; `git diff --check` | passed |
| `cargo llvm-cov nextest -p fastskill-cli --all-features --locked --branch ...` | 897 passed, one configured skip; `main.rs` 91.22% lines and 96.00% branches |
| Codex CLI 0.153.4 `gpt-5.6-luna` and Claude Code 2.1.269 `claude-sonnet-4-5` authoring/continuation | completed; generated and repaired suites validated; sanitized records retained |
| defect, corrected-target, conversion, and saved-evidence rescore pilots | expected defect failure preserved; corrected and rescore passed; outcome/adherence split observed |
| gateway `GET /v1/models` using `OPENAI_API_KEY` | HTTP 200; sole advertised model `tools-advanced` |
| pinned `eval judge` on copied defect run with evidence-only judge config | judged one, zero errors, overall 0.0, suite failed |
| pinned `eval judge` on copied corrected run with the same judge config | judged one, zero errors, overall 1.0, suite passed |
| direct Linux judge of original cross-machine run | expected `EVAL_CONFIG_MISSING`; copied summary path repair preserved originals and enabled calibration |

## Release gaps

- Native judge calibration is complete. The evidence-only endpoint/model/key-variable override is intentionally not committed into the portable example, and no credential value is recorded.
- The unrelated `manifest_add_records_repository_intent_after_catalog_match` test can consume stale shared cache state when the test process inherits the default cache root. It passes with a fresh supported `FASTSKILL_CACHE_DIR`; this isolation defect is recorded but does not justify widening the eval-authoring implementation.
- Saved eval runs currently retain an absolute originating `skill_project_root`; judging a copied run on another operating system requires an evidence-copy path repair. The original artifacts were preserved, and this limitation does not invalidate the executed contrast.

The original calibration and local consolidation gates are complete. GitHub CI is a
separate check on the pushed revisions. Both PRs remain drafts; do not enable auto-merge
or merge them. The companion report explicitly records retained harness limitations.

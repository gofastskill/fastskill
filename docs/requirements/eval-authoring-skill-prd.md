# Skill-driven eval authoring

Status: draft for external review. Version: 0.2. Date: 2026-09-12.

Product and architecture direction is agreed in ADR-0011 through ADR-0013. This document specifies proposed delivery requirements; it does not claim implementation or validation. It supersedes the working authoring proposal for this delivery.

## Problem and intended outcome

FastSkill can execute, judge, rescore, and report evaluation suites across supported agent runtimes. Creating those suites still requires substantial human attention: selecting meaningful scenarios, defining correctness, learning file conventions, designing graders, and interpreting validation and pilot failures.

Provide an eval-authoring skill that works inside the user's chosen agentic tool. It should guide the agent from an existing target skill to a reviewable suite in FastSkill's supported formats, with pilot evidence where execution is possible. FastSkill remains a CLI and evaluation toolchain; it does not become a conversational agent.

The primary outcome is reduced human active time per useful case, including review and repair. The number of generated cases is not a success metric by itself.

## Users and initial scope

The primary user maintains a skill and needs to design its first evaluation suite. No session history or known failure is required. The user's authoring agent must be able to read the authoring skill, inspect relevant files, edit suite files, and invoke FastSkill CLI. Authoring support does not imply that the same agent is supported as an evaluation target.

V1 delivers the authoring skill, focused references and examples, and supporting documentation/validation corrections described in the companion PRD. It uses existing suite and CLI capabilities. If an intended check cannot be represented faithfully, the workflow records the limitation rather than silently weakening the expectation.

Out of scope: a FastSkill interview engine, an authoring model API integration, a draft-state service, new case schemas or grader types, automated session import, skill optimization, native ablation, MCP simulation, new sandbox guarantees, and monetary spending caps. Necessary engine extensions discovered during development require separate proposals.

## Before and target state

| Area | Before | Target |
| --- | --- | --- |
| Starting point | Author learns formats and constructs cases | Agent inspects a target skill and proposes coverage |
| Human contribution | Scenario construction, syntax, commands, debugging | Intended behavior, consequential ambiguities, review |
| Authoring environment | No dedicated supported workflow identified | User's chosen agent plus FastSkill authoring skill |
| Artifacts | Existing CSV/TOML, prompts, fixtures, run evidence | Same execution formats, with concise human-readable review notes |
| Validation and execution | CLI capabilities available separately | Agent invokes those capabilities and explains evidence |
| Unsupported expectations | Easy to approximate accidentally | Explicit limitation tied to the affected behavior |

Claude plugin evals already provide assisted suite generation and pilots. This delivery aims to narrow that authoring gap while preserving choice of authoring agent and access to FastSkill's existing cross-runtime measurements and historical scorecards. It does not claim parity in grader expressiveness, plugin integration, mocks, ablation, or execution isolation.

## Responsibility boundary

| Responsibility | Owner |
| --- | --- |
| Conversation, clarification, user review, execution authorization | User's authoring agent, guided by the authoring skill |
| Layout guidance, workflow instructions, examples | Eval-authoring skill and maintained references |
| Supported formats, parsing, deterministic validation | FastSkill CLI and relevant existing libraries |
| Trial execution, configured judging, scoring, evidence | Existing eval toolchain |
| Definition of intended outcome where judgment is needed | Author |

The authoring skill cannot establish machine-enforced guarantees merely by instructing the agent to follow a procedure. No prose note is a CI attestation. Existing agent authentication is not proof that separately configured eval judges are available.

## Reference user workflow for implementation and testing

This workflow is a normative acceptance reference for the authoring skill and CLI integration. Implementers must create purpose-built target skills and fixtures to exercise it; testing only the authoring instructions' text or isolated CLI commands is insufficient. The example conversations illustrate intent, not exact wording that tests must match.

The user stays in their chosen agentic tool. All conversation below is conducted by that agent. FastSkill only receives explicit CLI invocations and returns results.

| Step | User and authoring-agent interaction | Implementation and test evidence |
| --- | --- | --- |
| WF1: Start | User: "Help me design eval tests for my invoice-extraction skill." Agent loads the eval-authoring skill, inspects the target and resources, and checks CLI compatibility. | Loaded skill/version, relevant inspected inputs, actual version/help output; no target suite initially exists. |
| WF2: Outline | Agent proposes ordinary invoices, missing fields, ambiguous dates, multiple currencies, and unrelated requests. It asks: "When the invoice number is missing, should extraction return null or fail?" User confirms null and reviews coverage. | Coverage outline precedes detailed suite generation; consequential answer is retained and no fixed questionnaire is required. |
| WF3: Generate | Agent writes supported prompts, checks, judge rubrics where needed, and fixtures. It distinguishes extraction correctness from triggering or required procedure. | Real generated files parse; missing-number criterion reflects null; trigger settings match engine semantics; unsupported expectations are explicit. |
| WF4: Review | Agent presents the suite together: each case's intent, expected result, and check method. It asks separately only about remaining ambiguities. | Suite-level review and targeted questions; no mandatory approval loop for every row. |
| WF5: Validate and challenge | Agent invokes CLI validation, repairs malformed files, and challenges graders with acceptable and unacceptable examples. | Actual validation output; labels tied to confirmed criteria; executed calibration distinguished from qualitative review or unavailable tooling. |
| WF6: Pilot authorization | If not already authorized, agent proposes "six cases, one trial each, on the selected runtime" and explains that this consumes model usage. The user can change scope. | Agent, not CLI, asks when needed. Prior authorization is respected. Invoked cases/runtimes/trials match the agreed scope; no unsupported cost guarantee. |
| WF7: Execute and explain | Agent invokes FastSkill. It explains which cases passed, which failed, and which could not be measured, linking the evidence. | Real run artifacts and results. The seeded ambiguous-date failure is identified without treating missing evidence as passing. |
| WF8: Refine and hand over | User: "Make the date criterion clearer and rerun the affected case." Agent updates the criterion and performs an appropriate supported rescore or rerun. It hands over files, evidence, and unresolved limits. | Unrelated user content is preserved; a new target run occurs only when needed; no criterion is weakened merely to obtain a pass; another authoring agent can continue from files. |

Expected end state: editable suite and fixtures; review notes; actual validation/pilot artifacts; explicit target failures and unsupported expectations; and clear identification of anything still unverified. Target failure is compatible with successful test authoring.

Failure branches must include missing CLI/runtime/judge prerequisites, incompatible CLI version, malformed generated configuration, unsupported checks, missing calibration capability, and a user changing the pilot scope. Preserve useful work in each branch and report the precise limitation.

## Functional requirements

### A1 — Inspect before asking

The authoring agent identifies the selected target skill, inspects its instructions and relevant supporting files, and checks the installed CLI version. It consults compatible references and actual help/validation output rather than assuming a command or field exists. The workflow does not require a specific authoring vendor, subscription, or model.

### A2 — Propose coverage, then clarify

Present a concise coverage outline before generating a large suite. Consider useful outcomes, positive and negative triggering, boundaries, and failure handling where relevant. Ask only questions whose answers materially change tests. Review the outline before proceeding to the detailed suite; avoid a fixed questionnaire and arbitrary case-count quotas.

### A3 — Separate outcomes and adherence

Each proposed expectation identifies whether it measures outcome correctness or skill adherence. A prescribed method is mandatory only when the author identifies it as a requirement. The agent must surface a conflict between a skill instruction and the intended outcome instead of silently resolving it.

The reviewed engine generates trigger checks from `should_trigger`. V1 must explain and respect that behavior, including explicit-check overrides and contradiction validation. It must not label a case outcome-only if the emitted configuration still has a mandatory adherence gate. If an intended scoring policy cannot be represented, identify it as unsupported; do not alter engine semantics under this PRD.

### A4 — Write supported, reviewable files

Generate ordinary suite files, judge prompts, and fixtures using documented supported formats. Preserve existing user files and show material changes when editing an existing suite. Retain concise review notes identifying coverage, requirement origins, assumptions, and unsupported expectations. These notes are ordinary documentation, not a new suite schema or managed lifecycle.

### A5 — Review the suite together

Show the proposed cases and criteria as a coherent suite. Request individual clarification for ambiguities or questionable examples, rather than approval of every row. Review acceptance does not imply that the target skill passed or that all graders were validated.

### A6 — Challenge graders with contrasting outcomes

Propose acceptable and unacceptable example outcomes, with labels grounded in author-confirmed criteria or independently verifiable facts. Record the reason for each label. Synthetic examples and a second model's agreement are not independent ground truth.

Exercise actual graders against contrasting evidence where existing supported interfaces permit it. Where they do not, distinguish qualitative review from executed calibration and preserve the missing verification explicitly. Never fabricate an official run artifact to pretend a grader was exercised. Adding a standalone calibration command is outside v1 and may be proposed separately.

### A7 — Validate and repair

Invoke existing config validation and runtime observability checks appropriate to the selected targets. Fix authored formatting/reference mistakes and repeat the relevant check. Separate invalid configuration, missing prerequisites, unobservable measurements, and actual target failures. Validation success alone is not evidence of behavioral correctness.

### A8 — Let the authoring agent authorize pilots

The agent uses existing user authorization or asks about a concrete pilot scope when authorization is unclear. Scope identifies cases, evaluation runtimes, and trial counts. FastSkill receives explicit command parameters; it does not interpret the conversation, ask consent questions, or maintain an approval store.

Pilot scope bounds work, not monetary cost. Do not promise a budget cap unsupported by the CLI. Missing evaluation-target or judge credentials must be reported without treating the authoring agent's credentials as interchangeable.

### A9 — Preserve honest completion evidence

Aim for a piloted suite. Report independently: author review, configuration validation, execution attempted/completed, grading/calibration evidence, target verdicts, and unresolved limitations. A target failing a useful test is a valid authoring result. Missing prerequisites leave an unverified draft; they must not be represented as passing or fully verified work.

Link actual run artifacts where available. Human-readable completion notes are not consumed as trusted verification by CI. Existing machine-readable artifacts retain their current meanings.

### A10 — Support continuation through files

Another capable authoring agent can continue from the suite and review notes without reconstructing a proprietary conversation state. The workflow must not require a hosted authoring service, session database, or FastSkill-owned interview state.

## Deliverables

- An installable eval-authoring skill with concise workflow instructions and focused supporting references.
- Examples covering positive/negative triggering, outcome judging, and an unsupported expectation handled explicitly.
- Instructions for validation, authorized pilot execution, result review, and handoff through ordinary files.
- A declared CLI compatibility range backed by checks from the companion PRD.
- Evidence from end-to-end authoring exercises and a record of unresolved limitations.
- Purpose-built test skills, fixture data, scripted user answers, and workflow tests covering WF1–WF8 and the acceptance scenarios below.
- A changed-file coverage inventory and reproducible validation evidence meeting the testing requirements below.

## Acceptance scenarios

| ID | Scenario | Required evidence |
| --- | --- | --- |
| AT1 | Target skill has no evals | Coverage outline, relevant clarification, generated suite, validation result |
| AT2 | User's agent is not an eval backend | Authoring succeeds; execution targets only supported runtimes |
| AT3 | Skill instruction conflicts with intended outcome | Conflict surfaced; resulting criteria reflect author's decision |
| AT4 | Positive and negative triggering cases | Expectations match actual current scoring semantics |
| AT5 | Required assertion is not supported | Limitation reported; no misleading weaker substitute |
| AT6 | Known acceptable and unacceptable examples | Labels explained; actual calibration distinguished from qualitative review |
| AT7 | CLI, target runtime, or judge prerequisite missing | Work retained with precise unmet prerequisite and unverified status |
| AT8 | Target skill fails a well-formed case | Failure retained as evidence; criteria not weakened merely to obtain a pass |
| AT9 | Pilot scope already authorized | No redundant permission interview; explicit scoped command invoked |
| AT10 | Pilot scope not authorized | Authoring agent asks; CLI remains non-conversational |
| AT11 | Author switches agent | Suite and notes are sufficient to resume without a proprietary state service |

## Validation and success measures

Run documented authoring exercises with at least two different capable authoring tools as a release check. Record versions, prerequisite setup, generated artifacts, validation results, human interventions, and paid runs. This is evidence of portability, not a universal compatibility claim. The same workflow and behavioral expectations must apply across tools; exact generated prose and case counts need not match.

### Test skills and fixtures to create

Create isolated, synthetic test assets in the chosen repository's test/fixture layout. They must not depend on private production data or change the developer's global skill installations.

| Test asset | Purpose and required exercises |
| --- | --- |
| Invoice-extraction skill with no evals | Main WF1–WF8 flow; text invoice fixtures with known values, missing number, ambiguous date, multiple currencies, and an unrelated request. |
| Target skill with a deliberate behavioral defect | Ensure the generated test detects an incorrect outcome and the authoring agent retains the failure instead of changing expectations. The defect and expected outcome must be established independently of the generated grader. |
| Target skill with an unnecessary prescribed step | Exercise the distinction between useful outcome and adherence; scripted author answers settle whether the step is mandatory. |
| Target requiring an unsupported assertion | Verify honest limitation handling without invented commands, formats, or misleading substitute checks. |
| Existing suite with unrelated user edits | Verify preservation, targeted edits, rescoring/rerun selection, and handoff between agents. |
| Environment/configuration variants | Missing credentials or runtime, incompatible version, invalid references, and unavailable observations; use controlled stubs for deterministic failure tests where appropriate. |

Keep expected labels, known defects, and scripted user answers outside the target's visible execution inputs where feasible. Separate test-owned ground truth from agent-generated outputs. Real workflows must use the actual authoring skill and CLI; stubs are appropriate for controlled failure branches, not as replacements for all end-to-end evidence.

### Authorized live-test tools and models

The project owner permits implementation testing with the following available-tool choices. These are permitted options, not a requirement to run every combination or a claim that each alias is installed or supported as an eval backend.

| Tool | Permitted model/provider choice | Setup requirement |
| --- | --- | --- |
| pi | Models available through the team's LiteLLM endpoint | Discover configured endpoint/model aliases and use an existing approved token; request secure provisioning if absent. |
| Cursor | Available Grok models | Verify the installed tool's actual supported model identifier and authentication. |
| Codex | GPT Luna, as requested by the owner | Resolve the actual available identifier; do not guess an alias or silently substitute another model. |
| Claude | Sonnet 4, as requested by the owner | Verify the actual available identifier and credentials; report unavailability. |

These tools can be authoring agents. Use them as evaluation targets only where the FastSkill backend supports them and can observe the selected checks. FastSkill's native judge endpoint/model configuration is separate from authoring-agent authentication and must satisfy the existing judge contract.

Use small, recorded pilot batches within this authorized testing scope; no new blanket approval is needed merely to exercise an allowed combination. Request missing credentials or environment permissions when required, never print tokens, and never place tokens in fixtures, prompts, logs, PRDs, or committed artifacts. Record endpoint identity/model IDs without secret values. If fewer than two authoring tools can run, retain a blocked portability release check rather than claiming completion from one tool.

### High coverage for every touched file

Every added or modified file in the implementation, including skill instructions, supporting references, fixtures, scripts, configuration, and code, must have explicit test or verification coverage appropriate to its contents. Submit a changed-file inventory mapping each file to test IDs, results, and remaining gaps. A high repository-wide average is not a substitute for coverage of an individual changed file.

For this PRD, high executable-code coverage means at least **90% line coverage per added/modified executable source file**, and **80% branch coverage where supported by the toolchain**, with stricter repository gates taking precedence. Cover all changed contract behaviors and critical failure paths explicitly; numeric coverage alone is insufficient. Test files themselves need discovery/execution evidence and meaningful assertions, not recursive tests of tests. Document instrumentation limitations; do not represent unmeasurable branch coverage as passing. Uncovered behavior or unmet thresholds remains a release gap unless explicitly waived by the owner.

For non-executable files, do not claim a source-line percentage. Require:

- Skill instructions: every normative behavior mapped to an executed workflow/acceptance scenario, including negative and recovery branches.
- References and Markdown: all command/config examples validated against the supported CLI, local links resolved, and material behavioral claims mapped to tests or verified code contracts.
- Fixtures/templates/configuration: every supplied asset parsed or exercised; invalid examples fail for the intended reason; expected labels and outcomes checked independently.
- Changed tests/build/CI configuration: demonstrate that intended tests run and that a controlled failure is detected by the relevant gate where applicable.

Avoid tests that only search for wording in SKILL.md or mirror implementation logic. Observe emitted suite files, CLI invocations, exit/results semantics, preserved edits, and real failure detection. Separate deterministic CI checks from live-model exercises; record every live attempt and flaky result instead of selectively reporting successful retries.

### Developer handoff and definition of done

The implementation handoff must contain the installable authoring skill and references; test targets and fixtures; requirement-to-test and changed-file coverage mappings; reproducible commands; CLI/agent/model versions; sanitized live-run artifacts; and a clear list of limitations. Run relevant existing regression and command/document parity checks in addition to new coverage.

Release is ready only when WF1–WF8, AT1–AT11, and the companion support criteria have evidence; the two-tool portability check and changed-file coverage gates pass; and known limitations are explicit. A deliberately failing target is expected evidence and does not fail authoring acceptance when correctly detected. This PRD update itself does not claim those implementation tests have been run.

Measure active human minutes per accepted useful case, clarification count, correction/rejection rate, layout errors caught by validation, and time spent repairing graders. Compare comparable tasks with manual FastSkill authoring and the reviewed Claude-assisted workflow when available. A quantitative improvement target must follow a measured baseline; no percentage advantage is claimed now.

## External review questions and implementation investigations

- Does the acceptance matrix distinguish enough evidence to prevent an unverified suite appearing release-ready?
- Is the usefulness of v1 acceptable when full contrast calibration is unavailable for a grader?
- Determine skill distribution repository/path and supported versions using the maintained distribution conventions. No new installation mechanism is presumed.
- Confirm the smallest safe representation of review notes and examples, without creating a new engine schema.
- Identify current-interface limitations with reproducible cases; route required new capabilities to separate proposals.

These are review and feasibility items. The user/agent/CLI responsibility boundary is settled.

## Related decisions and requirements

- [ADR-0011: skill-first authoring](../adr/0011-skill-first-eval-authoring.md)
- [ADR-0012: outcomes and adherence](../adr/0012-separate-outcomes-from-adherence.md)
- [ADR-0013: skill and CLI boundary](../adr/0013-skill-driven-eval-authoring.md)
- [Authoring support PRD](eval-authoring-support-prd.md)
- [Existing judge requirements](eval-judge.md)
- [Existing measurement-integrity requirements](eval-measurement-integrity.md)

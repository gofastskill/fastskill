# Documentation and validation support for eval authoring

Status: draft for external review. Version: 0.2. Date: 2026-09-12.

This companion to the eval-authoring skill PRD covers the narrow tooling and documentation work needed to make its instructions reliable. It does not introduce an authoring orchestrator or expand evaluation expressiveness.

## Problem

A portable authoring skill must teach the behavior of the installed FastSkill release. Documentation drift can cause it to generate valid-looking suites that measure the wrong thing. The reviewed 0.9.230 checkout already contains material contradictions: its setup guide calls `should_trigger` inert, says deterministic checks necessarily apply globally, and describes no-check scoring as process-exit-only. The pinned engine generates per-case trigger checks and supports case selectors.

The source baseline is FastSkill `dd983e89e5fee977325b77ae386b879e8310ed26` and its aikit pin `eb50f2c5509d4345cab65af0f7faed012ab0e557`. Revalidate against the implementation checkout before making fixes; these findings are not assertions about every version.

## Objective and boundary

Make supported authoring formats, commands, and semantics discoverable, correct, and testable. FastSkill remains responsible for validating explicit files and command parameters, producing evidence, and explaining errors. The user's authoring agent remains responsible for conversation, intent, and authorization.

Use existing help, command specification, documentation, and validation surfaces. Add or repair validation only for existing format contracts. New graders, case schemas, calibration commands, budget controls, and runtime isolation changes need separate scope and are not implied here.

## Requirements

### S1 — Correct the maintained reference

Audit and correct eval setup and command examples against pinned source behavior. Cover implicit trigger checks, explicit-check precedence, case selectors, contradiction handling, consultation evidence by backend, required/advisory checks, and scoring thresholds. Explain that path-reference evidence is a proxy for consultation, not proof that instructions were followed.

Document the distinction between configuration validation, actual execution, judgment, and rescoring. Do not claim that a supported command validates evidence it does not inspect.

### S2 — Keep ownership of rules clear

The parser and engine define enforceable formats and scoring. Maintained references explain them; the authoring skill links to those references instead of copying large independent schemas. Examples must identify their supported release range. The skill must not assume features from a later CLI merely because its own instructions are newer.

Determine whether current version/help/spec surfaces provide enough compatibility information before proposing additions. Do not introduce a new discovery API by default.

### S3 — Provide actionable existing-contract validation

Inventory existing validation behavior before implementing anything. Where a currently supported field has inadequate validation, propose a narrow correction with a reproducer. Errors should identify the affected file, case/check/judge where available, and the violated rule. Preserve established CLI and machine-readable output contracts unless a compatibility change is separately reviewed.

Do not convert missing runtime evidence into a passing check, or missing measurements into zero scores. Do not add conversational prompting or approval persistence.

### S4 — Ship executable examples

Maintain small fixtures exercising positive/negative triggering, case-scoped checks, optional adherence where representable, a configured outcome judge, and an unsupported expectation described as such. Keep secrets out of examples and use explicit placeholders for separately supplied endpoint credentials.

Validate syntax and deterministic behavior without paid calls. Any live agent or judge exercise is a separate, explicitly scoped validation activity. Do not substitute a model-generated success narrative for command output.

### S5 — Connect authoring guidance to real evidence

Examples and the skill must show how to interpret config errors, backend observability exclusions, execution failures, and target-quality failures separately. Document which current interfaces can exercise graders on saved evidence and which cannot. If a standalone calibration path does not exist, report that gap rather than inventing a command or forging artifacts.

CI decisions continue to rely on existing structured results and exit semantics. Authoring notes do not introduce a trusted `verified` flag or new gate.

### S6 — Preserve execution boundaries

Correct any guidance that confuses scratch-workspace skill isolation with OS-level confinement. Describe actual permissions and backend limitations of the supported version. The authoring agent obtains any needed user authorization and invokes explicit CLI arguments. This PRD neither adds a sandbox nor promises a monetary cap.

## Acceptance criteria

| ID | Criterion |
| --- | --- |
| ST1 | Maintained authoring references no longer contradict the supported release on trigger expectations, selectors, or no-check behavior |
| ST2 | All shipped suite examples parse under the declared supported versions; expected invalid examples fail for the intended reason |
| ST3 | Deterministic tests demonstrate positive/negative expectations and explicit-check contradiction behavior |
| ST4 | Authoring instructions use actual help/spec/validation interfaces and no invented CLI commands |
| ST5 | Documentation and examples distinguish outcome checks, adherence checks, and aggregate verdicts accurately |
| ST6 | Compatibility failures are explicit; silently assuming newer fields is not taught |
| ST7 | Missing prerequisites, absent observations, and quality failures are explained separately |
| ST8 | No conversational interview, approval state store, or new engine capability is introduced under this support work |

## Delivery and ownership

The installable workflow is part of `gofastskill/skill` under `fastskill/`:
`SKILL.md` routes to `references/eval-authoring.md` and its supporting references/example.
Authoring fixtures and asset tests belong in that repository's `evals/authoring/` and
`scripts/` directories. This CLI repository owns the engine contract tests, public command
reference, requirements, and verification record. See the
[consolidation assessment](https://github.com/gofastskill/skill/blob/main/specs/002-eval-authoring-consolidation.md).

### Workflow and coverage obligations

Use the primary PRD's [reference workflow](eval-authoring-skill-prd.md#reference-user-workflow-for-implementation-and-testing), test skills, live-tool choices, and [changed-file coverage requirements](eval-authoring-skill-prd.md#high-coverage-for-every-touched-file) as mandatory implementation and testing references. They apply equally to files changed under this support PRD and to any associated upstream fixes.

Every changed file must appear in the handoff coverage inventory. Executable source files require at least 90% per-file line coverage and 80% branch coverage where instrumentation supports it, or stricter existing repository gates. Skill and documentation files require behavioral/example/link verification rather than artificial line percentages. Every supplied fixture must be exercised, and test/CI changes need execution evidence. Unmet thresholds and unavailable checks are visible release gaps, not silently skipped success.

In addition to ST1–ST8, demonstrate that repaired guidance works during real skill-driven authoring: generate and validate suites from purpose-built target skills, preserve deliberately detected failures, and distinguish unsupported expectations from valid passing checks. Use controlled deterministic tests for parser/validator defects and scoped live exercises for agent behavior. The owner's permitted tools and models are recorded in the primary PRD; credentials must be provisioned securely and model identifiers verified before use.

Implement reference/example corrections in FastSkill and the chosen skill distribution location. Changes to shared parsers or validation belong in the owning library when a confirmed existing-contract bug requires them; FastSkill should not duplicate upstream evaluation semantics. Any upstream change must be pinned and verified in the consuming checkout.

Before development, identify the release compatibility matrix and enumerate confirmed validation defects. A new validation feature is not justified merely because it would be convenient for an agent.

## Risks and review items

- A prose reference can drift again: cover material behavioral claims with focused tests or existing parity checks where appropriate.
- Implicit trigger checks can conflict with author expectations of outcome-only scoring: demonstrate the actual configuration or mark the requested policy unsupported.
- Pilots may be expensive or unavailable: deterministic documentation checks must remain possible without live credentials.
- Broader evaluation gaps may emerge: document them separately rather than expanding the authoring release silently.

## Related material

- [Eval-authoring skill PRD](eval-authoring-skill-prd.md)
- [ADR-0013](../adr/0013-skill-driven-eval-authoring.md)
- [Eval setup reference](../../webdocs/evals-quality/setup.mdx)
- [Eval command reference](../../webdocs/cli-reference/eval-command.mdx)
- [Observability preflight](../../crates/fastskill-cli/src/commands/eval/observability.rs)

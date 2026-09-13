---
name: eval-authoring
description: Design, validate, pilot, and hand off FastSkill evaluation suites for an existing skill. Use when creating or repairing prompts.csv, checks.toml, judge rubrics, fixtures, or review notes; do not use to add unsupported evaluator features.
---

# Author FastSkill evals

Turn an existing target skill into a reviewable suite in the formats supported by the installed FastSkill CLI. The user chooses the authoring agent. FastSkill validates explicit files, runs selected evaluation targets, and returns evidence; it does not conduct the conversation or remember approval.

## Start from evidence

1. Locate the target `SKILL.md`, its project root, relevant references, and any existing eval files. Preserve unrelated content and local edits.
2. Run `fastskill --version`, `fastskill eval --help`, and `fastskill eval validate --help`. Compare the installed version with the compatibility note below; treat live help and validation as authoritative for command availability.
3. Read the maintained [Eval Setup](../../webdocs/evals-quality/setup.mdx) and [eval command](../../webdocs/cli-reference/eval-command.mdx) references from the matching checkout. Read [references/semantics.md](references/semantics.md) before choosing checks or judges.

This skill is maintained against FastSkill `0.9.230` and the pinned `aikit-evals` revision in that release. On another version, inspect its matching references and validate every field; report the suite as compatibility-unverified if the relevant contracts cannot be confirmed. Never assume that newer skill guidance adds a CLI field.

## Design before generating

After inspection, propose a short coverage outline spanning useful outcomes, positive and negative triggering, boundaries, and failure handling where relevant. Ask only questions whose answers materially change a case or criterion. Retain the answer and review the outline before creating a large suite.

For every expectation, label it in review notes as:

- **outcome**: the result must satisfy the user's acceptance criterion; or
- **adherence**: the agent must consult or follow a prescribed method.

Make adherence mandatory only when the author says the method is required. Surface conflicts between target instructions and intended outcomes. Do not silently choose one or weaken an unsupported expectation.

## Author and review

Use ordinary `skill-project.toml`, CSV, TOML, Markdown prompt, and fixture files. Prefer deterministic checks for evidence they actually observe and a judge only for outcome criteria that need semantic review. Keep expected labels and known defects outside inputs visible to the evaluated target where practical.

Check CSV row widths against the header and inspect parsed expected values. Ask the target to solve the task without supplying the expected answer in its prompt; a requested output format should use placeholders rather than the ground-truth result. File validation alone does not establish these properties.

For a mixed positive/negative suite, normally omit explicit `skill_invoked` checks and let each row's `should_trigger` value generate the correctly polarized check. Never add one global positive check: it applies to negative rows too and the supported validator rejects the contradiction. If explicit trigger checks are necessary, scope them to exact case IDs and keep their polarity equal to those rows.

Resolve judge prompt paths relative to `checks.toml`, not the project root. A judge can grade only evidence rendered into its prompt. Put independently established expected values in extra CSV columns such as `expected`, render them with `{{case.expected}}`, and do not claim a judge can inspect an unchanged fixture merely because it exists in the workspace.

In the supported schema, repeat singular `[[judge.criterion]]` tables and render the evaluated answer with `{{trial.final_answer}}`. Validate these names instead of guessing plural tables or generic output fields.

Write concise `evals/review-notes.md` recording:

- coverage and the origin of each expectation;
- outcome versus adherence classification;
- author-confirmed decisions and remaining assumptions;
- unsupported expectations and missing prerequisites;
- validation, calibration, pilot, and target-verdict evidence as separate statuses.

Present the cases and criteria as one coherent suite. Ask targeted follow-ups only for unresolved ambiguities; do not require approval row by row. For concrete layouts and limitation wording, read [references/examples.md](references/examples.md).

## Validate, challenge, and pilot

Run `fastskill eval validate` first, then validate selected runtime observability with `--agent <id>` or `--all`. Repair authored syntax and references, rerunning the failing check.

Before the default isolated pilot, ensure `skill-project.toml` declares `[metadata].id` for the target skill. File validation can succeed without this pilot prerequisite.

Keep these categories distinct:

- invalid configuration;
- missing CLI, runtime, endpoint, or credential prerequisite;
- measurement not observable on a backend;
- execution error;
- well-formed quality failure.

Challenge each judge with at least one author-confirmed or independently verifiable acceptable outcome and one unacceptable outcome. Record the label reason. If the installed interfaces cannot execute a standalone contrast, call it qualitative review and explicitly leave calibration unverified; never forge run artifacts. `eval judge` can judge or rejudge saved run evidence, but it is not a general calibration command.

Before a paid or model-backed pilot, use existing user authorization or propose an explicit scope naming cases, evaluation runtimes, and trial counts. If unclear, ask once. Do not promise a monetary cap. Authoring-agent credentials do not prove that evaluation runtimes or native judges are configured.

Run only the authorized scope with explicit parameters, for example:

```bash
fastskill eval run --agent codex --case invoice-basic --trials 1 --output-dir ./eval-runs
```

Explain actual artifacts and distinguish pass, fail, error, excluded/unobservable evidence, and missing judgment. A useful test that exposes a target defect is successful authoring evidence; do not change the criterion merely to obtain a pass.

## Refine and hand off

Use `eval score` when only deterministic checks changed and saved evidence remains suitable, `eval judge --rejudge` when only judge configuration changed and saved evidence remains suitable, and rerun the target when its prompt, fixture, target skill, runtime/model, or required evidence changed. Preserve unrelated edits.

Hand over editable suite files, fixtures, review notes, exact commands, run artifact paths, observed versions/model IDs, and unresolved limitations. Another capable authoring agent must be able to continue from files without conversation state. Use [references/handoff.md](references/handoff.md) for the completion record.

## Boundaries

Do not invent checks, schemas, graders, calibration commands, interview services, approval stores, native ablation, mocks, sandbox guarantees, or cost ceilings. Skill isolation controls discovered skills, not OS-level filesystem or network confinement. When the engine cannot faithfully represent an intended assertion, keep it as an explicit unsupported expectation and propose any engine extension separately.

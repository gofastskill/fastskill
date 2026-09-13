# FastSkill 0.9.230 eval semantics

Use this reference while mapping expectations to the maintained Eval Setup page. It highlights decisions that are easy to get wrong; it is not a duplicate schema.

## Triggering and case scope

`should_trigger` is executable scoring input in this release. For each case, the engine creates a required `skill_invoked` expectation with the same polarity unless an explicit `skill_invoked` check applies to that case. An explicit applicable check replaces the implicit one. Validation rejects an explicit polarity that contradicts the case's `should_trigger` value.

Every deterministic check accepts an optional `cases = ["exact-id", ...]` selector. It applies only to those exact case IDs. Scope mixed positive and negative behavior deliberately.

In a mixed-polarity suite, omit explicit `skill_invoked` checks unless they add necessary specificity. A global `expected = true` check applies to the negative rows and contradicts them; comments do not create per-case polarity. When explicit checks are used, select exact cases and match each selected row's `should_trigger` value.

`skill_invoked` recognizes a structured `Skill` invocation or, on supported decoders, a tool input that refers to the staged skill-document path. The path form is evidence of consultation, not proof that the agent read or followed the instructions. Measure outcome correctness separately.

With no checks file—or an empty check list—the implicit `should_trigger` expectation still applies. Process exit alone is not the verdict.

## Verdicts and evidence

Required deterministic checks are all-or-nothing for a trial. `required = false` keeps a check observable without gating. Across trials, `pass_threshold` controls the aggregated case result. Judge criteria have their own normalized scores; a judge gates only when it declares `min_score`.

Validation proves that configured files parse and that file-only contracts are consistent. Runtime validation also checks installed target availability and preflight observability. It does not execute the skill, contact judge endpoints, or establish outcome quality.

Judge `prompt_file`, `system_prompt_file`, and `retry_prompt_file` paths are relative to the directory containing `checks.toml`. A judge sees only rendered template variables. Unchanged fixture contents are not implied by `{{trial.workspace_diff}}`; provide test-owned expected facts through an extra CSV column and render `{{case.<column>}}` when the rubric needs them.

`eval run` records target execution evidence. `eval judge` applies declared judges to completed non-error trials. `eval score` reuses saved trace/workspace evidence for deterministic checks. Missing observations, errored trials, and missing judgments are not passes or zero-quality observations.

## Isolation and authorization

Default isolation stages the selected skill in a scratch workspace and suppresses ambient skill discovery where the backend supports it. This is measurement isolation, not an OS sandbox. Inspect the recorded isolation fidelity.

The authoring agent obtains user authorization for concrete model-backed work. FastSkill accepts explicit runtime/case/trial arguments and has no conversational consent state. Scope does not enforce a monetary ceiling.

# Assisted eval design for skills

Status: working discussion record, superseded for delivery scope by the [eval-authoring skill PRD](eval-authoring-skill-prd.md) and [support PRD](eval-authoring-support-prd.md). Product boundaries are agreed; PRDs are drafts for external review. See ADR-0011 through ADR-0013.

## Agreed authoring principles

- Immediately propose a short coverage outline after inspecting the skill. Ask only questions whose answers materially change the tests; avoid a fixed questionnaire.
- Measure outcome correctness separately from skill adherence. Adherence becomes mandatory only when the author identifies it as a requirement; do not silently promote every skill instruction to a correctness condition.
- Aim to deliver a piloted suite. If execution prerequisites are missing, preserve an explicitly unverified draft rather than claiming completion of verification.
- Exercise graders against acceptable and unacceptable examples. The skill passing its own tests is not a condition of successful authoring.

The authoring conversation runs in the user's chosen agentic tool, guided by an eval-authoring skill. FastSkill CLI and the evaluation engine own supported formats, validation, execution, and evidence. Drafts are ordinary files under version control; a dedicated interview engine and draft-state service are deferred.

Additional agreed decisions:

- Review the coverage outline first, then the suite as a whole. Request individual clarification for ambiguous requirements or questionable grading examples rather than requiring approval of every case.
- The authoring agent may propose graders and calibration examples. Labels must derive from author-confirmed criteria or independently verifiable facts; synthetic examples are evidence, not proof of correctness.
- The authoring skill checks CLI version compatibility and consults authoritative format references and validation results. Unsupported versions receive an explicit compatibility message; exact supported versions are an implementation deliverable.
- The user's authoring agent handles all conversational authorization. It uses existing authorization or asks once about the proposed pilot when necessary, then invokes explicit CLI parameters. FastSkill neither interprets conversational consent nor stores conversational approvals.
- A monetary ceiling is not required for this authoring change. An agreed set of cases, runtimes, and trial counts bounds the pilot's scope, not its actual monetary cost. Only controls implemented by the CLI may be described as enforced.

V1 builds the authoring skill first using existing suite/CLI capabilities, with documentation and existing-contract validation fixes. Unsupported intentions are reported rather than weakened. Engine expansion requires separate proposals. Distribution location, exact compatibility versions, and test mechanics require implementation investigation, not a new conversational subsystem.

## Problem

Creating meaningful cases consumes more human attention than writing CSV or TOML. Authors must reconstruct the task, decide what a correct outcome means, remove incidental context, supply fixtures, and establish that the grader distinguishes success from failure. Existing execution and scorecard infrastructure does not solve this authoring problem.

## Proposed outcome

An author provides a target skill to their chosen agentic tool and uses FastSkill's eval-authoring skill to design its evaluation tests. That agent inspects the target, proposes coverage and cases, surfaces uncertain assumptions, and invokes CLI validation and pilots as appropriate. Accepted cases enter an ordinary version-controlled suite with provenance. Human attention is spent on intent and ambiguous criteria rather than file syntax.

The initial scope is skill-to-suite authoring without requiring prior sessions or known failures. Session inspection for regression discovery and skill improvement is a separate future discussion. The skill's current instructions are evidence of intended behavior, not sufficient ground truth: a generated suite must not merely encode an existing mistake as the expected outcome.

## Proposed workflow

1. Select the skill to evaluate and inspect its instructions and relevant supporting files.
2. Propose a coverage plan spanning intended outcomes, triggering, non-triggering, boundaries, and failure handling where applicable. Identify missing intent before generating a large suite.
3. Propose expected outcomes and deterministic assertions where possible. Label inferred expectations and request clarification where correctness depends on unstated intent.
4. Show the proposed prompt, fixtures, assertions, evidence provenance, and unresolved assumptions together for review.
5. Pilot graders against acceptable and unacceptable example outcomes or deliberately constructed contrasts, labeled from author-confirmed criteria or independently verifiable facts. Do not treat a successful pilot as proof of validity.
6. Pilot execution on selected supported runtimes, reporting unavailable evidence separately from failure. Additional trials and ablation are a later expansion unless needed for the first use case.
7. Accept the candidate explicitly into the suite. Later executions and scorecards use the existing evaluation workflow.

All steps describe proposed behavior. They are not existing commands or implementation commitments.

## Initial scope recommendation

Start with skill inspection, intent clarification, coverage planning, candidate generation, suite-level review with targeted clarifications, grader calibration where supported, and conversion to the existing suite format. Report expectations that current capabilities cannot faithfully express; preserve unverified work explicitly.

Session import, session-derived regression discovery, and session-based skill improvement are deferred by the agreed scope. Also propose deferring automatic monitoring, unrestricted filesystem reconstruction, full MCP simulation, automatic skill rewriting, and autonomous promotion of generated cases until the initial authoring workflow is validated.

## Candidate ADR topics

Create numbered ADRs after the grilling session resolves the choices. Use the repository's context/decision/consequences structure and keep unresolved ADRs proposed.

| Topic | Recommended starting position | Decision to resolve |
| --- | --- | --- |
| Authoring entry point | Agreed: start from a skill and design its eval tests | Recorded in ADR-0011; session inspection deferred. |
| Authoring boundary | Agreed: user's agent plus authoring skill; CLI owns enforceable mechanics | ADR-0013; exact distribution and compatibility versions require investigation. |
| Candidate lifecycle | Ordinary files and version control; outline then suite-level review | Verification artifact details depend on the remaining engine-scope decision. |
| Case representation | Compile into existing CSV/check/judge formats, retain authoring metadata separately | Are richer fixtures or multi-turn cases essential enough to require schema changes now? |
| Ground truth and calibration | Agreed: author-confirmed criteria or independently verifiable facts establish labels | Preserve unverified work where prerequisites are missing; calibration implementation depends on engine scope. |
| Authoring inputs and data handling | User-selected agent owns the conversation and its provider setup | No new FastSkill authoring provider or conversational approval store. |
| Execution and ownership | Reusable calibration primitives upstream where appropriate; FastSkill owns CLI/project integration | Which parts belong in aikit-evals, and what isolation guarantees must pilots enforce? |

## Success measures

Measure human active minutes and clarification turns per accepted case, including subsequent repairs. Compare with manual FastSkill authoring and assisted plugin-eval authoring on comparable tasks. Do not optimize for the number of generated cases.

Track whether accepted cases distinguish known failures from acceptable outcomes, how often reviewers reject or rewrite suggested criteria, and how often graders change verdict on unchanged evidence. Record supported-runtime coverage and infrastructure errors separately. Numerical acceptance targets require a baseline and are not invented here.

## Questions to challenge in the grilling session

- Where should review checkpoints fall after the coverage outline is proposed?
- Can the case be reproduced without the original private repository or external service state?
- Who decides what a correct answer is when the skill's instructions are ambiguous or wrong?
- Is a cross-agent case still testing the same outcome after tool-specific details are removed?
- How do we prevent the generator from making a grader that simply rewards its own proposed answer?
- Can the workflow reduce total human work after fixture repair, review, and grader maintenance are counted?
- What happens when a proposed case cannot be expressed faithfully in the existing engine?
- How are sensitive source details removed without destroying the behavior being tested?

## Related material

- [Skill-first eval authoring ADR](../adr/0011-skill-first-eval-authoring.md).
- [Outcome correctness and adherence ADR](../adr/0012-separate-outcomes-from-adherence.md).
- [Skill-driven authoring ADR](../adr/0013-skill-driven-eval-authoring.md).
- [Command taxonomy ADR](../adr/0010-command-taxonomy.md): any new authoring actions must use the existing explicit namespace model.
- [Eval measurement integrity requirements](eval-measurement-integrity.md).
- [Eval judging requirements](eval-judge.md).
- [Eval scorecard reporting requirements](eval-scorecard-report.md).

# Start assisted eval authoring from the skill

Status: accepted. Date: 2026-09-12.

## Context

Designing meaningful eval cases requires human attention to intended outcomes, coverage, and grading criteria. The initial authoring experience could start from a skill that needs tests or from an observed session that should become a regression case. These solve different entry-point problems.

## Decision

The first assisted-authoring experience starts from an existing skill and helps its author design the skill's evaluation tests. It does not require session history or a known failure.

Inspecting sessions to discover regression cases or improve skills is deferred for a separate discussion. It is not a prerequisite or committed deliverable of the initial change.

This decision settles scope only. It does not select the authoring model, interaction style, suite representation, calibration method, acceptance workflow, or implementation ownership.

## Consequences

- Initial design and acceptance criteria must cover a skill with no prior usage sessions.
- Session import adapters and transcript reconstruction are outside the initial scope.
- The next decisions must establish how intended behavior is clarified and how generated cases and graders are validated.
- Future session-derived cases can be considered independently without delaying skill-first authoring.

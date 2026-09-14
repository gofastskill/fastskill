# Separate outcome correctness from skill adherence

Status: accepted. Date: 2026-09-12.

Generated evals must distinguish a useful, correct outcome from compliance with a skill's prescribed method. Measure these separately and make adherence mandatory only when the author identifies it as a requirement. Automatically treating every instruction as correctness would encode existing mistakes and could reward unnecessary tool use; treating outcomes alone as sufficient would miss genuinely required procedures.

This decision does not choose the storage schema or change the current engine's implicit trigger checks. Implementation must reconcile those existing checks with the authoring distinction before claiming support. Pilot success establishes evidence about cases and graders, not that the skill passes the suite.

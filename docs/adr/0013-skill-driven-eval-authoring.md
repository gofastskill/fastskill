# Drive eval authoring through a skill and the CLI

Status: accepted. Date: 2026-09-12.

Users design evals in their chosen agentic tool with FastSkill CLI and an eval-authoring skill that explains its layouts and standards. The authoring skill guides inspection, coverage discussion, case construction, and iteration; FastSkill and its eval engine define and validate artifacts, execute evaluations, and preserve measurement evidence. This keeps conversational judgment in the user's agent and enforceable mechanics in code, avoiding a separate model integration and interview orchestrator inside FastSkill.

Drafts use ordinary files and version control. A dedicated interview engine and draft-state service are deferred. Requirements that CI relies on must be represented in artifacts and checked by code; instructions in the authoring skill alone cannot establish verified status.

Authoring-tool choice is independent of evaluation-runtime support. An agent able to follow the skill, edit files, and invoke the CLI can author a suite without having a FastSkill evaluation backend. Executing that agent as the subject of an eval still requires a supported backend and observable evidence.

The authoring agent owns conversational authorization: it uses the user's existing authorization or asks about pilot scope before invoking the CLI. FastSkill validates explicit parameters and returns results or errors; it does not infer consent, remember conversational approvals, or conduct an interview. A spending ceiling is not a prerequisite of this change and must not be promised unless implemented.

The skill checks CLI compatibility and follows authoritative format references and validation output. Exact supported versions and distribution location remain implementation details to investigate. Review covers the outline and then the suite, with individual questions for ambiguities. Calibration labels follow author-confirmed criteria or independently verifiable facts.

The first delivery is the authoring skill with supporting documentation and validation corrections for existing contracts. Unsupported test intentions are reported explicitly; new grader types, schemas, or execution capabilities require separate proposals. The PRDs are drafts for external review, not a claim of implementation readiness or completed verification.

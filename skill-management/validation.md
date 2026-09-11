# Validate a skill

FastSkill 0.9.229

Source: https://docs.gofastskill.com/skill-management/validation

Release revision: 68975dfaca15508bc5cf2fb761d6218405c954ea

Documentation revision: 68975dfaca15508bc5cf2fb761d6218405c954ea



## Start with SKILL.md

A skill root contains `SKILL.md` with YAML frontmatter and instructions:

```markdown
---
name: review-notes
description: Review meeting notes and extract decisions and action items.
metadata:
  version: "1.0.0"
---

List decisions, owners, and next actions. Mark missing details as unspecified.
```

The standard validator expects a lowercase name of 1–64 characters using letters,
digits, and single separating hyphens. The description must be nonempty and at most
1024 characters. Name and description help an agent decide when to load the skill.

FastSkill derives identity from the skill metadata/name and uses a semantic version
when resolving or packaging skills. Version metadata is optional for a minimal skill;
FastSkill defaults an unspecified version to `1.0.0`. Set `metadata.version`
explicitly for a skill you distribute.

## Validate installation and state

After creating the source above, add it from a configured project:

```bash
fastskill skill add ./source/review-notes --no-reindex
fastskill skill list --check
fastskill skill read review-notes
```

Installation validates metadata and source contents. Reconciliation checks desired
constraints, locked facts, dependency ownership, and installed content. Editable
local sources are reported as mutable rather than digest-verified.

A successful structural check does not prove that instructions are safe or useful.
Review scripts and resources, then use [evals](/evals-quality/overview) to measure
behavior on representative tasks. See [reconciliation](/skill-management/reconciliation)
for missing, changed, conflicting, and unverifiable installations.


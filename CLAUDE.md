# CLAUDE.md

FastSkill is a package manager and operational toolkit for AI agent skills, providing install/lock/discover/eval/serve workflows around the Claude Code `SKILL.md` layout.

## Pull request automation

When opening a pull request in this repository, arm auto-merge on it immediately:

```bash
gh pr merge --auto --squash --delete-branch
```

This repository has required status checks, so auto-merge holds the PR until they pass — it
never merges early. Arming it is what lets a PR land when green without a manual step.

### Merge safety — no exceptions

- **NEVER** use `--admin`, `--force`, or any bypass/override flag to merge a pull request.
  If a required check is red, or a review is required, **leave the PR open** and report it.
  This applies even when the failure looks unrelated to your change.
- Do not merge a PR with failing checks. A pre-existing break on `main` is a reason to stop
  and report, not a reason to push through.
- Do not change repository visibility (private ↔ public).
- Do feature work on a branch off the latest `origin/main`, never directly on `main`.

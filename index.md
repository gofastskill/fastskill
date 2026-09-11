# FastSkill

FastSkill 0.9.230

Source: https://docs.gofastskill.com/

Release revision: dd983e89e5fee977325b77ae386b879e8310ed26

Documentation revision: dd983e89e5fee977325b77ae386b879e8310ed26



Install and manage agent skills. [Start with a local skill](/quickstart).


## What is FastSkill?

**Package manager and operational toolkit for Agent AI Skills.** FastSkill enables discovery, installation, versioning, and deployment of skills at scale. It implements the same `SKILL.md` conventions used by Claude Code–compatible agents, and adds **manifests**, **lockfiles**, **self-contained team bundles**, **validation*&#x2A;, optional &#x2A;*`fastskill eval`** runs, and **search**. Modern agents read installed skills directly from the skills directory — no metadata-file sync step.

The project is designed around a **CLI-first workflow*&#x2A;; local &#x2A;*`fastskill server serve`** exposes an HTTP API and web UI for browsing and integration.

## What you can do

**Package & lifecycle**

`skill add`, `project install`, `skill update`, `skill remove`, `skill list`, `skill read` with `skill-project.toml` and `skills.lock`.


**Validation**

Structure checks on install and reconciliation across manifest, lock, and disk. See [Skill validation](/skill-management/validation).


**Team bundles**

Build one verified, versioned ZIP containing a complete skill setup and dependency closure. See [bundle command](/cli-reference/bundle-command).


**Evals**

`fastskill eval validate | run | judge | report | score | scorecard` for defined quality suites. See [eval command](/cli-reference/eval-command).


**Discovery & search**

Catalog search by default; `--local` for installed skills, embeddings, and keyword fallback.


**Diagnostics**

`fastskill cli doctor` reports configuration and environment readiness, including whether semantic search is available. See [tooling commands](/cli-reference/tooling-commands).


**Catalogs & sources**

`repo` for remote origins and catalog browsing. See [Registry overview](/registry/overview).



## Start with one local skill

The [quickstart](/quickstart) creates a skill, installs it, verifies its contents,
and makes it available to an agent. It needs no registry or API key.

## How it fits together

* **You**: edit `SKILL.md` and manifests; agents **read** skill content. FastSkill does not execute skill logic as a runtime sandbox.
* **CLI**: single entry point for installs, checks, search, and evals.
* **Optional `server serve`**: HTTP API and UI on your machine for browsing and integrations.

![Declare, lock, and ship workflow in FastSkill](/images/hero-diagram.svg)

## Use cases (summary)

* **Authors**: init, validate, eval, and ship skills via the platform.
* **Developers**: install, list, read, search for what agents load.
* **Teams**: shared manifest + lock, groups for optional stacks, and self-contained bundle releases.
* **Automation**: `project install --lock` and reproducible installs in CI.

## Next steps

### Install

[Installation](/installation) for the `fastskill` binary.


### Learn the model

[Welcome](/welcome) and [Manifest system](/skill-management/manifest-system).


### Operate skills

[Skill validation](/skill-management/validation), [Evals and quality](/evals-quality/overview), [CLI reference](/cli-reference/overview).


### Integrate

[Cursor integration](/integration/cursor-integration) or [Claude Code integration](/integration/claude-code-integration).



## Getting help

* [Documentation home](/welcome)
* [GitHub Issues](https://github.com/gofastskill/fastskill/issues)

Continue with [Quick start](/quickstart) or the [CLI reference](/cli-reference/overview).


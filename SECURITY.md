# Security policy

## Scope of security vulnerabilities

fastskill is a skill package manager. Due to the design of the AI agent skill ecosystem and the dynamic
nature of skills themselves, there are many cases where fastskill can execute arbitrary code. For example:

- fastskill loads and executes skills that may contain arbitrary code and scripts
- skills may execute code when loaded or invoked by AI agents
- skills are installed from multiple sources (Git, local, ZIP, registries)
- skills may include tool integrations and external API calls

These are not considered vulnerabilities in fastskill. If you think fastskill's stance in these areas can be
hardened, please file an issue for a new feature.

## Reporting a vulnerability

If you have found a possible vulnerability that is not excluded by the above
[scope](#scope-of-security-vulnerabilities), please report it through our GitHub security advisory system.

## Bug bounties

While reports of suspected security problems are sincerely appreciated and encouraged, please note that
gofastskill does not currently run any bug bounty programs.

## Vulnerability disclosures

Critical vulnerabilities will be disclosed via GitHub's
[security advisory](https://github.com/gofastskill/fastskill/security) system.

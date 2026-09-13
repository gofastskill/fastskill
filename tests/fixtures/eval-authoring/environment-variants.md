# Controlled prerequisite variants

Use these deterministic branches in workflow exercises:

- incompatible CLI: stub `fastskill --version` as `0.8.0`; retain files and mark compatibility unverified;
- missing runtime: select an unavailable runtime and retain the parse-valid suite;
- missing judge credential: unset the declared `api_key_env`; do not treat authoring-agent authentication as judge authentication;
- invalid reference: point `checks` at malformed TOML, capture validation failure, repair it, and rerun validation;
- unavailable observation: select a backend that preflight identifies as unable to observe a required check; report exclusion/error rather than pass.

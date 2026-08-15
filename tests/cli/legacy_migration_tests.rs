//! Formerly: integration tests for legacy command deprecation and
//! forwarding behavior (`sources`/`registry` -> `repos`).
//!
//! All five tests here asserted that the retired `sources`/`registry`
//! top-level commands still worked and emitted a deprecation warning
//! forwarding to `repos` (issue-#183 "cli-command-surface-redesign").
//! That forwarding shim is gone: `fastskill sources ...` and `fastskill
//! registry ...` now fail outright with "unknown argument" / "unrecognized
//! subcommand" (verified directly against `./target/debug/fastskill`), with
//! no deprecation warning and no forwarding — the migration period is over
//! and there is nothing left to forward. There is no `repos`-side behavior
//! to port: the `repos` command surface itself is already covered by
//! `tests/cli/repos_integration_tests.rs`, and the two tests that only
//! inspected top-level `--help` output
//! (`test_legacy_commands_hidden_from_top_level_help`,
//! `test_registry_remove_is_rejected_by_parser`) were asserting on the
//! now-defunct `sources`/`registry` commands specifically, not on `repos`,
//! so they had no meaningful `repos` equivalent either. All tests were
//! deleted; this file intentionally contains no `#[test]` functions.

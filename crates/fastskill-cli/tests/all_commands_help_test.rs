#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Regression test for a class of clap `debug_assert` panics: a command's own
//! `ArgSpec` (id, `--long`, or short flag) colliding with a clap
//! auto-generated flag (`--version`/`-V` propagated to every subcommand from
//! the root, or `--help`/`-h`). Such a collision panics in debug builds the
//! *instant* the offending command is built -- e.g. `fastskill update --help`
//! -- not just when the colliding flag is actually used.
//!
//! Concrete incident: `crates/fastskill-cli/src/commands/update.rs` declared
//! an `ArgSpec { name: "version", long: Some("version"), .. }`, which
//! collided with clap's auto-generated `--version`/`-V` (propagated to every
//! subcommand via `Command::propagate_version(true)` on the root in
//! `cli-framework`'s `build_clap_root`). This panicked on *every* invocation
//! of `fastskill update`, including plain `--help`. An audit of the full
//! command surface (driven by `fastskill spec --format json`, the same
//! source of truth used by `spec_docs_parity_test.rs`) found the identical
//! defect in `init` and `marketplace create` as well.
//!
//! This test walks every command path in the live command tree -- every leaf
//! command *and* every intermediate command group -- and asserts that
//! `--help` exits successfully with no panic. It is deliberately blunt (no
//! per-flag introspection): any clap `debug_assert` panic in any command's
//! spec (duplicate arg names, conflicting short flags, colliding long flags,
//! bad group refs, ...) trips the same failure mode -- a panic the instant
//! that command's clap `Command` gets built -- so a bare `--help` sweep
//! catches the whole class, not just this one incident.
//!
//! `fastskill-cli` has no `[lib]` target (bin-only), so this shells out to
//! the compiled test binary via `CARGO_BIN_EXE_fastskill`, same pattern as
//! `mcp_stdio_protocol_test.rs` and `spec_docs_parity_test.rs` in this
//! directory.

use std::collections::BTreeSet;
use std::process::Command;

/// Command path separator used by `fastskill spec`'s JSON output (`"repos/add"`).
const SPEC_PATH_SEP: char = '/';

/// Leaf command paths from the live command tree, as `Vec<&str>` segments
/// (e.g. `["repos", "add"]`, `["update"]`).
fn leaf_command_paths() -> Vec<Vec<String>> {
    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(["spec", "--format", "json"])
        .output()
        .expect("spawn `fastskill spec --format json`");

    assert!(
        output.status.success(),
        "`fastskill spec --format json` exited with {}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let doc: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("parse `fastskill spec --format json` stdout as JSON");

    doc["commands"]
        .as_array()
        .expect("`commands` array in `fastskill spec` JSON output")
        .iter()
        .map(|c| {
            let path = c["path"]
                .as_str()
                .expect("command `path` string in `fastskill spec` JSON output");
            path.split(SPEC_PATH_SEP)
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every command path that should have a working `--help`: every leaf
/// command, plus every intermediate group prefix (e.g. `["repos", "add"]`
/// implies the group `["repos"]` also needs checking -- `fastskill repos
/// --help` builds and prints the same clap `Command` tree as any leaf under
/// it).
fn all_paths_including_groups(leaves: &[Vec<String>]) -> BTreeSet<Vec<String>> {
    let mut all: BTreeSet<Vec<String>> = BTreeSet::new();
    for leaf in leaves {
        all.insert(leaf.clone());
        for len in 1..leaf.len() {
            all.insert(leaf[..len].to_vec());
        }
    }
    all
}

/// Run `fastskill <segments> --help` and assert it exits successfully with
/// no panic on stderr. A `debug_assert` panic in clap's `Command::build`
/// (triggered lazily, the first time the command tree is parsed) is a hard
/// process abort with a nonzero exit code and a `thread 'main' panicked at`
/// line on stderr -- this is what would have caught the `update`/`init`/
/// `marketplace create` `--version` collisions before they shipped.
fn assert_help_succeeds(segments: &[String]) {
    let mut args: Vec<&str> = segments.iter().map(|s| s.as_str()).collect();
    args.push("--help");

    let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
        .args(&args)
        .output()
        .unwrap_or_else(|e| panic!("spawn `fastskill {}`: {e}", args.join(" ")));

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stderr.contains("panicked at"),
        "`fastskill {}` panicked:\n--- stderr ---\n{}\n--- stdout ---\n{}",
        args.join(" "),
        stderr,
        stdout
    );
    assert!(
        output.status.success(),
        "`fastskill {}` exited with {} (expected success)\n--- stderr ---\n{}\n--- stdout ---\n{}",
        args.join(" "),
        output.status,
        stderr,
        stdout
    );
}

#[test]
fn all_commands_help_succeeds() {
    let leaves = leaf_command_paths();
    assert!(
        !leaves.is_empty(),
        "extracted zero command paths from `fastskill spec --format json` -- \
         extraction logic is likely broken"
    );

    let all_paths = all_paths_including_groups(&leaves);
    assert!(
        all_paths.len() >= leaves.len(),
        "expected at least as many paths (leaves + groups) as leaves alone"
    );

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for path in &all_paths {
        checked += 1;
        let result = std::panic::catch_unwind(|| assert_help_succeeds(path));
        if let Err(payload) = result {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic payload>".to_string());
            failures.push(format!("fastskill {}:\n{msg}", path.join(" ")));
        }
    }

    assert!(
        checked > 0,
        "checked zero command paths -- test is not exercising anything"
    );

    if !failures.is_empty() {
        panic!(
            "\n{}/{} command path(s) failed `--help`:\n\n{}\n",
            failures.len(),
            checked,
            failures.join("\n\n")
        );
    }
}

/// Root-level invocations: `fastskill --help`, `-h`, `--version`, `-V`, and
/// no-args-at-all. These go through the same clap `Command::build()` path as
/// every subcommand and are cheap to cover here too.
#[test]
fn root_help_and_version_succeed() {
    for args in [vec!["--help"], vec!["-h"], vec!["--version"], vec!["-V"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_fastskill"))
            .args(&args)
            .output()
            .unwrap_or_else(|e| panic!("spawn `fastskill {}`: {e}", args.join(" ")));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("panicked at"),
            "`fastskill {}` panicked:\n{}",
            args.join(" "),
            stderr
        );
        assert!(
            output.status.success(),
            "`fastskill {}` exited with {}\nstderr:\n{}",
            args.join(" "),
            output.status,
            stderr
        );
    }
}

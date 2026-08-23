//! Helper utilities for snapshot testing CLI output with insta.
//!
//! This module provides common patterns for testing CLI commands with snapshots,
//! including normalization of dynamic content like paths, timestamps, and version numbers.

use std::process::Command;

/// Result of running a CLI command
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Normalized snapshot settings for redactions
pub struct SnapshotSettings {
    pub normalize_paths: bool,
    pub normalize_versions: bool,
    pub normalize_timestamps: bool,
}

/// Get the path to the fastskill binary for testing.
///
/// Uses `CARGO_BIN_EXE_fastskill`, which Cargo sets at compile time for
/// integration tests to the exact path of the `fastskill` bin target as
/// built for the active profile (debug/release/llvm-cov-target/...). This
/// crate's tests now live under `crates/fastskill-cli` while `target/` is
/// shared at the workspace root, so hand-rolling `{manifest_dir}/target/...`
/// silently resolves to a path that never exists (silently falling back to
/// `cargo run`, which is slow and pollutes captured stdout/stderr with build
/// output). `CARGO_BIN_EXE_*` sidesteps that entirely.
pub fn get_binary_path() -> String {
    env!("CARGO_BIN_EXE_fastskill").to_string()
}

/// Run a fastskill command and return the result
pub fn run_fastskill_command(
    args: &[&str],
    working_dir: Option<&std::path::Path>,
) -> CommandResult {
    let binary = get_binary_path();
    let mut cmd = if binary == "cargo" {
        let mut cmd = Command::new("cargo");
        cmd.args(&["run", "--bin", "fastskill", "--"]).args(args);
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }
        cmd
    } else {
        let mut cmd = Command::new(&binary);
        cmd.args(args);
        if let Some(dir) = working_dir {
            #[cfg(test)]
            eprintln!("DEBUG: Setting working directory to: {:?}", dir);
            cmd.current_dir(dir);
        }
        #[cfg(test)]
        eprintln!("DEBUG: Executing command: {} with args: {:?}", binary, args);
        cmd
    };

    let output = cmd.output().expect("Failed to execute command");

    #[cfg(test)]
    eprintln!(
        "DEBUG: Command output - success: {}, stdout: {}, stderr: {}",
        output.status.success(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    CommandResult {
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
        success: output.status.success(),
    }
}

/// Run a fastskill command with custom environment variables
pub fn run_fastskill_command_with_env(
    args: &[&str],
    env_vars: &[(&str, &str)],
    working_dir: Option<&std::path::Path>,
) -> CommandResult {
    let binary = get_binary_path();
    let mut cmd = if binary == "cargo" {
        let mut cmd = Command::new("cargo");
        cmd.args(&["run", "--bin", "fastskill", "--"]).args(args);
        cmd
    } else {
        let mut cmd = Command::new(&binary);
        cmd.args(args);
        cmd
    };

    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    let output = cmd.output().expect("Failed to execute command");

    #[cfg(test)]
    eprintln!(
        "DEBUG: Command output - success: {}, stdout: {}, stderr: {}",
        output.status.success(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    CommandResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        success: output.status.success(),
    }
}

/// Normalize output for snapshot testing by removing dynamic content
pub fn normalize_snapshot_output(output: &str, settings: &SnapshotSettings) -> String {
    let mut result = output.to_string();

    // Strip ANSI escape sequences to keep snapshots stable across environments
    // where colorized logging may be enabled (e.g. CI terminals).
    result = regex::Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
        .unwrap()
        .replace_all(&result, "")
        .to_string();

    // Strip structured log lines emitted by tracing subscribers so snapshots focus
    // on user-facing CLI output and are stable across environment log settings.
    result = regex::Regex::new(
        r"(?m)^(?:\[TIMESTAMP\]|\d{4}-\d{2}-\d{2}T[^\s]*)\s+(?:TRACE|DEBUG|INFO|WARN|ERROR)\s+[^\n]*\n?",
    )
    .unwrap()
    .replace_all(&result, "")
    .to_string();

    if settings.normalize_versions {
        // Normalize version numbers (semantic versioning)
        result = regex::Regex::new(r"\d+\.\d+\.\d+(-[a-zA-Z0-9.-]+)?")
            .unwrap()
            .replace_all(&result, "[VERSION]")
            .to_string();
    }

    if settings.normalize_paths {
        // Strip the `\\?\` extended-length-path prefix that
        // `Path::canonicalize()` emits on Windows (e.g. `\\?\C:\Users\...`).
        // It's a plain literal, not a regex, so a plain string replace is
        // enough -- and it must run first, or the prefix survives into the
        // snapshot and every pattern below (which all assume a normal-looking
        // drive path) fails to match.
        result = result.replace(r"\\?\", "");

        // Normalize the path of the binary under test. `argv[0]` shows up in
        // usage lines, and its absolute path depends on where the repo happens
        // to be checked out -- `/home/me/src/fastskill/...` locally versus
        // `/home/runner/work/fastskill/...` on CI, or
        // `C:\Users\runneradmin\...\target\debug\fastskill.exe` on Windows.
        // Masking only the home directory leaves the differing remainder, so
        // match the whole path.
        //
        // Anchor on the profile directory rather than `/target/`: under
        // `cargo llvm-cov` the binary is built into `target/llvm-cov-target/
        // debug/`, so requiring a literal `/target/debug/` would miss the
        // coverage job and reintroduce an environment-dependent snapshot.
        //
        // `[/\\]` accepts either path separator and the trailing `(?:\.exe)?`
        // accepts the Windows executable suffix, so the SAME placeholder
        // covers both platforms with one pattern. This must run before the
        // temp/home patterns below: a Windows binary path also contains
        // `Users\<name>`, and the (less specific) home-dir pattern would
        // otherwise chew up just that prefix, leaving
        // `\target\debug\fastskill.exe` behind unmatched.
        result = regex::Regex::new(r"\S*[/\\](?:debug|release)[/\\]fastskill(?:\.exe)?\b")
            .unwrap()
            .replace_all(&result, "[FASTSKILL_BIN]")
            .to_string();

        // Normalize clap's program name in `Usage:` / help output. clap derives
        // it from `argv[0]`'s file stem, which is a bare `fastskill.exe` on
        // Windows but `fastskill` on Linux, so every `--help` snapshot's usage
        // line differs by that suffix. This is the *bare* command name, not a
        // path (path-carrying occurrences were already folded into
        // `[FASTSKILL_BIN]` just above), so strip the `.exe` to match Linux.
        // No-op on Linux, where `fastskill.exe` never appears.
        result = regex::Regex::new(r"\bfastskill\.exe\b")
            .unwrap()
            .replace_all(&result, "fastskill")
            .to_string();

        // Normalize temporary directory paths (Unix).
        result = regex::Regex::new(r"/tmp/[^\s]+")
            .unwrap()
            .replace_all(&result, "[TEMP_DIR]")
            .to_string();

        // Normalize temporary directory paths (Windows), e.g.
        // `C:\Users\RUNNER~1\AppData\Local\Temp\...`. Must run before the
        // home-dir pattern below: a Windows temp path also contains
        // `Users\<name>`, and the home pattern would otherwise mask only
        // that prefix and leave `\AppData\Local\Temp\...` -- a different,
        // still environment-specific suffix -- behind.
        result = regex::Regex::new(r"(?i)[A-Za-z]:\\(?:[^\\\s]+\\)*Temp\\[^\s]+")
            .unwrap()
            .replace_all(&result, "[TEMP_DIR]")
            .to_string();

        // Normalize user home directory (Unix).
        result = regex::Regex::new(r"/home/[^\s/]+")
            .unwrap()
            .replace_all(&result, "[HOME_DIR]")
            .to_string();

        // Normalize user home directory (Windows), e.g. `C:\Users\alice`.
        // Deliberately narrower than a bare `C:\\[^\s]+` catch-all (the old
        // `[WINDOWS_PATH]` placeholder that used to live here): that
        // shadowed the more specific binary/temp placeholders above and
        // doesn't appear in any checked-in snapshot, so it was pure noise.
        result = regex::Regex::new(r"[A-Za-z]:\\Users\\[^\\\s]+")
            .unwrap()
            .replace_all(&result, "[HOME_DIR]")
            .to_string();

        // Normalize port numbers in URLs (after version normalization)
        result = regex::Regex::new(r"http://\[VERSION\]\.\d+:\d{4,5}")
            .unwrap()
            .replace_all(&result, "http://[VERSION].1:[PORT]")
            .to_string();
    }

    // Normalize network-dependent git and socket errors to keep snapshots stable
    // across environments with different DNS/network policies.
    result = regex::Regex::new(
        r"(?m)^(?:\[TIMESTAMP\]|\d{4}-\d{2}-\d{2}T[^\s]*)\s+WARN fastskill_core::storage::git: Git operation failed with network error.*\n?",
    )
    .unwrap()
    .replace_all(&result, "")
    .to_string();

    result = regex::Regex::new(r"fatal: unable to access '[^']+': [^\n]+")
        .unwrap()
        .replace_all(&result, "[GIT_NETWORK_ERROR]")
        .to_string();

    result =
        regex::Regex::new(r"remote: Repository not found\.\nfatal: repository '[^']+' not found")
            .unwrap()
            .replace_all(&result, "[GIT_NETWORK_ERROR]")
            .to_string();

    result = regex::Regex::new(r"tcp (?:connect|open) error: [^\n]+")
        .unwrap()
        .replace_all(&result, "tcp connect error: [NETWORK_ERROR]")
        .to_string();

    // reqwest 0.12 may stop error chains at "error sending request for url (...)"
    // without including the lower-level socket error details.
    result = regex::Regex::new(r"error sending request for url \(([^)]+)\)(?:\n|$)")
        .unwrap()
        .replace_all(
            &result,
            "error sending request for url ($1): error trying to connect: tcp connect error: [NETWORK_ERROR]\n",
        )
        .to_string();

    // reqwest URL parser errors may now return only "builder error" without detail.
    result = regex::Regex::new(r"builder error(?:\: invalid port number)?")
        .unwrap()
        .replace_all(&result, "builder error: invalid port number")
        .to_string();

    // Ignore standalone network placeholders when transient git failures are logged on stdout.
    result = regex::Regex::new(r"(?m)^\[GIT_NETWORK_ERROR\]\n?")
        .unwrap()
        .replace_all(&result, "")
        .to_string();

    result = regex::Regex::new(r"\n\n  (Installing|Updating)")
        .unwrap()
        .replace_all(&result, "\n  $1")
        .to_string();

    result = regex::Regex::new(r"(?m)^(  (?:Installing|Updating) [^\n]*\n)\n")
        .unwrap()
        .replace_all(&result, "$1")
        .to_string();

    // Collapse extra blank lines created by normalization.
    result = regex::Regex::new(r"\n{3,}")
        .unwrap()
        .replace_all(&result, "\n\n")
        .to_string();

    if settings.normalize_timestamps {
        // Normalize ISO 8601 timestamps
        result =
            regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?")
                .unwrap()
                .replace_all(&result, "[TIMESTAMP]")
                .to_string();

        // Normalize Unix timestamps
        result = regex::Regex::new(r"\d{10,}")
            .unwrap()
            .replace_all(&result, "[UNIX_TIMESTAMP]")
            .to_string();
    }

    result
}

/// Helper to assert a snapshot with normalization
pub fn assert_snapshot_with_settings(name: &str, content: &str, settings: &SnapshotSettings) {
    let normalized = normalize_snapshot_output(content, settings);
    insta::assert_snapshot!(name, normalized);
}

/// Standard settings for CLI output snapshots
pub fn cli_snapshot_settings() -> SnapshotSettings {
    SnapshotSettings {
        normalize_paths: true,
        normalize_versions: true,
        normalize_timestamps: true,
    }
}

/// Settings for help command snapshots (usually don't need path normalization)
#[allow(dead_code)]
pub fn help_snapshot_settings() -> SnapshotSettings {
    SnapshotSettings {
        normalize_paths: false,
        normalize_versions: true,
        normalize_timestamps: false,
    }
}

/// Settings for error output snapshots
#[allow(dead_code)]
pub fn error_snapshot_settings() -> SnapshotSettings {
    SnapshotSettings {
        normalize_paths: true,
        normalize_versions: false,
        normalize_timestamps: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_paths() {
        let settings = SnapshotSettings {
            normalize_paths: true,
            normalize_versions: false,
            normalize_timestamps: false,
        };

        // The binary path collapses to a single placeholder: the checkout
        // location differs between a developer machine and a CI runner, so
        // keeping any of it would make snapshots environment-specific.
        let input = "/home/user/fastskill/target/debug/fastskill --help";
        let expected = "[FASTSKILL_BIN] --help";
        assert_eq!(normalize_snapshot_output(input, &settings), expected);

        // Same path under a CI-style checkout must normalize identically.
        let ci_input = "/home/runner/work/fastskill/fastskill/target/debug/fastskill --help";
        assert_eq!(normalize_snapshot_output(ci_input, &settings), expected);

        // `cargo llvm-cov` builds into its own target dir; the coverage job
        // must produce the same snapshot as the plain test job.
        let cov_input = "/home/runner/work/fastskill/target/llvm-cov-target/debug/fastskill --help";
        assert_eq!(normalize_snapshot_output(cov_input, &settings), expected);

        // A release-profile build normalizes the same way.
        let rel_input = "/home/user/fastskill/target/release/fastskill --help";
        assert_eq!(normalize_snapshot_output(rel_input, &settings), expected);

        // Home directories unrelated to the binary are still masked.
        let home_input = "/home/user/skills/my-skill";
        assert_eq!(
            normalize_snapshot_output(home_input, &settings),
            "[HOME_DIR]/skills/my-skill"
        );
    }

    #[test]
    fn test_normalize_windows_paths() {
        // Windows inputs must fold to the SAME canonical placeholders as
        // their Linux counterparts above, so one set of `.snap` files can
        // serve both platforms without a Windows-only snapshot fork.
        let settings = SnapshotSettings {
            normalize_paths: true,
            normalize_versions: false,
            normalize_timestamps: false,
        };

        // Ordinary Windows binary path (backslashes, `.exe` suffix).
        let input = r"Usage: C:\Users\RUNNER~1\work\fastskill\fastskill\target\debug\fastskill.exe <command>";
        let expected = "Usage: [FASTSKILL_BIN] <command>";
        assert_eq!(normalize_snapshot_output(input, &settings), expected);

        // Release-profile build normalizes the same way.
        let release_input = r"Usage: C:\Users\RUNNER~1\work\fastskill\fastskill\target\release\fastskill.exe <command>";
        assert_eq!(
            normalize_snapshot_output(release_input, &settings),
            expected
        );

        // `Path::canonicalize()` on Windows returns the `\\?\` extended-length
        // prefix -- it must be stripped before path matching, not leak into
        // the snapshot as a literal `\\?\`.
        let extended_input =
            r"\\?\C:\Users\RUNNER~1\work\fastskill\fastskill\target\release\fastskill.exe";
        assert_eq!(
            normalize_snapshot_output(extended_input, &settings),
            "[FASTSKILL_BIN]"
        );

        // Windows temp dir, including the 8.3 short name (`RUNNER~1`) GitHub
        // Actions runners use, must collapse to the same [TEMP_DIR]
        // placeholder as `/tmp/...` does on Linux.
        let temp_input =
            r"Installing skill to C:\Users\RUNNER~1\AppData\Local\Temp\skl_abc123\skill";
        assert_eq!(
            normalize_snapshot_output(temp_input, &settings),
            "Installing skill to [TEMP_DIR]"
        );

        // Windows home dir must collapse to the same [HOME_DIR] placeholder
        // as `/home/<user>` does on Linux, leaving the remainder of the path
        // intact (mirrors the Linux `[HOME_DIR]/skills/my-skill` case).
        let home_input = r"Reading config from C:\Users\alice\skills\my-skill";
        assert_eq!(
            normalize_snapshot_output(home_input, &settings),
            r"Reading config from [HOME_DIR]\skills\my-skill"
        );

        // A Windows path that is neither the binary, a temp dir, nor a home
        // dir (no `Users\` segment) is left alone -- proving the old
        // `[WINDOWS_PATH]` catch-all (`C:\\[^\s]+`) is gone and nothing else
        // over-matches in its place.
        let unrelated_input = r"See D:\a\fastskill\fastskill\README.md for details";
        assert_eq!(
            normalize_snapshot_output(unrelated_input, &settings),
            unrelated_input
        );

        // clap's bare program name in a usage line carries `.exe` on Windows
        // (`Usage: fastskill.exe add ...`); it must fold to `fastskill` so the
        // help snapshots match Linux. This is the bare name, not a path -- a
        // pathful `...\debug\fastskill.exe` still becomes [FASTSKILL_BIN].
        let usage_input = "Usage: fastskill.exe eval report [OPTIONS] --run-dir <run-dir>";
        assert_eq!(
            normalize_snapshot_output(usage_input, &settings),
            "Usage: fastskill eval report [OPTIONS] --run-dir <run-dir>"
        );
        // The pathful form is still masked as the binary placeholder, not just
        // stripped of its suffix.
        let pathful = r"C:\Users\RUNNER~1\work\fastskill\fastskill\target\debug\fastskill.exe";
        assert_eq!(
            normalize_snapshot_output(pathful, &settings),
            "[FASTSKILL_BIN]"
        );

        // CRLF line endings around a Windows path don't get swallowed into
        // the match: `\S`/path-segment classes stop at `\r` since it's
        // whitespace, so captured Windows output with `\r\n` line endings
        // normalizes the same as `\n`-only output.
        let crlf_input =
            "Usage: C:\\Users\\RUNNER~1\\work\\fastskill\\fastskill\\target\\debug\\fastskill.exe\r\n<command>\r\n";
        assert_eq!(
            normalize_snapshot_output(crlf_input, &settings),
            "Usage: [FASTSKILL_BIN]\r\n<command>\r\n"
        );
    }

    #[test]
    fn test_normalize_versions() {
        let settings = SnapshotSettings {
            normalize_paths: false,
            normalize_versions: true,
            normalize_timestamps: false,
        };

        let input = "fastskill 1.2.3-beta.1";
        let expected = "fastskill [VERSION]";
        assert_eq!(normalize_snapshot_output(input, &settings), expected);
    }

    #[test]
    fn test_normalize_timestamps() {
        let settings = SnapshotSettings {
            normalize_paths: false,
            normalize_versions: false,
            normalize_timestamps: true,
        };

        let input = "Created at 2023-12-01T10:30:45Z";
        let expected = "Created at [TIMESTAMP]";
        assert_eq!(normalize_snapshot_output(input, &settings), expected);
    }
}

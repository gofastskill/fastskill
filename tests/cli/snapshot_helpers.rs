//! Helper utilities for snapshot testing CLI output with insta.
//!
//! This module provides common patterns for testing CLI commands with snapshots,
//! including normalization of dynamic content like paths, timestamps, and version numbers.

use std::path::Path;
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

/// Get the path to the fastskill binary for testing
pub fn get_binary_path() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let debug_path = format!("{}/target/debug/fastskill", manifest_dir);
    let release_path = format!("{}/target/release/fastskill", manifest_dir);

    if Path::new(&debug_path).exists() {
        debug_path
    } else if Path::new(&release_path).exists() {
        release_path
    } else {
        // Fallback to cargo run
        "cargo".to_string()
    }
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
            cmd.current_dir(dir);
        }
        cmd
    };

    let output = cmd.output().expect("Failed to execute command");

    CommandResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        success: output.status.success(),
    }
}

/// Run a fastskill command with custom environment variables
pub fn run_fastskill_command_with_env(args: &[&str], env_vars: &[(&str, &str)]) -> CommandResult {
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

    let output = cmd.output().expect("Failed to execute command");

    CommandResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        success: output.status.success(),
    }
}

/// Normalize output for snapshot testing by removing dynamic content
pub fn normalize_snapshot_output(output: &str, settings: &SnapshotSettings) -> String {
    let mut result = output.to_string();

    if settings.normalize_paths {
        // Normalize user home directory
        result = regex::Regex::new(r"/home/[^\s/]+")
            .unwrap()
            .replace_all(&result, "[HOME_DIR]")
            .to_string();

        // Normalize temporary directory paths
        result = regex::Regex::new(r"/tmp/[^\s]+")
            .unwrap()
            .replace_all(&result, "[TEMP_DIR]")
            .to_string();

        // Normalize Windows paths if any
        result = regex::Regex::new(r"C:\\[^\s]+")
            .unwrap()
            .replace_all(&result, "[WINDOWS_PATH]")
            .to_string();
    }

    if settings.normalize_versions {
        // Normalize version numbers (semantic versioning)
        result = regex::Regex::new(r"\d+\.\d+\.\d+(-[a-zA-Z0-9.-]+)?")
            .unwrap()
            .replace_all(&result, "[VERSION]")
            .to_string();
    }

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
pub fn help_snapshot_settings() -> SnapshotSettings {
    SnapshotSettings {
        normalize_paths: false,
        normalize_versions: true,
        normalize_timestamps: false,
    }
}

/// Settings for error output snapshots
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

        let input = "/home/user/fastskill/target/debug/fastskill --help";
        let expected = "[HOME_DIR]/fastskill/target/debug/fastskill --help";
        assert_eq!(normalize_snapshot_output(input, &settings), expected);
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

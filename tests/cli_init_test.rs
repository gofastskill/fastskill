//! CLI tests for init command

#![allow(clippy::all, clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn get_binary_path() -> String {
    // Use absolute path from manifest directory
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let debug_path = format!("{}/target/debug/fastskill", manifest_dir);
    let release_path = format!("{}/target/release/fastskill", manifest_dir);

    if Path::new(&debug_path).exists() {
        debug_path
    } else if Path::new(&release_path).exists() {
        release_path
    } else {
        // Fallback to cargo run for testing
        "cargo".to_string()
    }
}

#[test]
fn test_init_command_in_empty_directory() {
    let temp_dir = TempDir::new().unwrap();
    // Create a subdirectory with a valid skill ID name
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let original_dir = std::env::current_dir().unwrap().canonicalize().unwrap();

    std::env::set_current_dir(&skill_dir).unwrap();

    let binary = get_binary_path();
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = if binary == "cargo" {
        Command::new("cargo")
            .current_dir(&skill_dir)
            .args(&[
                "run",
                "--manifest-path",
                manifest_path.to_str().unwrap(),
                "--bin",
                "fastskill",
                "--",
                "init",
                "--yes",
                "--skills-dir",
                ".claude/skills",
            ])
            .output()
            .expect("Failed to execute init command")
    } else {
        Command::new(&binary)
            .current_dir(&skill_dir)
            .args(&["init", "--yes", "--skills-dir", ".claude/skills"])
            .output()
            .expect("Failed to execute init command")
    };

    // Verify skill-project.toml was created (check before changing directory)
    let project_toml = skill_dir.join("skill-project.toml");

    // Debug: check if command succeeded
    if !output.status.success() {
        eprintln!("Command failed. Status: {:?}", output.status);
        eprintln!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
        eprintln!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
    }

    // Change back to original directory, handling potential race conditions
    if let Err(e) = std::env::set_current_dir(&original_dir) {
        // If we can't change back, try to change to a known safe directory
        std::env::set_current_dir("/").unwrap_or_else(|_| {
            panic!("Failed to restore directory to {:?}: {}", original_dir, e);
        });
    }

    assert!(
        project_toml.exists(),
        "skill-project.toml should be created at {:?}",
        project_toml
    );

    // Verify content
    let content = fs::read_to_string(&project_toml).unwrap();
    assert!(
        content.contains("[metadata]"),
        "Should contain [metadata] section"
    );
    assert!(
        content.contains("version = \"1.0.0\""),
        "Should have default version"
    );
}

#[test]
fn test_init_command_with_existing_file() {
    let temp_dir = TempDir::new().unwrap();
    // Create a subdirectory with a valid skill ID name
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let original_dir = std::env::current_dir().unwrap().canonicalize().unwrap();

    std::env::set_current_dir(&skill_dir).unwrap();

    // Create existing skill-project.toml
    fs::write("skill-project.toml", "[metadata]\nversion = \"0.1.0\"\n").unwrap();

    let binary = get_binary_path();
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = if binary == "cargo" {
        Command::new("cargo")
            .current_dir(&skill_dir)
            .args(&[
                "run",
                "--manifest-path",
                manifest_path.to_str().unwrap(),
                "--bin",
                "fastskill",
                "--",
                "init",
                "--yes",
                "--skills-dir",
                ".claude/skills",
            ])
            .output()
            .expect("Failed to execute init command")
    } else {
        Command::new(&binary)
            .current_dir(&skill_dir)
            .args(&["init", "--yes", "--skills-dir", ".claude/skills"])
            .output()
            .expect("Failed to execute init command")
    };

    // Change back to original directory, handling potential race conditions
    if let Err(e) = std::env::set_current_dir(&original_dir) {
        // If we can't change back, try to change to a known safe directory
        std::env::set_current_dir("/").unwrap_or_else(|_| {
            panic!("Failed to restore directory to {:?}: {}", original_dir, e);
        });
    }

    // Should error (non-zero exit code or error message)
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Either exit code should be non-zero OR error message should be present
    assert!(
        !output.status.success()
            || stderr.contains("already exists")
            || stdout.contains("already exists")
            || stderr.contains("--force")
            || stdout.contains("--force"),
        "Should error when skill-project.toml already exists. stderr: {}, stdout: {}",
        stderr,
        stdout
    );
}

#[test]
fn test_init_command_with_force_flag() {
    let temp_dir = TempDir::new().unwrap();
    // Create a subdirectory with a valid skill ID name
    let skill_dir = temp_dir.path().join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let original_dir = std::env::current_dir().unwrap().canonicalize().unwrap();

    std::env::set_current_dir(&skill_dir).unwrap();

    // Create existing skill-project.toml
    fs::write("skill-project.toml", "[metadata]\nversion = \"0.1.0\"\n").unwrap();

    let binary = get_binary_path();
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = if binary == "cargo" {
        Command::new("cargo")
            .current_dir(&skill_dir)
            .args(&[
                "run",
                "--manifest-path",
                manifest_path.to_str().unwrap(),
                "--bin",
                "fastskill",
                "--",
                "init",
                "--yes",
                "--force",
                "--version",
                "2.0.0",
                "--skills-dir",
                ".claude/skills",
            ])
            .output()
            .expect("Failed to execute init command")
    } else {
        Command::new(&binary)
            .current_dir(&skill_dir)
            .args(&[
                "init",
                "--yes",
                "--force",
                "--version",
                "2.0.0",
                "--skills-dir",
                ".claude/skills",
            ])
            .output()
            .expect("Failed to execute init command")
    };

    // Debug: print output if command failed
    if !output.status.success() {
        eprintln!(
            "Command failed. Binary: {}, Status: {:?}",
            binary, output.status
        );
        eprintln!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
        eprintln!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
    }

    // Change back to original directory, handling potential race conditions
    if let Err(e) = std::env::set_current_dir(&original_dir) {
        // If we can't change back, try to change to a known safe directory
        std::env::set_current_dir("/").unwrap_or_else(|_| {
            panic!("Failed to restore directory to {:?}: {}", original_dir, e);
        });
    }

    // Should succeed with --force
    if !output.status.success() {
        eprintln!("Command failed. Status: {:?}", output.status);
        eprintln!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
        eprintln!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
    }
    assert!(
        output.status.success(),
        "Should succeed with --force flag. Stderr: {}, Stdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    // Verify file was overwritten with new version
    let project_toml = skill_dir.join("skill-project.toml");
    assert!(
        project_toml.exists(),
        "File should exist at: {:?}",
        project_toml
    );
    let content = fs::read_to_string(&project_toml)
        .unwrap_or_else(|e| panic!("Failed to read file at {:?}: {}", project_toml, e));
    assert!(
        content.contains("version = \"2.0.0\""),
        "Should have new version. Content: {}",
        content
    );
}

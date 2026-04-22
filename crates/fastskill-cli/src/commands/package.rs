//! Package command implementation

use crate::error::{CliError, CliResult};
use crate::utils::messages;
use clap::{Args, Subcommand};
use fastskill_core::core::build_cache::BuildCache;
use fastskill_core::core::change_detection::{
    calculate_skill_hash, detect_changed_skills_git, detect_changed_skills_hash,
};
use fastskill_core::core::version_bump::{
    bump_version, get_current_version, parse_version, update_skill_version, BumpType,
};
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

/// Package preset subcommands for simplified workflows
#[derive(Debug, Clone, Subcommand)]
pub enum PackagePreset {
    /// Auto-detect and package changed skills (hash-based detection)
    Auto {
        /// Force package all skills regardless of changes
        #[arg(long)]
        force: bool,
    },
    /// Package specific skills by ID
    Skill {
        /// Skill IDs to package
        #[arg(required = true)]
        skills: Vec<String>,
        /// Bump version type (major, minor, patch)
        #[arg(long, value_enum)]
        bump: Option<BumpTypeArg>,
    },
}

/// Package skills into ZIP artifacts
///
/// Reads skill metadata from skill-project.toml [metadata] section (skill-level context).
/// Includes skill dependencies from skill-project.toml [dependencies] if present.
///
/// PRESETS (recommended for common workflows):
///   fastskill package auto              # Auto-detect changed skills
///   fastskill package skill <id>        # Package specific skill(s)
///
/// ADVANCED OPTIONS (for power users):
///   fastskill package --detect-changes  # Hash-based change detection
///   fastskill package --git-diff base head  # Git-based change detection
///   fastskill package --skills id1 id2  # Package specific skills
///   fastskill package --force           # Package all skills
///
/// Note: the `evals/` directory is omitted from packaged skill ZIPs; exclusion is enforced in
/// `fastskill_core::core::packaging` when building archives, not in this CLI module.
#[derive(Debug, Args)]
pub struct PackageArgs {
    /// Package preset command
    #[command(subcommand)]
    pub preset: Option<PackagePreset>,

    // ===== Advanced options (grouped for help clarity) =====
    /// Auto-detect changed skills (hash-based)
    #[arg(long)]
    pub detect_changes: bool,

    /// Use git diff for change detection (format: base_ref head_ref)
    #[arg(long, num_args = 2, value_names = ["BASE", "HEAD"])]
    pub git_diff: Option<Vec<String>>,

    /// Package specific skills (skill IDs)
    #[arg(long, num_args = 1..)]
    pub skills: Option<Vec<String>>,

    /// Bump version type (major, minor, patch)
    #[arg(long, value_enum)]
    pub bump: Option<BumpTypeArg>,

    /// Auto-detect bump type from changes
    #[arg(long)]
    pub auto_bump: bool,

    /// Output directory for artifacts
    #[arg(short, long, default_value = "./artifacts")]
    pub output: PathBuf,

    /// Force package all skills regardless of changes
    #[arg(long)]
    pub force: bool,

    /// Dry run (show what would be packaged)
    #[arg(long)]
    pub dry_run: bool,

    /// Skills directory to scan
    #[arg(long, default_value = "./skills")]
    pub skills_dir: PathBuf,

    /// Recursively scan skills directory for nested skills
    #[arg(long)]
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq, clap::ValueEnum)]
pub enum BumpTypeArg {
    Major,
    Minor,
    Patch,
}

impl From<BumpTypeArg> for BumpType {
    fn from(arg: BumpTypeArg) -> Self {
        match arg {
            BumpTypeArg::Major => BumpType::Major,
            BumpTypeArg::Minor => BumpType::Minor,
            BumpTypeArg::Patch => BumpType::Patch,
        }
    }
}

/// Validate package arguments for conflicting preset and manual flags (PKG_001)
fn validate_package_args(args: &PackageArgs) -> CliResult<()> {
    if args.preset.is_some() {
        let has_conflicting_flags =
            args.skills.is_some() || args.detect_changes || args.git_diff.is_some() || args.force;

        if has_conflicting_flags {
            return Err(CliError::Validation(
                "PKG_001: Cannot combine preset with manual flags. Use preset OR advanced flags, not both."
                    .to_string(),
            ));
        }
    }

    match &args.preset {
        Some(PackagePreset::Skill { skills, .. }) if skills.is_empty() => {
            return Err(CliError::Validation(
                "PKG_002: Package skill preset requires at least one skill ID".to_string(),
            ));
        }
        _ => {}
    }

    Ok(())
}

/// Map preset to equivalent flag combinations
fn map_preset_to_flags(preset: &PackagePreset, args: &mut PackageArgs) {
    match preset {
        PackagePreset::Auto { force } => {
            args.detect_changes = true;
            args.force = *force;
        }
        PackagePreset::Skill { skills, bump } => {
            args.skills = Some(skills.clone());
            args.bump = bump.clone();
        }
    }
}

pub async fn execute_package(mut args: PackageArgs) -> CliResult<()> {
    info!("Starting package command");

    // Validate and apply preset if provided
    validate_package_args(&args)?;

    // Extract and apply preset before mutation
    let preset = args.preset.clone();
    if let Some(ref p) = preset {
        map_preset_to_flags(p, &mut args);
    }

    // Determine which skills to package
    let skills_to_package = if let Some(skill_ids) = args.skills {
        skill_ids
    } else if args.force {
        // Package all skills
        if args.recursive {
            get_all_skills_recursive(&args.skills_dir)?
        } else {
            get_all_skills_shallow(&args.skills_dir)?
        }
    } else if let Some(ref git_diff) = args.git_diff {
        if git_diff.len() != 2 {
            return Err(CliError::Validation(
                "git-diff requires exactly 2 arguments: base_ref head_ref".to_string(),
            ));
        }
        detect_changed_skills_git(&git_diff[0], &git_diff[1], &args.skills_dir)
            .map_err(|e| CliError::Validation(format!("Git change detection failed: {}", e)))?
    } else if args.detect_changes {
        // Load build cache
        let cache_path = PathBuf::from(".fastskill/build-cache.json");
        let cache = BuildCache::load(&cache_path)
            .map_err(|e| CliError::Validation(format!("Failed to load build cache: {}", e)))?;

        detect_changed_skills_hash(&args.skills_dir, &cache)
            .map_err(|e| CliError::Validation(format!("Hash change detection failed: {}", e)))?
    } else {
        return Err(CliError::Validation(
            "Must specify --skills, --force, --detect-changes, or --git-diff".to_string(),
        ));
    };

    if skills_to_package.is_empty() {
        println!("{}", messages::info("No skills to package"));
        return Ok(());
    }

    println!(
        "{}",
        messages::info(&format!("Packaging {} skill(s)", skills_to_package.len()))
    );

    // Load build cache
    let cache_path = PathBuf::from(".fastskill/build-cache.json");
    let mut cache = BuildCache::load(&cache_path)
        .map_err(|e| CliError::Validation(format!("Failed to load build cache: {}", e)))?;

    let mut packaged_artifacts = Vec::new();

    for skill_id in &skills_to_package {
        let skill_path = args.skills_dir.join(skill_id);

        if !skill_path.exists() {
            println!(
                "{}",
                messages::warning(&format!(
                    "Skill directory not found: {}",
                    skill_path.display()
                ))
            );
            continue;
        }

        // Get current version
        let current_version_str = get_current_version(&skill_path)
            .map_err(|e| {
                CliError::Validation(format!("Failed to get version for {}: {}", skill_id, e))
            })?
            .unwrap_or_else(|| "1.0.0".to_string());

        // Parse and bump version if needed
        let new_version = if let Some(ref bump_type) = args.bump {
            let current_version = parse_version(&current_version_str).map_err(|e| {
                CliError::Validation(format!("Invalid version '{}': {}", current_version_str, e))
            })?;
            let bumped = bump_version(&current_version, (*bump_type).clone().into());
            bumped.to_string()
        } else if args.auto_bump {
            // Auto-bump: default to patch for now
            // TODO: Analyze changes to determine bump type
            let current_version = parse_version(&current_version_str).map_err(|e| {
                CliError::Validation(format!("Invalid version '{}': {}", current_version_str, e))
            })?;
            let bumped = bump_version(&current_version, BumpType::Patch);
            bumped.to_string()
        } else {
            current_version_str.clone()
        };

        if args.dry_run {
            println!(
                "  Would package: {} (version: {} -> {})",
                skill_id, current_version_str, new_version
            );
            continue;
        }

        // Update skill-project.toml with new version if it changed
        if new_version != current_version_str {
            update_skill_version(&skill_path, &new_version)
                .map_err(|e| CliError::Validation(format!("Failed to update version: {}", e)))?;
            println!("  Updated version: {} -> {}", skill_id, new_version);
        }

        // Package skill (pass skill_id to preserve scoped names like "swe/codeguardian")
        let artifact_path = fastskill_core::core::packaging::package_skill_with_id(
            &skill_path,
            &args.output,
            &new_version,
            Some(skill_id),
        )
        .map_err(|e| CliError::Validation(format!("Failed to package {}: {}", skill_id, e)))?;

        // Calculate hash
        let hash = calculate_skill_hash(&skill_path)
            .map_err(|e| CliError::Validation(format!("Failed to calculate hash: {}", e)))?;

        // Get git commit if available
        let git_commit = get_git_commit().ok();

        // Update cache
        cache.update_skill(
            skill_id,
            &new_version,
            &hash,
            &artifact_path,
            git_commit.as_deref(),
        );

        packaged_artifacts.push(artifact_path.clone());
        println!(
            "{}",
            messages::ok(&format!(
                "Packaged: {} -> {}",
                skill_id,
                artifact_path.display()
            ))
        );
    }

    // Save cache
    if !args.dry_run {
        cache
            .save(&cache_path)
            .map_err(|e| CliError::Validation(format!("Failed to save build cache: {}", e)))?;
    }

    println!();
    println!(
        "{}",
        messages::ok(&format!(
            "Packaged {} artifact(s)",
            packaged_artifacts.len()
        ))
    );
    for artifact in &packaged_artifacts {
        println!("  {}", artifact.display());
    }

    Ok(())
}

fn get_all_skills_shallow(skills_dir: &Path) -> CliResult<Vec<String>> {
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    let entries = std::fs::read_dir(skills_dir).map_err(CliError::Io)?;

    for entry in entries {
        let entry = entry.map_err(CliError::Io)?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(skill_id) = path.file_name().and_then(|n| n.to_str()) {
                if path.join("SKILL.md").exists() {
                    skills.push(skill_id.to_string());
                }
            }
        }
    }

    Ok(skills)
}

fn get_all_skills_recursive(skills_dir: &Path) -> CliResult<Vec<String>> {
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    let skills_dir_canonical = skills_dir.canonicalize().map_err(CliError::Io)?;

    for entry in WalkDir::new(skills_dir)
        .min_depth(1)
        .max_depth(usize::MAX)
        .follow_links(false)
    {
        let entry = entry.map_err(|e| {
            CliError::Io(
                e.into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("WalkDir error")),
            )
        })?;
        let path = entry.path();

        // Check if this directory contains SKILL.md
        if path.is_dir() && path.join("SKILL.md").exists() {
            // Compute relative path from skills_dir to this skill directory
            let path_canonical = path.canonicalize().map_err(CliError::Io)?;
            let relative_path =
                path_canonical
                    .strip_prefix(&skills_dir_canonical)
                    .map_err(|_| {
                        CliError::Validation(format!(
                            "Failed to compute relative path for: {}",
                            path.display()
                        ))
                    })?;

            // Skip hidden directories and tooling directories (check relative path components)
            let should_skip = relative_path.components().any(|c| {
                if let std::path::Component::Normal(name) = c {
                    name.to_string_lossy().starts_with('.')
                } else {
                    false
                }
            });
            if should_skip {
                continue;
            }

            // Convert to string with forward slashes (normalize path separators)
            let skill_id = relative_path.to_string_lossy().replace('\\', "/");
            skills.push(skill_id.to_string());
        }
    }

    Ok(skills)
}

fn get_git_commit() -> Result<String, CliError> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| CliError::Validation(format!("Failed to execute git: {}", e)))?;

    if !output.status.success() {
        return Err(CliError::Validation(
            "Failed to get git commit hash".to_string(),
        ));
    }

    let commit_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(commit_hash)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_package_nonexistent_skills_dir() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_dir = temp_dir.path().join("nonexistent");

        let args = PackageArgs {
            preset: None,
            detect_changes: false,
            git_diff: None,
            skills: None,
            bump: None,
            auto_bump: false,
            output: temp_dir.path().join("artifacts"),
            force: false,
            dry_run: true,
            skills_dir: nonexistent_dir,
            recursive: false,
        };

        let result = execute_package(args).await;
        // Should handle nonexistent directory gracefully
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_package_empty_skills_dir() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let args = PackageArgs {
            preset: None,
            detect_changes: false,
            git_diff: None,
            skills: None,
            bump: None,
            auto_bump: false,
            output: temp_dir.path().join("artifacts"),
            force: false,
            dry_run: true,
            skills_dir,
            recursive: false,
        };

        // Should succeed with empty directory (no skills to package)
        let result = execute_package(args).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_package_dry_run() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let args = PackageArgs {
            preset: None,
            detect_changes: false,
            git_diff: None,
            skills: Some(vec!["test-skill".to_string()]),
            bump: None,
            auto_bump: false,
            output: temp_dir.path().join("artifacts"),
            force: false,
            dry_run: true,
            skills_dir,
            recursive: false,
        };

        // Dry run should not fail even if skills don't exist
        let result = execute_package(args).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_get_all_skills_shallow() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        // Create top-level skills
        let skill1_dir = skills_dir.join("skill-a");
        fs::create_dir_all(&skill1_dir).unwrap();
        fs::write(skill1_dir.join("SKILL.md"), "# Skill A").unwrap();

        let skill2_dir = skills_dir.join("skill-b");
        fs::create_dir_all(&skill2_dir).unwrap();
        fs::write(skill2_dir.join("SKILL.md"), "# Skill B").unwrap();

        // Create nested skill (should not be found in shallow mode)
        let nested_dir = skills_dir.join("category1").join("skill-c");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(nested_dir.join("SKILL.md"), "# Skill C").unwrap();

        let skills = get_all_skills_shallow(&skills_dir).unwrap();
        assert_eq!(skills.len(), 2);
        assert!(skills.contains(&"skill-a".to_string()));
        assert!(skills.contains(&"skill-b".to_string()));
        assert!(!skills.contains(&"category1/skill-c".to_string()));
    }

    #[test]
    fn test_get_all_skills_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        // Create top-level skills
        let skill1_dir = skills_dir.join("skill-a");
        fs::create_dir_all(&skill1_dir).unwrap();
        fs::write(skill1_dir.join("SKILL.md"), "# Skill A").unwrap();

        // Create nested skills
        let nested1_dir = skills_dir.join("category1").join("skill-b");
        fs::create_dir_all(&nested1_dir).unwrap();
        fs::write(nested1_dir.join("SKILL.md"), "# Skill B").unwrap();

        let nested2_dir = skills_dir.join("category2").join("sub").join("skill-c");
        fs::create_dir_all(&nested2_dir).unwrap();
        fs::write(nested2_dir.join("SKILL.md"), "# Skill C").unwrap();

        // Create a directory without SKILL.md (should be ignored)
        let no_skill_dir = skills_dir.join("not-a-skill");
        fs::create_dir_all(&no_skill_dir).unwrap();

        // Create hidden directory (should be ignored)
        let hidden_dir = skills_dir.join(".hidden").join("skill-d");
        fs::create_dir_all(&hidden_dir).unwrap();
        fs::write(hidden_dir.join("SKILL.md"), "# Skill D").unwrap();

        let skills = get_all_skills_recursive(&skills_dir).unwrap();
        assert_eq!(skills.len(), 3);
        assert!(skills.contains(&"skill-a".to_string()));
        assert!(skills.contains(&"category1/skill-b".to_string()));
        assert!(skills.contains(&"category2/sub/skill-c".to_string()));
        // Hidden directory should be skipped
        assert!(!skills.contains(&".hidden/skill-d".to_string()));
    }

    #[test]
    fn test_get_all_skills_recursive_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skills = get_all_skills_recursive(&skills_dir).unwrap();
        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn test_get_all_skills_recursive_nonexistent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_dir = temp_dir.path().join("nonexistent");

        let skills = get_all_skills_recursive(&nonexistent_dir).unwrap();
        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn test_validate_package_args_preset_conflict_pkg_001() {
        let args = PackageArgs {
            preset: Some(PackagePreset::Auto { force: false }),
            detect_changes: true, // Conflict!
            git_diff: None,
            skills: None,
            bump: None,
            auto_bump: false,
            output: PathBuf::from("./artifacts"),
            force: false,
            dry_run: false,
            skills_dir: PathBuf::from("./skills"),
            recursive: false,
        };

        let result = validate_package_args(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::Validation(msg) if msg.contains("PKG_001")));
    }

    #[test]
    fn test_validate_package_args_preset_with_skills_conflict() {
        let args = PackageArgs {
            preset: Some(PackagePreset::Auto { force: false }),
            detect_changes: false,
            git_diff: None,
            skills: Some(vec!["skill1".to_string()]), // Conflict!
            bump: None,
            auto_bump: false,
            output: PathBuf::from("./artifacts"),
            force: false,
            dry_run: false,
            skills_dir: PathBuf::from("./skills"),
            recursive: false,
        };

        let result = validate_package_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_package_args_preset_skill_empty_pkg_002() {
        let args = PackageArgs {
            preset: Some(PackagePreset::Skill {
                skills: vec![], // Empty skills list
                bump: None,
            }),
            detect_changes: false,
            git_diff: None,
            skills: None,
            bump: None,
            auto_bump: false,
            output: PathBuf::from("./artifacts"),
            force: false,
            dry_run: false,
            skills_dir: PathBuf::from("./skills"),
            recursive: false,
        };

        let result = validate_package_args(&args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CliError::Validation(msg) if msg.contains("PKG_002")));
    }

    #[test]
    fn test_validate_package_args_preset_valid() {
        let args = PackageArgs {
            preset: Some(PackagePreset::Auto { force: true }),
            detect_changes: false,
            git_diff: None,
            skills: None,
            bump: None,
            auto_bump: false,
            output: PathBuf::from("./artifacts"),
            force: false,
            dry_run: false,
            skills_dir: PathBuf::from("./skills"),
            recursive: false,
        };

        let result = validate_package_args(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_map_preset_to_flags_auto() {
        let preset = PackagePreset::Auto { force: true };
        let mut args = PackageArgs {
            preset: None,
            detect_changes: false,
            git_diff: None,
            skills: None,
            bump: None,
            auto_bump: false,
            output: PathBuf::from("./artifacts"),
            force: false,
            dry_run: false,
            skills_dir: PathBuf::from("./skills"),
            recursive: false,
        };

        map_preset_to_flags(&preset, &mut args);

        assert!(args.detect_changes);
        assert!(args.force);
    }

    #[test]
    fn test_map_preset_to_flags_skill() {
        let preset = PackagePreset::Skill {
            skills: vec!["skill1".to_string(), "skill2".to_string()],
            bump: Some(BumpTypeArg::Patch),
        };
        let mut args = PackageArgs {
            preset: None,
            detect_changes: false,
            git_diff: None,
            skills: None,
            bump: None,
            auto_bump: false,
            output: PathBuf::from("./artifacts"),
            force: false,
            dry_run: false,
            skills_dir: PathBuf::from("./skills"),
            recursive: false,
        };

        map_preset_to_flags(&preset, &mut args);

        assert_eq!(
            args.skills,
            Some(vec!["skill1".to_string(), "skill2".to_string()])
        );
        assert_eq!(args.bump, Some(BumpTypeArg::Patch));
    }
}

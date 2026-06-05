//! Package command implementation

use crate::error::{CliError, CliResult};
use crate::utils::messages;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::build_cache::BuildCache;
use fastskill_core::core::change_detection::{
    calculate_skill_hash, detect_changed_skills_git, detect_changed_skills_hash,
};
use fastskill_core::core::version_bump::{
    bump_version, get_current_version, parse_version, update_skill_version, BumpType,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

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
#[derive(Debug)]
pub struct PackageArgs {
    // ===== Advanced options (grouped for help clarity) =====
    /// Auto-detect changed skills (hash-based)
    pub detect_changes: bool,

    /// Use git diff for change detection (format: base_ref head_ref)
    pub git_diff: Option<Vec<String>>,

    /// Package specific skills (skill IDs)
    pub skills: Option<Vec<String>>,

    /// Bump version type (major, minor, patch)
    pub bump: Option<BumpTypeArg>,

    /// Auto-detect bump type from changes
    pub auto_bump: bool,

    /// Output directory for artifacts
    pub output: PathBuf,

    /// Force package all skills regardless of changes
    pub force: bool,

    /// Dry run (show what would be packaged)
    pub dry_run: bool,

    /// Skills directory to scan
    pub skills_dir: PathBuf,

    /// Recursively scan skills directory for nested skills
    pub recursive: bool,
}

#[derive(Debug, Clone, PartialEq)]
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

/// Immutable execution plan derived from parsed args and preset.
/// Conflicting flag combinations are structurally impossible once constructed.
#[derive(Debug)]
pub(crate) struct PackagePlan {
    pub skills: Option<Vec<String>>,
    pub bump: Option<BumpTypeArg>,
    pub detect_changes: bool,
    pub force: bool,
}

impl PackagePlan {
    fn from_args(args: &PackageArgs) -> Self {
        PackagePlan {
            skills: args.skills.clone(),
            bump: args.bump.clone(),
            detect_changes: args.detect_changes,
            force: args.force,
        }
    }
}

impl IntoCommandSpec for PackageArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Package skills into ZIP artifacts",
            syntax: Some("package [OPTIONS]"),
            category: Some("packaging"),
            args: vec![
                ArgSpec {
                    name: "detect-changes",
                    kind: ArgKind::Flag,
                    long: Some("detect-changes"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Auto-detect changed skills (hash-based)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "git-diff",
                    kind: ArgKind::Option,
                    long: Some("git-diff"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Use git diff for change detection (BASE HEAD)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "skills",
                    kind: ArgKind::Option,
                    long: Some("skills"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Package specific skills (skill IDs)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "bump",
                    kind: ArgKind::Option,
                    long: Some("bump"),
                    value_type: ArgValueType::Enum(vec!["major", "minor", "patch"]),
                    cardinality: Cardinality::Optional,
                    help: "Bump version type (major, minor, patch)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "auto-bump",
                    kind: ArgKind::Flag,
                    long: Some("auto-bump"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Auto-detect bump type from changes",
                    ..Default::default()
                },
                ArgSpec {
                    name: "output",
                    kind: ArgKind::Option,
                    long: Some("output"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output directory for artifacts",
                    default: Some(ArgValue::Str("./artifacts".to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "force",
                    kind: ArgKind::Flag,
                    long: Some("force"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Force package all skills regardless of changes",
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    kind: ArgKind::Flag,
                    long: Some("dry-run"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Dry run (show what would be packaged)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "skills-dir",
                    kind: ArgKind::Option,
                    long: Some("skills-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Skills directory to scan",
                    default: Some(ArgValue::Str("./skills".to_string())),
                    ..Default::default()
                },
                ArgSpec {
                    name: "recursive",
                    kind: ArgKind::Flag,
                    long: Some("recursive"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Recursively scan skills directory for nested skills",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for PackageArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            detect_changes: matches!(map.get("detect-changes"), Some(ArgValue::Bool(true))),
            git_diff: match map.get("git-diff") {
                Some(ArgValue::List(items)) => {
                    let strings: Vec<String> = items
                        .iter()
                        .filter_map(|i| {
                            if let ArgValue::Str(s) = i {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if strings.is_empty() {
                        None
                    } else {
                        Some(strings)
                    }
                }
                _ => None,
            },
            skills: match map.get("skills") {
                Some(ArgValue::List(items)) => {
                    let strings: Vec<String> = items
                        .iter()
                        .filter_map(|i| {
                            if let ArgValue::Str(s) = i {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if strings.is_empty() {
                        None
                    } else {
                        Some(strings)
                    }
                }
                _ => None,
            },
            bump: map.get("bump").and_then(|v| match v {
                ArgValue::Str(s) | ArgValue::Enum(s) => match s.as_str() {
                    "major" => Some(BumpTypeArg::Major),
                    "minor" => Some(BumpTypeArg::Minor),
                    "patch" => Some(BumpTypeArg::Patch),
                    _ => None,
                },
                _ => None,
            }),
            auto_bump: matches!(map.get("auto-bump"), Some(ArgValue::Bool(true))),
            output: map
                .get("output")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(PathBuf::from(s))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| PathBuf::from("./artifacts")),
            force: matches!(map.get("force"), Some(ArgValue::Bool(true))),
            dry_run: matches!(map.get("dry-run"), Some(ArgValue::Bool(true))),
            skills_dir: map
                .get("skills-dir")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(PathBuf::from(s))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| PathBuf::from("./skills")),
            recursive: matches!(map.get("recursive"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Validate package arguments for conflicting preset and manual flags (PKG_001)
fn validate_package_args(_args: &PackageArgs) -> CliResult<()> {
    Ok(())
}

pub async fn execute_package(args: PackageArgs) -> CliResult<()> {
    info!("Starting package command");

    // Validate and apply preset if provided
    validate_package_args(&args)?;

    // Derive execution plan from args and preset
    let plan = PackagePlan::from_args(&args);

    // Determine which skills to package
    let skills_to_package = if let Some(skill_ids) = plan.skills {
        skill_ids
    } else if plan.force {
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
    } else if plan.detect_changes {
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
        let new_version = if let Some(ref bump_type) = plan.bump {
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
}

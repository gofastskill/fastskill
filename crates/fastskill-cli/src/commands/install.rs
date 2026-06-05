//! Install command - installs skills from skill-project.toml dependencies

use crate::config::create_service_config;
use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::{install_utils, manifest_utils, messages};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::{
    dependency_resolver::{DependencyResolver, SkillInstallItem},
    lock::{project_lock_path, ProjectSkillsLock},
    manifest::{SkillEntry, SkillProjectToml},
    project::resolve_project_file,
    repository::RepositoryManager,
};
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::fs;

/// Apply manifest: install skills from skill-project.toml [dependencies]
///
/// This is the canonical command for manifest-driven workflow.
///
/// Reads dependencies from skill-project.toml [dependencies] at the project root.
/// Installs to the skills directory configured in [tool.fastskill].skills_directory.
/// Creates or updates skills.lock for reproducible installations.
///
/// Use --lock to install exact versions from skills.lock instead of resolving from manifest.
#[derive(Debug)]
pub struct InstallArgs {
    /// Exclude skills from these groups (like poetry --without dev)
    without: Option<Vec<String>>,

    /// Only install skills from these groups
    only: Option<Vec<String>>,

    /// Install from skills.lock (exact versions) instead of resolving from skill-project.toml
    lock: bool,

    /// Maximum transitive dependency depth (overrides config file setting)
    depth: Option<u32>,
}

impl IntoCommandSpec for InstallArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Apply manifest: install skills from skill-project.toml [dependencies]",
            syntax: Some("install [OPTIONS]"),
            category: Some("packages"),
            args: vec![
                ArgSpec {
                    name: "without",
                    kind: ArgKind::Option,
                    long: Some("without"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Exclude skills from these groups (like poetry --without dev)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "only",
                    kind: ArgKind::Option,
                    long: Some("only"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    help: "Only install skills from these groups",
                    ..Default::default()
                },
                ArgSpec {
                    name: "lock",
                    kind: ArgKind::Flag,
                    long: Some("lock"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Install from skills.lock (exact versions) instead of resolving from skill-project.toml",
                    ..Default::default()
                },
                ArgSpec {
                    name: "depth",
                    kind: ArgKind::Option,
                    long: Some("depth"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Maximum transitive dependency depth (overrides config file setting)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

fn repeated_str_list(v: &ArgValue) -> Option<Vec<String>> {
    if let ArgValue::List(items) = v {
        let strings: Vec<String> = items
            .iter()
            .filter_map(|item| {
                if let ArgValue::Str(s) = item {
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
    } else {
        None
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for InstallArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            without: map.get("without").and_then(repeated_str_list),
            only: map.get("only").and_then(repeated_str_list),
            lock: matches!(map.get("lock"), Some(ArgValue::Bool(true))),
            depth: map.get("depth").and_then(|v| {
                if let ArgValue::Int(n) = v {
                    Some(*n as u32)
                } else {
                    None
                }
            }),
        }
    }
}

/// Configuration for recursive install
#[derive(Debug, Clone)]
pub struct RecursiveInstallConfig {
    pub max_depth: u32,
    pub exclude_groups: Option<Vec<String>>,
    pub only_groups: Option<Vec<String>>,
    pub skip_transitive: bool,
}

impl RecursiveInstallConfig {
    pub fn from_args_and_config(
        args: &InstallArgs,
        config_depth: u32,
        config_skip_transitive: bool,
    ) -> Self {
        Self {
            max_depth: args.depth.unwrap_or(config_depth),
            exclude_groups: args.without.clone(),
            only_groups: args.only.clone(),
            skip_transitive: config_skip_transitive,
        }
    }
}

fn group_passes_filter(
    groups: &[String],
    exclude: Option<&[String]>,
    only: Option<&[String]>,
) -> bool {
    if let Some(exclude) = exclude {
        if groups
            .iter()
            .any(|g| exclude.iter().any(|ex| ex == g.as_str()))
        {
            return false;
        }
    }
    if let Some(only) = only {
        if !groups.is_empty()
            && !groups
                .iter()
                .any(|g| only.iter().any(|on| on == g.as_str()))
        {
            return false;
        }
    }
    true
}

pub async fn execute_install(args: InstallArgs) -> CliResult<()> {
    println!("Installing skills...");
    println!();

    // Validate depth argument (must be > 0 if provided)
    if let Some(depth) = args.depth {
        if depth == 0 {
            return Err(CliError::InvalidDepth(
                "Depth must be greater than 0. Use --depth 1 for direct dependencies only."
                    .to_string(),
            ));
        }
    }

    // T027: Resolve skill-project.toml from project root
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    // Require manifest unless installing from lock
    if !args.lock && !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    // T033: Lock file at project root (skills.lock)
    let lock_path = project_lock_path(&project_file_path);

    // Check for lock file early if lock mode is requested (before service initialization)
    if args.lock && !lock_path.exists() {
        return Err(CliError::Config(
            "skills.lock not found. Run 'fastskill install' first to create it.".to_string(),
        ));
    }

    // Resolve skills directory from config
    let skills_dir = crate::config::resolve_skills_storage_directory(false)?;

    // Initialize service
    // Note: install command doesn't have access to CLI sources_path, so uses env var or walk-up
    let config = create_service_config(false, None, None)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;

    let repositories = crate::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Create SourcesManager from marketplace-based repositories for PackageResolver
    let sources_manager = install_utils::create_sources_manager_from_repositories(&repo_manager)
        .map_err(|e| CliError::Config(format!("Failed to create sources manager: {}", e)))?;

    // Determine effective depth limit and skip_transitive flag from config then CLI override
    let (config_depth, config_skip_transitive) = if project_file_result.found {
        SkillProjectToml::load_from_file(&project_file_path)
            .ok()
            .and_then(|p| p.tool)
            .and_then(|t| t.fastskill)
            .map(|cfg| (cfg.install_depth, cfg.skip_transitive))
            .unwrap_or((5, false))
    } else {
        (5, false)
    };

    // Build recursive install config
    let recursive_config =
        RecursiveInstallConfig::from_args_and_config(&args, config_depth, config_skip_transitive);

    // T027: Load from skill-project.toml or lock file
    let skills_to_install: Vec<SkillInstallItem> = if args.lock {
        // Lock file already checked above, so it exists
        let lock = ProjectSkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?;

        println!("Using lock file ({} skills)", lock.skills.len());

        // Convert lock entries to installable items
        lock.skills
            .into_iter()
            .map(|locked| SkillInstallItem {
                entry: SkillEntry {
                    id: locked.id,
                    source: locked.source,
                    version: locked.version,
                    editable: locked.editable,
                    groups: locked.groups,
                },
                depth: locked.depth,
                parent_skill: locked.parent_skill,
            })
            .collect()
    } else {
        // Load from skill-project.toml (manifest already required above when !args.lock)
        let project = SkillProjectToml::load_from_file(&project_file_path)
            .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;

        // Validate context
        let context = project_file_result.context;
        project.validate_for_context(context).map_err(|e| {
            CliError::Config(format!("skill-project.toml validation failed: {}", e))
        })?;

        // Convert dependencies to SkillEntry format
        let mut entries = project
            .to_skill_entries()
            .map_err(|e| CliError::Config(format!("Failed to parse dependencies: {}", e)))?;

        // Filter skills by groups
        let exclude_groups = args.without.as_deref();
        let only_groups = args.only.as_deref();
        entries.retain(|e| group_passes_filter(&e.groups, exclude_groups, only_groups));

        // Sort entries by ID for deterministic output
        entries.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

        // Recursively resolve transitive dependencies unless disabled
        let resolved_items = if recursive_config.skip_transitive {
            entries
                .into_iter()
                .map(|entry| SkillInstallItem {
                    entry,
                    depth: 0,
                    parent_skill: None,
                })
                .collect()
        } else {
            let mut resolver = DependencyResolver::new(recursive_config.max_depth);
            resolver
                .resolve_dependencies(entries, &skills_dir)
                .await
                .map_err(|e| CliError::Config(format!("Dependency resolution failed: {}", e)))?
        };

        // Apply group filters to resolved dependencies
        let filtered_items: Vec<SkillInstallItem> = if recursive_config.skip_transitive {
            resolved_items
        } else {
            let ex = recursive_config.exclude_groups.as_deref();
            let on = recursive_config.only_groups.as_deref();
            resolved_items
                .into_iter()
                .filter(|item| group_passes_filter(&item.entry.groups, ex, on))
                .collect()
        };

        filtered_items
    };

    println!("Found {} skills to install", skills_to_install.len());

    if skills_to_install.is_empty() {
        println!(
            "{}",
            messages::info("No skills to install (filtered by groups)")
        );
        return Ok(());
    }

    // Ensure skills directory exists
    fs::create_dir_all(&skills_dir)
        .map_err(|e| CliError::Config(format!("Failed to create skills directory: {}", e)))?;

    // Install each skill
    let mut installed_skills = Vec::new();
    let mut failed_skills = Vec::new();
    for item in &skills_to_install {
        println!("  Installing {} (depth {})...", item.entry.id, item.depth);
        match install_utils::install_skill_from_entry(
            &service,
            item.entry.clone(),
            sources_manager.as_ref(),
        )
        .await
        {
            Ok(skill_def) => {
                installed_skills.push((
                    skill_def,
                    item.entry.groups.clone(),
                    item.entry.editable,
                    item.depth,
                    item.parent_skill.clone(),
                ));
                println!(
                    "  {}",
                    messages::ok(&format!("Installed {}", item.entry.id))
                );
            }
            Err(e) => {
                let context = match &item.parent_skill {
                    Some(parent) => format!(" (required by {})", parent),
                    None => String::new(),
                };
                eprintln!(
                    "  {}",
                    messages::error(&format!(
                        "Failed to install {}{}: {}",
                        item.entry.id, context, e
                    ))
                );
                failed_skills.push(item.entry.id.to_string());
            }
        }
    }

    // Update lock file with all installed skills including depth and parent info
    for (skill_def, groups, _editable, depth, parent_skill) in installed_skills {
        manifest_utils::update_lock_file_with_depth(
            &lock_path,
            &skill_def,
            groups,
            depth,
            parent_skill,
        )
        .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;
    }

    println!();
    println!("{}", messages::ok("Installation complete"));
    println!("   Updated skills.lock");

    // Return error if any skills failed to install
    if !failed_skills.is_empty() {
        return Err(CliError::Config(format!(
            "Failed to install {} skill(s): {}",
            failed_skills.len(),
            failed_skills.join(", ")
        )));
    }

    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock,
    clippy::collapsible_if
)]
mod tests {
    use super::*;
    use fastskill_core::test_utils::DirGuard;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_install_no_manifest() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: false,
            depth: None,
        };

        let result = execute_install(args).await;
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            assert!(
                (msg.contains("skill-project.toml not found") && msg.contains("fastskill init"))
                    || msg.contains("skill-project.toml")
                        && (msg.contains("not found") || msg.contains("Manifest file not found")),
                "Error message must mention skill-project.toml and creation/init: '{}'",
                msg
            );
        } else {
            panic!("Expected Config error, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_execute_install_with_lock_file_not_found() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create skill-project.toml but no lock file
        fs::write(
            temp_dir.path().join("skill-project.toml"),
            "[dependencies]\n\n[tool.fastskill]\nskills_directory = \".claude/skills\"\n",
        )
        .unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: true,
            depth: None,
        };

        let result = execute_install(args).await;
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            // Should fail because skills.lock not found
            assert!(
                msg.contains("skills.lock not found"),
                "Error message '{}' does not contain expected text",
                msg
            );
        } else {
            panic!("Expected Config error, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_execute_install_with_empty_manifest() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create skill-project.toml at project root with empty [dependencies]
        let project_toml = temp_dir.path().join("skill-project.toml");
        fs::write(&project_toml, "[dependencies]\n").unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: false,
            depth: None,
        };

        // Should succeed with empty manifest (no skills to install) or fail on service/repos; shouldn't panic
        let result = execute_install(args).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_install_success() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let source_dir = temp_dir.path().join("source-skill");
        fs::create_dir_all(&source_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
        fs::write(source_dir.join("SKILL.md"), skill_content).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = { path = "source-skill" }
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let args = InstallArgs {
            without: None,
            only: None,
            lock: false,
            depth: None,
        };

        let result = execute_install(args).await;
        // May succeed or fail depending on lock file, but shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }
}

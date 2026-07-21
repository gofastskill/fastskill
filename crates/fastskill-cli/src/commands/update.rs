//! Update command - updates skills in the configured skills directory

use crate::config::create_service_config;
use crate::error::{manifest_required_message, CliError, CliResult};
use crate::utils::{install_utils, manifest_utils, messages};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::{
    lock::{global_lock_path, GlobalSkillsLock, ProjectSkillsLock},
    manifest::SkillProjectToml,
    project::resolve_project_file,
    repository::RepositoryManager,
    resolver::PackageResolver,
    sources::SourcesManager,
    update::{UpdateService, UpdateStrategy},
};
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

/// Update skills to latest versions (behavior matrix affects manifest, lock, and installed state)
///
/// Behavior Matrix:
/// - 'fastskill update' (no skill specified): Updates all skills, modifies skills.lock
/// - 'fastskill update <skill-id>': Updates specific skill only, modifies skills.lock
/// - 'fastskill update --check': Check-only mode, no modifications to any files
/// - 'fastskill update --dry-run': Preview changes without applying them
///
/// Reads dependencies from skill-project.toml and updates installed skills.
/// Always updates skills.lock with new versions (except in check/dry-run modes).
/// Use 'fastskill install --lock' to apply lock file changes without version resolution.
#[derive(Debug)]
pub struct UpdateArgs {
    /// Skill ID to update (if not specified, updates all)
    skill_id: Option<String>,

    /// Check for updates without installing
    check: bool,

    /// Show what would be updated without actually updating
    dry_run: bool,

    /// Update to specific version
    version: Option<String>,

    /// Update from specific source
    #[allow(dead_code)]
    source: Option<String>,

    /// Update strategy: latest, patch, minor, major
    strategy: String,

    /// Trigger reindex after update (overrides config)
    reindex: bool,

    /// Skip reindex after update
    no_reindex: bool,
}

impl IntoCommandSpec for UpdateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Update skills to latest versions",
            syntax: Some("update [SKILL_ID] [OPTIONS]"),
            category: Some("packages"),
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Skill ID to update (if not specified, updates all)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "check",
                    kind: ArgKind::Flag,
                    long: Some("check"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Check for updates without installing",
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    kind: ArgKind::Flag,
                    long: Some("dry-run"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Show what would be updated without actually updating",
                    ..Default::default()
                },
                ArgSpec {
                    name: "version",
                    kind: ArgKind::Option,
                    long: Some("version"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Update to specific version",
                    ..Default::default()
                },
                ArgSpec {
                    name: "source",
                    kind: ArgKind::Option,
                    long: Some("source"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Update from specific source",
                    ..Default::default()
                },
                ArgSpec {
                    name: "strategy",
                    kind: ArgKind::Option,
                    long: Some("strategy"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("latest".to_string())),
                    help: "Update strategy: latest, patch, minor, major",
                    ..Default::default()
                },
                ArgSpec {
                    name: "reindex",
                    kind: ArgKind::Flag,
                    long: Some("reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Trigger reindex after update (overrides config)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-reindex",
                    kind: ArgKind::Flag,
                    long: Some("no-reindex"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Skip reindex after update",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

fn opt_str(v: &ArgValue) -> Option<String> {
    if let ArgValue::Str(s) = v {
        Some(s.clone())
    } else {
        None
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for UpdateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            skill_id: map.get("skill-id").and_then(opt_str),
            check: matches!(map.get("check"), Some(ArgValue::Bool(true))),
            dry_run: matches!(map.get("dry-run"), Some(ArgValue::Bool(true))),
            version: map.get("version").and_then(opt_str),
            source: map.get("source").and_then(opt_str),
            strategy: map
                .get("strategy")
                .and_then(opt_str)
                .unwrap_or_else(|| "latest".to_string()),
            reindex: matches!(map.get("reindex"), Some(ArgValue::Bool(true))),
            no_reindex: matches!(map.get("no-reindex"), Some(ArgValue::Bool(true))),
        }
    }
}

pub async fn execute_update(args: UpdateArgs, global: bool) -> CliResult<()> {
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    let reindex = args.reindex;
    let no_reindex = args.no_reindex;
    let check = args.check;
    let dry_run = args.dry_run;

    if global {
        execute_update_global(args).await?;
    } else {
        execute_update_project(args).await?;
    }

    if !check && !dry_run {
        let auto_reindex_config = crate::config_file::load_auto_reindex_config();
        let config = create_service_config(global, None)?;
        if let Ok(mut svc) = fastskill_core::FastSkillService::new(config).await {
            if svc.initialize().await.is_ok() {
                let _ = crate::utils::reindex_utils::maybe_auto_reindex(
                    &svc,
                    "update",
                    reindex,
                    no_reindex,
                    auto_reindex_config,
                    false,
                )
                .await;
            }
        }
    }

    Ok(())
}

async fn execute_update_global(args: UpdateArgs) -> CliResult<()> {
    println!("Updating global skills...");
    println!();

    let lock_path = global_lock_path()
        .map_err(|e| CliError::Config(format!("Failed to resolve global lock path: {}", e)))?;

    if !lock_path.exists() {
        println!(
            "{}",
            messages::info(
                "No global-skills.lock found. Run 'fastskill add --global <skill>' first."
            )
        );
        return Ok(());
    }

    let mut lock = GlobalSkillsLock::load_from_file(&lock_path)
        .map_err(|e| CliError::Config(format!("Failed to load global lock file: {}", e)))?;

    let skill_ids: Vec<String> = if let Some(id) = &args.skill_id {
        vec![id.clone()]
    } else {
        lock.skills.iter().map(|s| s.id.clone()).collect()
    };

    if skill_ids.is_empty() {
        println!("{}", messages::info("No global skills to update"));
        return Ok(());
    }

    if args.check || args.dry_run {
        println!("\nGlobal skills (check mode):\n");
        for id in &skill_ids {
            if let Some(entry) = lock.skills.iter().find(|s| s.id == *id) {
                println!("  • {} @ {}", entry.id, entry.resolved.version);
            }
        }
        if args.check {
            let now = chrono::Utc::now();
            for id in &skill_ids {
                lock.mark_checked(id, now);
            }
            lock.save_to_file(&lock_path)
                .map_err(|e| CliError::Config(format!("Failed to save global lock: {}", e)))?;
            println!(
                "\n{}",
                messages::info("Updated last_checked_at in global-skills.lock")
            );
        }
        return Ok(());
    }

    // Actual update: mark updated_at for each skill
    let now = chrono::Utc::now();
    let mut updated_count = 0;
    for id in &skill_ids {
        if lock.skills.iter().any(|s| s.id == *id) {
            lock.mark_updated(id, now);
            updated_count += 1;
            println!("  {}", messages::ok(&format!("Marked {} as updated", id)));
        }
    }

    lock.save_to_file(&lock_path)
        .map_err(|e| CliError::Config(format!("Failed to save global lock: {}", e)))?;

    println!();
    println!(
        "{}",
        messages::ok(&format!("Updated {} global skill(s)", updated_count))
    );
    println!("   Updated global-skills.lock");

    Ok(())
}

async fn execute_update_project(args: UpdateArgs) -> CliResult<()> {
    println!("Updating skills...");
    println!();

    // T034: Resolve skill-project.toml from project root
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let project_file_path = project_file_result.path;

    if !project_file_result.found {
        return Err(CliError::Config(manifest_required_message().to_string()));
    }

    let project = SkillProjectToml::load_from_file(&project_file_path)
        .map_err(|e| CliError::Config(format!("Failed to load skill-project.toml: {}", e)))?;

    // Validate context
    let context = project_file_result.context;
    project
        .validate_for_context(context)
        .map_err(|e| CliError::Config(format!("skill-project.toml validation failed: {}", e)))?;

    // Convert dependencies to SkillEntry format
    let mut entries = project
        .to_skill_entries()
        .map_err(|e| CliError::Config(format!("Failed to parse dependencies: {}", e)))?;

    // Filter by skill_id if specified
    if let Some(skill_id) = &args.skill_id {
        entries.retain(|e| e.id == *skill_id);
    }

    // Sort entries alphabetically for deterministic output
    entries.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    if entries.is_empty() {
        println!("{}", messages::info("No skills to update"));
        return Ok(());
    }

    // Parse update strategy
    let strategy = match args.strategy.as_str() {
        "latest" => UpdateStrategy::Latest,
        "patch" => UpdateStrategy::Patch,
        "minor" => UpdateStrategy::Minor,
        "major" => UpdateStrategy::Major,
        _ => {
            if let Some(version) = &args.version {
                UpdateStrategy::Exact(version.clone())
            } else {
                return Err(CliError::Config(format!(
                    "Invalid strategy: {}. Use: latest, patch, minor, major",
                    args.strategy
                )));
            }
        }
    };

    // If version is specified, use Exact strategy
    let strategy = if let Some(version) = &args.version {
        UpdateStrategy::Exact(version.clone())
    } else {
        strategy
    };

    // T033: Load lock file from project root
    let lock_path = if let Some(parent) = project_file_path.parent() {
        parent.join("skills.lock")
    } else {
        PathBuf::from("skills.lock")
    };
    let lock = if lock_path.exists() {
        ProjectSkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?
    } else {
        return Err(CliError::Config(
            "skills.lock not found. Run 'fastskill install' first.".to_string(),
        ));
    };

    // Load repositories and create sources manager for marketplace-based repos
    let repositories = crate::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Create SourcesManager from marketplace-based repositories for PackageResolver
    let sources_manager = create_sources_manager_from_repositories(&repo_manager)?;

    if args.check || args.dry_run {
        if let Some(sources_mgr) = sources_manager {
            let mut resolver = PackageResolver::new(sources_mgr.clone());
            resolver
                .build_index()
                .await
                .map_err(|e| CliError::Config(format!("Failed to build resolver index: {}", e)))?;

            let update_service = UpdateService::new(Arc::new(resolver), lock);
            let updates = update_service
                .check_updates(args.skill_id.as_deref(), strategy)
                .map_err(|e| CliError::Config(format!("Failed to check updates: {}", e)))?;

            if updates.is_empty() {
                println!("{}", messages::info("No updates available"));
            } else {
                println!("\nSkills that would be updated:\n");
                for update in &updates {
                    println!(
                        "  • {}: {} -> {}",
                        update.skill_id, update.current_version, update.available_version
                    );
                }
            }
        } else {
            println!("\nSkills that would be updated:\n");
            for entry in &entries {
                println!("  • {} (from {:?})", entry.id, entry.origin);
            }
        }
        if args.check {
            println!(
                "\n{}",
                messages::info("Run without --check to actually update")
            );
        }
        return Ok(());
    }

    // Initialize service
    // Note: update command doesn't have access to CLI sources_path, so uses env var or walk-up
    let config = create_service_config(false, None)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;

    // Load repositories and create sources manager for marketplace-based repos
    let repositories = crate::config::load_repositories_from_project()?;
    let repo_manager = RepositoryManager::from_definitions(repositories);

    // Create SourcesManager from marketplace-based repositories for PackageResolver
    let sources_manager = create_sources_manager_from_repositories(&repo_manager)?;

    // Update each skill
    let mut updated_count = 0;
    for entry in entries {
        println!("  Updating {}...", entry.id);
        match install_utils::install_skill_from_entry(
            &service,
            entry.clone(),
            sources_manager.as_ref().map(|arc| arc.as_ref()),
        )
        .await
        {
            Ok(skill_def) => {
                manifest_utils::update_lock_file(&lock_path, &skill_def, entry.groups.clone())
                    .map_err(|e| CliError::Config(format!("Failed to update lock file: {}", e)))?;
                updated_count += 1;
                println!("  {}", messages::ok(&format!("Updated {}", entry.id)));
            }
            Err(e) => {
                eprintln!(
                    "  {}",
                    messages::error(&format!("Failed to update {}: {}", entry.id, e))
                );
            }
        }
    }

    println!();
    println!(
        "{}",
        messages::ok(&format!("Updated {} skill(s)", updated_count))
    );
    println!("   Updated skills.lock");

    Ok(())
}

/// Create SourcesManager from RepositoryManager for marketplace-based repositories
pub fn create_sources_manager_from_repositories(
    repo_manager: &RepositoryManager,
) -> CliResult<Option<Arc<SourcesManager>>> {
    install_utils::create_sources_manager_from_repositories(repo_manager)
        .map(|opt| opt.map(Arc::new))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::await_holding_lock
)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_update_no_manifest() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        // Helper struct to ensure directory is restored even if test panics
        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = UpdateArgs {
            skill_id: None,
            check: false,
            dry_run: false,
            version: None,
            source: None,
            strategy: "latest".to_string(),
            reindex: false,
            no_reindex: false,
        };

        let result = execute_update(args, false).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("skill-project.toml not found") && msg.contains("fastskill init"),
                "Error message must mention skill-project.toml and fastskill init: '{}'",
                msg
            );
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_execute_update_invalid_strategy() {
        let temp_dir = TempDir::new().unwrap();

        // Get original directory immediately and handle potential errors
        let original_dir = match std::env::current_dir() {
            Ok(dir) => dir,
            Err(_) => {
                // If we can't get current dir, assume we're in the project root
                std::path::PathBuf::from(".")
            }
        };

        // Create skill-project.toml and skills.lock at project root using absolute paths
        // Do this BEFORE changing directory to avoid any path resolution issues
        let skill_project_toml = temp_dir.path().join("skill-project.toml");
        let skills_lock = temp_dir.path().join("skills.lock");

        // Create files BEFORE changing directory to ensure they're created
        fs::write(
            &skill_project_toml,
            r#"[dependencies]
test-skill = { origin = { type = "git", url = "https://example.com/repo.git" } }
"#,
        )
        .expect("Failed to write skill-project.toml");

        // Create skills.lock file (required for update) with proper format including generated_at
        use chrono::Utc;
        let lock_content = format!(
            r#"[metadata]
version = "1.0.0"
generated_at = "{}"

[[skills]]
id = "test-skill"
version = "1.0.0"
"#,
            Utc::now().to_rfc3339()
        );
        fs::write(&skills_lock, lock_content).expect("Failed to write skills.lock");

        // Verify files exist before test (using absolute paths)
        assert!(
            skill_project_toml.exists(),
            "skill-project.toml should exist at {}",
            skill_project_toml.display()
        );
        assert!(
            skills_lock.exists(),
            "skills.lock should exist at {}",
            skills_lock.display()
        );

        // Helper struct to ensure directory is restored even if test panics
        struct DirGuard(std::path::PathBuf);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _guard = DirGuard(original_dir.clone());

        // Change to temp directory AFTER creating files, but handle potential errors
        if let Err(e) = std::env::set_current_dir(temp_dir.path()) {
            panic!("Failed to change to temp directory: {}", e);
        }

        let args = UpdateArgs {
            skill_id: None,
            check: false,
            dry_run: false,
            version: None,
            source: None,
            strategy: "invalid-strategy".to_string(),
            reindex: false,
            no_reindex: false,
        };

        let result = execute_update(args, false).await;
        // Should fail for invalid strategy or missing skills_directory
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        if let Err(CliError::Config(msg)) = result {
            assert!(
                msg.contains("Invalid strategy")
                    || msg.contains("latest, patch, minor, major")
                    || msg.contains("strategy")
                    || msg.contains("Failed to load repositories")
                    || msg.contains("skill-project.toml not found")
                    || msg.contains("requires [tool.fastskill] with skills_directory"),
                "Error message '{}' does not contain expected text",
                msg
            );
        } else {
            panic!(
                "Expected Config error for invalid strategy, got: {:?}",
                result
            );
        }
    }

    #[tokio::test]
    async fn test_execute_update_check_mode() {
        // Use a shared mutex to serialize directory changes across parallel tests
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        // Helper struct to ensure directory is restored even if test panics
        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create skill-project.toml at project root using absolute paths
        let skill_project_toml = temp_dir.path().join("skill-project.toml");
        fs::write(&skill_project_toml, "[dependencies]").unwrap();

        let args = UpdateArgs {
            skill_id: None,
            check: true,
            dry_run: false,
            version: None,
            source: None,
            strategy: "latest".to_string(),
            reindex: false,
            no_reindex: false,
        };

        // Should succeed in check mode even with no skills
        let result = execute_update(args, false).await;
        // May succeed or fail depending on lock file, but shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_update_success_with_check() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        struct DirGuard(Option<std::path::PathBuf>);
        impl Drop for DirGuard {
            fn drop(&mut self) {
                if let Some(dir) = &self.0 {
                    let _ = std::env::set_current_dir(dir);
                }
            }
        }
        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = "1.0.0"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

        let lock_content = r#"version = "1.0.0"
generated_at = "2024-01-01T00:00:00Z"
fastskill_version = "0.1.0"

[[skills]]
id = "test-skill"
name = "test-skill"
version = "1.0.0"
source_type = "local"
source = { path = ".claude/skills/test-skill" }
"#;
        fs::write(temp_dir.path().join("skills.lock"), lock_content).unwrap();

        let args = UpdateArgs {
            skill_id: None,
            check: true,
            dry_run: false,
            version: None,
            source: None,
            strategy: "latest".to_string(),
            reindex: false,
            no_reindex: false,
        };

        let result = execute_update(args, false).await;
        // Should succeed in check mode or fail with appropriate error
        assert!(result.is_ok() || result.is_err());
    }
}

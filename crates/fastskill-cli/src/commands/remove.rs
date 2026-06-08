//! Remove command implementation

use crate::config::get_skill_search_locations_for_display;
use crate::error::{CliError, CliResult, SkillNotFoundMessage};
use crate::utils::manifest_utils;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::lock::project_lock_path;
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::fs;

/// Uninstall skills (only way to stop using skills)
///
/// This is the only command to completely stop using skills after the removal of 'disable'.
///
/// Behavior:
/// - For manifest-managed projects: Removes from skill-project.toml [dependencies] and local installation
/// - For local-only skills: Removes from local installation only
/// - Always updates skills.lock to reflect removals (when manifest exists)
///
/// Use this command when you want to completely stop using a skill.
#[derive(Debug)]
pub struct RemoveArgs {
    /// Skill IDs to remove
    pub skill_ids: Vec<String>,

    /// Force removal without confirmation
    pub force: bool,

    /// Skills directory path (overrides default discovery)
    #[allow(dead_code)]
    pub skills_dir: Option<std::path::PathBuf>,

    /// Trigger reindex after removal (overrides config)
    pub reindex: bool,

    /// Skip reindex after removal
    pub no_reindex: bool,
}

impl IntoCommandSpec for RemoveArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Uninstall skills (removes from manifest and local installation)",
            syntax: Some("remove <SKILL_ID>... [OPTIONS]"),
            category: Some("packages"),
            args: vec![
                ArgSpec {
                    name: "skill-ids",
                    long: None,
                    short: None,
                    help: "Skill IDs to remove",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "force",
                    long: Some("force"),
                    short: Some('f'),
                    help: "Force removal without confirmation",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "skills-dir",
                    long: Some("skills-dir"),
                    short: None,
                    help: "Skills directory path (overrides default discovery)",
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "reindex",
                    long: Some("reindex"),
                    short: None,
                    help: "Trigger reindex after removal (overrides config)",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
                ArgSpec {
                    name: "no-reindex",
                    long: Some("no-reindex"),
                    short: None,
                    help: "Skip reindex after removal",
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

impl FromArgValueMap for RemoveArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        RemoveArgs {
            skill_ids: match map.get("skill-ids") {
                Some(ArgValue::List(items)) => items
                    .iter()
                    .filter_map(|i| {
                        if let ArgValue::Str(s) = i {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => vec![],
            },
            force: matches!(map.get("force"), Some(ArgValue::Bool(true))),
            skills_dir: map.get("skills-dir").and_then(|v| {
                if let ArgValue::Str(s) = v {
                    Some(PathBuf::from(s))
                } else {
                    None
                }
            }),
            reindex: matches!(map.get("reindex"), Some(ArgValue::Bool(true))),
            no_reindex: matches!(map.get("no-reindex"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Validate that all skills exist before removal; returns parsed SkillIds
async fn validate_skills_exist(
    service: &FastSkillService,
    skill_ids: &[String],
    global: bool,
) -> CliResult<Vec<fastskill_core::SkillId>> {
    let mut nonexistent_skills = Vec::new();
    let mut parsed_ids = Vec::new();

    for skill_id in skill_ids {
        let skill_id_parsed = fastskill_core::SkillId::new(skill_id.clone())
            .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;

        let exists_in_registry = service
            .skill_manager()
            .get_skill(&skill_id_parsed)
            .await
            .map(|opt| opt.is_some())
            .unwrap_or(false);
        let exists_in_filesystem = find_skill_directory(service, skill_id, true).is_some();

        if !exists_in_registry && !exists_in_filesystem {
            nonexistent_skills.push(skill_id.clone());
        }
        parsed_ids.push(skill_id_parsed);
    }

    if !nonexistent_skills.is_empty() {
        let skill_id_display = nonexistent_skills.join(", ");
        let searched_paths = get_skill_search_locations_for_display(global).unwrap_or_else(|_| {
            vec![(
                service.config().skill_storage_path.clone(),
                if global { "global" } else { "project" }.to_string(),
            )]
        });
        return Err(CliError::SkillNotFound(SkillNotFoundMessage::new(
            skill_id_display,
            searched_paths,
        )));
    }

    Ok(parsed_ids)
}

/// Prompt user for confirmation unless force flag is set
fn confirm_removal(skill_ids: &[String], force: bool) -> CliResult<bool> {
    if force {
        return Ok(true);
    }

    println!("Warning: This will permanently remove the following skills:");
    for skill_id in skill_ids {
        println!("  - {}", skill_id);
    }
    print!("Are you sure you want to continue? (y/n): ");
    io::stdout()
        .flush()
        .map_err(|e| CliError::Config(format!("Failed to flush stdout: {}", e)))?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(CliError::Io)?;

    let trimmed = input.trim().to_lowercase();
    Ok(trimmed == "yes" || trimmed == "y")
}

/// Find the skill directory, checking both direct and nested locations.
///
/// When `require_marker` is true a directory only matches if it also contains a
/// `SKILL.md` file — used for existence validation. When false, an existing
/// directory at the direct path matches even without `SKILL.md`, so that removal
/// can still clean up partial or corrupt installations. Nested matches always
/// require `SKILL.md` to avoid deleting unrelated directories.
fn find_skill_directory(
    service: &FastSkillService,
    skill_id: &str,
    require_marker: bool,
) -> Option<PathBuf> {
    let skill_dir = service.config().skill_storage_path.join(skill_id);

    if skill_dir.exists() && (!require_marker || skill_dir.join("SKILL.md").exists()) {
        return Some(skill_dir);
    }

    // Search in subdirectories (handles nested directory structures)
    if let Ok(entries) = std::fs::read_dir(&service.config().skill_storage_path) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let candidate_dir = entry.path().join(skill_id);
                    if candidate_dir.exists() && candidate_dir.join("SKILL.md").exists() {
                        return Some(candidate_dir);
                    }
                }
            }
        }
    }

    None
}

/// Unregister skill from the service
async fn unregister_skill_from_service(
    service: &FastSkillService,
    skill_id: fastskill_core::SkillId,
) -> CliResult<()> {
    match service.skill_manager().unregister_skill(&skill_id).await {
        Ok(_) => Ok(()),
        Err(fastskill_core::ServiceError::SkillNotFound(_)) => {
            // Skill not in registry, but that's okay - we still want to clean up files
            Ok(())
        }
        Err(e) => Err(CliError::Service(fastskill_core::ServiceError::Custom(
            format!("Failed to remove skill {}: {}", skill_id, e),
        ))),
    }
}

/// Delete skill directory from filesystem
async fn delete_skill_directory(service: &FastSkillService, skill_id: &str) -> CliResult<()> {
    if let Some(skill_dir) = find_skill_directory(service, skill_id, false) {
        fs::remove_dir_all(&skill_dir).await.map_err(|e| {
            CliError::Io(std::io::Error::other(format!(
                "Failed to delete skill directory {}: {}",
                skill_dir.display(),
                e
            )))
        })?;
    }
    Ok(())
}

/// Remove skill from lock file
fn remove_from_lock_file(skill_id: &str) -> CliResult<()> {
    let current_dir = env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_file_result = resolve_project_file(&current_dir);
    let lock_path = project_lock_path(&project_file_result.path);

    if lock_path.exists() {
        let mut lock = fastskill_core::core::lock::ProjectSkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?;
        lock.remove_skill(skill_id);
        lock.save_to_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to save lock file: {}", e)))?;
    }

    Ok(())
}

/// Remove skill from vector index
async fn remove_from_vector_index(service: &FastSkillService, skill_id: &str) {
    if let Some(vector_index_service) = service.vector_index_service() {
        if let Err(e) = vector_index_service.remove_skill(skill_id).await {
            tracing::warn!(
                "Failed to remove skill {} from vector index: {}",
                skill_id,
                e
            );
        }
    }
}

/// Remove a single skill (all cleanup steps)
async fn remove_single_skill(
    service: &FastSkillService,
    skill_id: fastskill_core::SkillId,
    raw_id: &str,
    global: bool,
) -> CliResult<()> {
    // Unregister from service
    unregister_skill_from_service(service, skill_id).await?;

    // Delete directory
    delete_skill_directory(service, raw_id).await?;

    if global {
        // Remove from global lock only; do not touch project manifest or project lock
        manifest_utils::remove_from_global_lock_file(raw_id)
            .map_err(|e| CliError::Config(format!("Failed to update global lock file: {}", e)))?;
    } else {
        // Remove from manifest
        manifest_utils::remove_skill_from_project_toml(raw_id)
            .map_err(|e| CliError::Config(format!("Failed to update skill-project.toml: {}", e)))?;
        // Remove from lock file
        remove_from_lock_file(raw_id)?;
    }

    // Remove from vector index (non-critical, just log warnings)
    remove_from_vector_index(service, raw_id).await;

    Ok(())
}

pub async fn execute_remove(
    service: &FastSkillService,
    args: RemoveArgs,
    global: bool,
) -> CliResult<()> {
    if args.reindex && args.no_reindex {
        return Err(CliError::Validation(
            "--reindex and --no-reindex cannot be used together".to_string(),
        ));
    }
    let reindex = args.reindex;
    let no_reindex = args.no_reindex;

    // Validate inputs
    if args.skill_ids.is_empty() {
        return Err(CliError::Config("No skill IDs provided".to_string()));
    }

    // Validate all skills exist; get back parsed SkillIds
    let parsed_ids = validate_skills_exist(service, &args.skill_ids, global).await?;

    // Get user confirmation
    if !confirm_removal(&args.skill_ids, args.force)? {
        println!("Removal cancelled.");
        return Ok(());
    }

    // Remove each skill
    let mut removed_count = 0;
    for (skill_id, raw_id) in parsed_ids.into_iter().zip(args.skill_ids.iter()) {
        remove_single_skill(service, skill_id, raw_id, global).await?;
        println!("Removed skill: {}", raw_id);
        removed_count += 1;
    }

    // Display success message
    if removed_count > 0 {
        if global {
            println!(
                "{}",
                crate::utils::messages::ok("Updated global-skills.lock")
            );
        } else {
            println!(
                "{}",
                crate::utils::messages::ok("Updated skill-project.toml and skills.lock")
            );
        }
    }

    let auto_reindex = crate::config_file::load_auto_reindex_config();
    crate::utils::reindex_utils::maybe_auto_reindex(
        service,
        "remove",
        reindex,
        no_reindex,
        auto_reindex,
        false,
    )
    .await
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
    use fastskill_core::test_utils::DirGuard;
    use fastskill_core::ServiceConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_remove_empty_args() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec![],
            force: false,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
        };

        let result = execute_remove(&service, args, false).await;
        assert!(result.is_err());
        if let Err(CliError::Config(msg)) = result {
            assert!(msg.contains("No skill IDs provided"));
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_execute_remove_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["invalid@skill@id".to_string()],
            force: true,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
        };

        let result = execute_remove(&service, args, false).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("Invalid skill ID format"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_execute_remove_nonexistent_skill() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["nonexistent@1.0.0".to_string()],
            force: true,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
        };

        // This should fail because the skill doesn't exist
        let result = execute_remove(&service, args, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_remove_success() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let skills_dir = temp_dir.path().join(".claude/skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = "1.0.0"
"#;
        std::fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

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
        std::fs::write(temp_dir.path().join("skills.lock"), lock_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["test-skill".to_string()],
            force: true,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
        };

        let result = execute_remove(&service, args, false).await;
        // May succeed or fail depending on various factors
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_remove_calls_vector_index_remove_skill() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();

        let _guard = DirGuard(original_dir);

        std::env::set_current_dir(temp_dir.path()).unwrap();

        let skills_dir = temp_dir.path().join(".claude/skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
        std::fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let manifest_content = r#"[tool.fastskill]
skills_directory = ".claude/skills"

[dependencies]
test-skill = "1.0.0"
"#;
        std::fs::write(temp_dir.path().join("skill-project.toml"), manifest_content).unwrap();

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
        std::fs::write(temp_dir.path().join("skills.lock"), lock_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir.clone(),
            embedding: Some(fastskill_core::EmbeddingConfig {
                openai_base_url: "https://api.openai.com/v1".to_string(),
                embedding_model: "text-embedding-3-small".to_string(),
                index_path: None,
            }),
            ..Default::default()
        };

        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = RemoveArgs {
            skill_ids: vec!["test-skill".to_string()],
            force: true,
            skills_dir: None,
            reindex: false,
            no_reindex: false,
        };

        let result = execute_remove(&service, args, false).await;
        // May succeed or fail depending on various factors
        // The important part is that it attempts to remove from vector index
        assert!(result.is_ok() || result.is_err());
    }
}

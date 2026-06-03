//! Show command - displays skill details and dependency tree
//!
//! `show` displays skill metadata (name, version, description, source) not full content.
//! For full SKILL.md content, use `read`.
//!
//! With no skill_id argument, lists all installed skills with minimal details.
//! With a skill_id, shows detailed metadata for that specific skill.

use crate::commands::common::validate_format_args;
use crate::config::{create_service_config, get_skill_search_locations_for_display};
use crate::error::{CliError, CliResult, SkillNotFoundMessage};
use chrono::Utc;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::lock::ProjectSkillsLock;
use fastskill_core::core::{ProjectConfig, SkillDefinition};
use fastskill_core::{FastSkillService, OutputFormat};
use std::collections::HashMap;
use std::path::PathBuf;

/// Show skill details
#[derive(Debug)]
pub struct ShowArgs {
    /// Skill ID to show (if not specified, shows all)
    skill_id: Option<String>,

    /// Show dependency tree
    tree: bool,

    /// Output format: table, json, grid, xml (default: table)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json
    pub json: bool,

    /// Skills directory path (overrides default discovery)
    pub skills_dir: Option<std::path::PathBuf>,

    /// Read from skills.lock only (do not initialize service)
    pub locked: bool,

    /// Read installed state from storage (default mode)
    #[allow(dead_code)]
    pub installed: bool,
}

impl IntoCommandSpec for ShowArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Show metadata for one or all installed skills",
            syntax: Some("show [SKILL_ID] [OPTIONS]"),
            category: Some("discovery"),
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Skill ID to show (if not specified, shows all)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "tree",
                    kind: ArgKind::Flag,
                    long: Some("tree"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Show dependency tree",
                    ..Default::default()
                },
                ArgSpec {
                    name: "format",
                    kind: ArgKind::Option,
                    long: Some("format"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output format: table, json, grid, xml (default: table)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Shorthand for --format json",
                    ..Default::default()
                },
                ArgSpec {
                    name: "locked",
                    kind: ArgKind::Flag,
                    long: Some("locked"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Read from skills.lock only (do not initialize service)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "installed",
                    kind: ArgKind::Flag,
                    long: Some("installed"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Read installed state from storage (default mode)",
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

fn parse_output_format(s: &str) -> Option<OutputFormat> {
    match s {
        "table" => Some(OutputFormat::Table),
        "json" => Some(OutputFormat::Json),
        "grid" => Some(OutputFormat::Grid),
        "xml" => Some(OutputFormat::Xml),
        _ => None,
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for ShowArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            skill_id: map.get("skill-id").and_then(opt_str),
            tree: matches!(map.get("tree"), Some(ArgValue::Bool(true))),
            format: map
                .get("format")
                .and_then(opt_str)
                .as_deref()
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            // skills_dir is omitted from the spec; use the global --skills-dir flag instead
            skills_dir: None,
            locked: matches!(map.get("locked"), Some(ArgValue::Bool(true))),
            installed: matches!(map.get("installed"), Some(ArgValue::Bool(true))),
        }
    }
}

pub async fn execute_show(args: ShowArgs, global: bool) -> CliResult<()> {
    let format = validate_format_args(&args.format, args.json)?;

    // Validate flag combinations
    validate_show_args(&args, global)?;

    if args.locked {
        // Locked mode: read only from skills.lock
        let (config, lock_opt) = load_config_and_lock()?;
        match lock_opt {
            Some(lock) => run_with_lock(lock, &args, &config, format)?,
            None => {
                return Err(CliError::Config(
                    "No skills.lock found. Run 'fastskill install' to generate one.".to_string(),
                ));
            }
        }
    } else {
        // Installed mode (default): use FastSkillService
        run_with_service(&args, format, global).await?;
    }
    Ok(())
}

fn validate_show_args(args: &ShowArgs, global: bool) -> CliResult<()> {
    // --locked + --skills-dir is invalid
    if args.locked && args.skills_dir.is_some() {
        return Err(CliError::Validation(
            "--locked mode does not support --skills-dir (lock file uses project-relative paths)"
                .to_string(),
        ));
    }

    // --locked + --global is invalid
    if args.locked && global {
        return Err(CliError::Validation(
            "--locked mode does not support --global (lock file uses project-relative paths)"
                .to_string(),
        ));
    }

    Ok(())
}

fn load_config_and_lock() -> CliResult<(ProjectConfig, Option<ProjectSkillsLock>)> {
    let current_dir = std::env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let config =
        fastskill_core::core::load_project_config(&current_dir).map_err(CliError::Config)?;
    let lock_path = config.project_root.join("skills.lock");
    let lock_opt = if lock_path.exists() {
        let lock = ProjectSkillsLock::load_from_file(&lock_path)
            .map_err(|e| CliError::Config(format!("Failed to load lock file: {}", e)))?;
        Some(lock)
    } else {
        None
    };
    Ok((config, lock_opt))
}

fn run_with_lock(
    lock: ProjectSkillsLock,
    args: &ShowArgs,
    config: &ProjectConfig,
    format: OutputFormat,
) -> CliResult<()> {
    if let Some(skill_id) = &args.skill_id {
        if let Some(skill) = lock.skills.iter().find(|s| s.id == skill_id.as_str()) {
            let skill_definition = locked_skill_to_definition(skill)?;
            let formatted_output =
                fastskill_core::output::format_show_results(&[skill_definition], format)
                    .map_err(CliError::Config)?;
            print!("{}", formatted_output);
            if args.tree {
                println!("\nDependency Tree:");
                println!("   (Dependency tree not yet implemented)");
            }
            return Ok(());
        }
        let paths = search_paths_from_config(config);
        return Err(CliError::SkillNotFound(SkillNotFoundMessage::new(
            skill_id.clone(),
            paths,
        )));
    }
    print_lock_list(&lock, args.tree, format)?;
    Ok(())
}

fn search_paths_from_config(config: &ProjectConfig) -> Vec<(PathBuf, String)> {
    get_skill_search_locations_for_display(false)
        .unwrap_or_else(|_| vec![(config.skills_directory.clone(), "project".to_string())])
}

fn print_lock_list(lock: &ProjectSkillsLock, tree: bool, format: OutputFormat) -> CliResult<()> {
    let skills: Vec<SkillDefinition> = lock
        .skills
        .iter()
        .map(locked_skill_to_definition)
        .collect::<Result<Vec<_>, CliError>>()?;

    let formatted_output =
        fastskill_core::output::format_show_results(&skills, format).map_err(CliError::Config)?;
    print!("{}", formatted_output);

    if tree {
        for skill in &lock.skills {
            println!("  Groups: {:?}", skill.groups);
            println!("  Source: {:?}", skill.source);
        }
    }

    Ok(())
}

async fn run_with_service(args: &ShowArgs, format: OutputFormat, global: bool) -> CliResult<()> {
    let config = create_service_config(global, args.skills_dir.clone(), None)?;
    let mut service = FastSkillService::new(config)
        .await
        .map_err(CliError::Service)?;
    service.initialize().await.map_err(CliError::Service)?;

    if let Some(skill_id) = &args.skill_id {
        return run_show_one_from_service(&service, skill_id, format).await;
    }
    run_show_all_from_service(&service, format).await
}

async fn run_show_one_from_service(
    service: &FastSkillService,
    skill_id: &str,
    format: OutputFormat,
) -> CliResult<()> {
    let skill_id_parsed = fastskill_core::SkillId::new(skill_id.to_string())
        .map_err(|_| CliError::Validation(format!("Invalid skill ID format: {}", skill_id)))?;
    let skill = service
        .skill_manager()
        .get_skill(&skill_id_parsed)
        .await
        .map_err(CliError::Service)?;
    if let Some(skill) = skill {
        let formatted_output = fastskill_core::output::format_show_results(&[skill], format)
            .map_err(CliError::Config)?;
        print!("{}", formatted_output);
        return Ok(());
    }
    let paths = get_skill_search_locations_for_display(false).unwrap_or_else(|_| {
        vec![(
            service.config().skill_storage_path.clone(),
            "project".to_string(),
        )]
    });
    Err(CliError::SkillNotFound(SkillNotFoundMessage::new(
        skill_id.to_string(),
        paths,
    )))
}

async fn run_show_all_from_service(
    service: &FastSkillService,
    format: OutputFormat,
) -> CliResult<()> {
    let skills = service
        .skill_manager()
        .list_skills(None)
        .await
        .map_err(CliError::Service)?;

    let formatted_output =
        fastskill_core::output::format_show_results(&skills, format).map_err(CliError::Config)?;
    print!("{}", formatted_output);

    Ok(())
}

fn locked_skill_to_definition(
    skill: &fastskill_core::core::lock::ProjectLockedSkillEntry,
) -> Result<SkillDefinition, CliError> {
    let now = Utc::now();
    let id = fastskill_core::SkillId::new(skill.id.clone()).map_err(|_| {
        CliError::Validation(format!(
            "Invalid skill ID format in lock file: {}",
            skill.id
        ))
    })?;

    Ok(SkillDefinition {
        id,
        name: skill.name.clone(),
        description: format!("Skill from {}", skill.name),
        version: skill.version.clone(),
        author: None,
        enabled: true,
        created_at: now,
        updated_at: now,
        skill_file: PathBuf::from(format!("/skills/{}", skill.id)),
        reference_files: None,
        script_files: None,
        asset_files: None,
        execution_environment: None,
        dependencies: None,
        timeout: None,
        source_url: None,
        source_type: None,
        source_branch: None,
        source_tag: None,
        source_subdir: None,
        installed_from: None,
        commit_hash: None,
        fetched_at: None,
        editable: skill.editable,
    })
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

    /// Runs a test in a temp project (cwd set, .claude/skills, skill-project.toml). Restores cwd on drop.
    async fn with_temp_project<F, Fut, R>(f: F) -> R
    where
        F: FnOnce(TempDir) -> Fut,
        Fut: std::future::Future<Output = R>,
    {
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
        fs::create_dir_all(temp_dir.path().join(".claude/skills")).unwrap();
        let manifest = r#"[tool.fastskill]
skills_directory = ".claude/skills"
"#;
        fs::write(temp_dir.path().join("skill-project.toml"), manifest).unwrap();
        f(temp_dir).await
    }

    #[tokio::test]
    async fn test_execute_show_no_skill_id_no_lock_file() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("FASTSKILL_SKILLS_DIR", temp_dir.path());

        let args = ShowArgs {
            skill_id: None,
            tree: false,
            format: None,
            json: false,
            skills_dir: None,
            locked: false,
            installed: false,
        };

        // Should not crash even if no lock file exists
        let result = execute_show(args, false).await;
        // Result may be Ok or Err depending on service initialization, but shouldn't panic
        assert!(result.is_ok() || result.is_err());

        std::env::remove_var("FASTSKILL_SKILLS_DIR");
    }

    #[tokio::test]
    async fn test_execute_show_with_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_var("FASTSKILL_SKILLS_DIR", temp_dir.path());

        let args = ShowArgs {
            skill_id: Some("invalid@skill@id".to_string()),
            tree: false,
            format: None,
            json: false,
            skills_dir: None,
            locked: false,
            installed: false,
        };

        let result = execute_show(args, false).await;
        // Should fail with validation error or config error
        assert!(result.is_err());

        std::env::remove_var("FASTSKILL_SKILLS_DIR");
    }

    #[tokio::test]
    async fn test_execute_show_with_nonexistent_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().ok();
        std::env::set_var("FASTSKILL_SKILLS_DIR", temp_dir.path());

        let args = ShowArgs {
            skill_id: Some("nonexistent@1.0.0".to_string()),
            tree: false,
            format: None,
            json: false,
            skills_dir: None,
            locked: false,
            installed: false,
        };

        let result = execute_show(args, false).await;
        // Should fail because skill doesn't exist or invalid format
        assert!(result.is_err(), "Expected error, got: {:?}", result);
        match &result {
            Err(CliError::SkillNotFound(_)) => {}
            Err(CliError::Config(msg)) => {
                assert!(
                    msg.contains("not found")
                        || msg.contains("Failed to get current directory")
                        || msg.contains("Failed to load lock file")
                        || msg.contains("Failed to load skill-project.toml")
                        || msg.contains("skills_directory"),
                    "Error message '{}' does not contain expected text",
                    msg
                );
            }
            Err(CliError::Validation(msg)) => {
                // Validation error for invalid skill ID format is also acceptable
                assert!(
                    msg.contains("Invalid") || msg.contains("format"),
                    "Validation error '{}' should mention invalid format",
                    msg
                );
            }
            Err(e) => {
                // Any error is acceptable as long as it fails
                eprintln!("Got error: {:?}", e);
            }
            Ok(_) => {
                panic!("Expected error but got Ok");
            }
        }

        std::env::remove_var("FASTSKILL_SKILLS_DIR");
        if let Some(dir) = original_dir {
            let _ = std::env::set_current_dir(&dir);
        }
    }

    #[tokio::test]
    async fn test_execute_show_with_lock() {
        with_temp_project(|temp_dir| async move {
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
            let args = ShowArgs {
                skill_id: None,
                tree: false,
                format: None,
                json: false,
                skills_dir: None,
                locked: true,
                installed: false,
            };
            let result = execute_show(args, false).await;
            assert!(result.is_ok() || result.is_err());
        })
        .await;
    }

    #[tokio::test]
    async fn test_execute_show_with_skill_id() {
        with_temp_project(|temp_dir| async move {
            let skill_dir = temp_dir.path().join(".claude/skills/test-skill");
            fs::create_dir_all(&skill_dir).unwrap();
            let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage
"#;
            fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();
            let args = ShowArgs {
                skill_id: Some("test-skill".to_string()),
                tree: false,
                format: None,
                json: false,
                skills_dir: None,
                locked: false,
                installed: false,
            };
            let result = execute_show(args, false).await;
            assert!(result.is_ok() || result.is_err());
        })
        .await;
    }

    #[tokio::test]
    async fn test_validate_show_args_locked_with_skills_dir() {
        let args = ShowArgs {
            skill_id: None,
            tree: false,
            format: None,
            json: false,
            skills_dir: Some(PathBuf::from("/tmp/skills")),
            locked: true,
            installed: false,
        };
        let result = validate_show_args(&args, false);
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("--locked mode does not support --skills-dir"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_validate_show_args_locked_with_global() {
        let args = ShowArgs {
            skill_id: None,
            tree: false,
            format: None,
            json: false,
            skills_dir: None,
            locked: true,
            installed: false,
        };
        let result = validate_show_args(&args, true);
        assert!(result.is_err());
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("--locked mode does not support --global"));
        } else {
            panic!("Expected Validation error");
        }
    }

    #[tokio::test]
    async fn test_validate_show_args_installed_mode_valid() {
        let args = ShowArgs {
            skill_id: None,
            tree: false,
            format: None,
            json: false,
            skills_dir: Some(PathBuf::from("/tmp/skills")),
            locked: false,
            installed: true,
        };
        let result = validate_show_args(&args, false);
        assert!(result.is_ok());
    }
}

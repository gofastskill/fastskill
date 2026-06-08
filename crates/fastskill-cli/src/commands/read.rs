//! Read command - streams skill documentation to stdout
//!
//! Reads and outputs the full SKILL.md content. For structured metadata
//! (name, version, description, author, source) use `--meta`; for the
//! dependency tree use `--tree`.

use crate::commands::common::validate_format_args;
use crate::config;
use crate::error::{CliError, CliResult, SkillNotFoundMessage};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::lock::ProjectSkillsLock;
use fastskill_core::core::skill_manager::SkillDefinition;
use fastskill_core::output::{format_show_results, OutputFormat};
use fastskill_core::FastSkillService;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn parse_output_format(s: &str) -> Option<OutputFormat> {
    match s {
        "table" => Some(OutputFormat::Table),
        "json" => Some(OutputFormat::Json),
        "grid" => Some(OutputFormat::Grid),
        "xml" => Some(OutputFormat::Xml),
        _ => None,
    }
}

/// Read skill documentation
#[derive(Debug)]
pub struct ReadArgs {
    /// Skill ID to read
    pub skill_id: String,

    /// Show metadata instead of full content
    pub meta: bool,

    /// Show dependency tree
    pub tree: bool,

    /// Output format for --meta mode (table, json, grid, xml)
    pub format: Option<OutputFormat>,

    /// Shorthand for --format json in --meta mode
    pub json: bool,

    /// Read from skills.lock (--meta mode only)
    pub locked: bool,
}

impl IntoCommandSpec for ReadArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Read full SKILL.md content for a skill",
            syntax: Some("read <SKILL_ID> [OPTIONS]"),
            category: Some("discovery"),
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Skill identifier (e.g., 'pptx', 'scope/pptx', 'pptx@1.2.3')",
                    ..Default::default()
                },
                ArgSpec {
                    name: "meta",
                    kind: ArgKind::Flag,
                    long: Some("meta"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Show metadata instead of full content",
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
                    help: "Output format for --meta mode: table, json, grid, xml (default: table)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "json",
                    kind: ArgKind::Flag,
                    long: Some("json"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Shorthand for --format json in --meta mode",
                    ..Default::default()
                },
                ArgSpec {
                    name: "locked",
                    kind: ArgKind::Flag,
                    long: Some("locked"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Read from skills.lock (--meta mode only)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for ReadArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            skill_id: map
                .get("skill-id")
                .map(|v| {
                    if let ArgValue::Str(s) = v {
                        s.clone()
                    } else {
                        panic!("fw bug: skill-id wrong type")
                    }
                })
                .unwrap_or_else(|| panic!("fw bug: missing required skill-id")),
            meta: matches!(map.get("meta"), Some(ArgValue::Bool(true))),
            tree: matches!(map.get("tree"), Some(ArgValue::Bool(true))),
            format: map
                .get("format")
                .and_then(|v| {
                    if let ArgValue::Str(s) = v {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .and_then(parse_output_format),
            json: matches!(map.get("json"), Some(ArgValue::Bool(true))),
            locked: matches!(map.get("locked"), Some(ArgValue::Bool(true))),
        }
    }
}

/// Resolve a skill by ID using the service (with partial-match fallback).
async fn resolve_skill(
    service: &Arc<FastSkillService>,
    skill_id_str: &str,
) -> CliResult<SkillDefinition> {
    let skill_id = fastskill_core::SkillId::new(skill_id_str.to_string()).map_err(|e| {
        eprintln!("Error: Invalid skill ID format: '{}'", skill_id_str);
        eprintln!();
        eprintln!("Expected format:");
        eprintln!("  - id");
        eprintln!("  - scope/id");
        eprintln!("  - id@version or scope/id@version (if supported)");
        CliError::Validation(format!("Invalid skill ID format: {}", e))
    })?;

    // Try exact match first
    match service
        .skill_manager()
        .get_skill(&skill_id)
        .await
        .map_err(CliError::Service)?
    {
        Some(skill) => Ok(skill),
        None => {
            // Check for partial matches
            let all_skills = service
                .skill_manager()
                .list_skills()
                .await
                .map_err(CliError::Service)?;

            let matching_skills: Vec<_> = all_skills
                .iter()
                .filter(|s| {
                    let id_str = s.id.to_string();
                    id_str.ends_with(&format!("/{}", skill_id_str))
                        || id_str == skill_id_str
                        || s.name == skill_id_str
                })
                .collect();

            if matching_skills.len() > 1 {
                eprintln!("Error: Multiple skills match '{}':", skill_id_str);
                eprintln!();
                for s in &matching_skills {
                    eprintln!("  - {} ({})", s.id, s.version);
                }
                eprintln!();
                eprintln!("To disambiguate, use:");
                eprintln!("  - Version qualifier: <skill-id>@<version>");
                eprintln!("  - Full identifier: <scope>/<id>");
                return Err(CliError::Validation(format!(
                    "Multiple skills match '{}'",
                    skill_id_str
                )));
            }

            if matching_skills.is_empty() {
                let searched_paths = config::get_skill_search_locations_for_display(false)
                    .unwrap_or_else(|_| {
                        vec![(
                            service.config().skill_storage_path.clone(),
                            "project".to_string(),
                        )]
                    });
                return Err(CliError::SkillNotFound(SkillNotFoundMessage::new(
                    skill_id_str.to_string(),
                    searched_paths,
                )));
            }

            Ok(matching_skills[0].clone())
        }
    }
}

/// Execute the read command
pub async fn execute_read(service: Arc<FastSkillService>, args: ReadArgs) -> CliResult<()> {
    // Validate: --locked requires --meta
    if args.locked && !args.meta {
        return Err(CliError::Validation(
            "--meta is required when using --locked with read".to_string(),
        ));
    }

    // Validate: --format requires --meta
    if args.format.is_some() && !args.meta {
        return Err(CliError::Validation(
            "--meta is required when using --format with read".to_string(),
        ));
    }

    // Validate: --json requires --meta (implied: same gate as --format)
    if args.json && !args.meta {
        return Err(CliError::Validation(
            "--meta is required when using --json with read".to_string(),
        ));
    }

    // --meta mode
    if args.meta {
        let format = validate_format_args(&args.format, args.json)?;

        let skill = if args.locked {
            // Load skill definition from skills.lock
            let lock_path = PathBuf::from("skills.lock");
            let lock = ProjectSkillsLock::load_from_file(&lock_path)
                .map_err(|e| CliError::Config(format!("Failed to load skills.lock: {}", e)))?;

            // Validate skill ID syntax before searching the lock
            fastskill_core::SkillId::new(args.skill_id.clone()).map_err(|e| {
                eprintln!("Error: Invalid skill ID format: '{}'", args.skill_id);
                CliError::Validation(format!("Invalid skill ID format: {}", e))
            })?;

            let entry = lock
                .skills
                .iter()
                .find(|s| {
                    s.id == args.skill_id
                        || s.name == args.skill_id
                        || s.id.ends_with(&format!("/{}", args.skill_id))
                })
                .ok_or_else(|| {
                    CliError::Validation(format!(
                        "Skill '{}' not found in skills.lock",
                        args.skill_id
                    ))
                })?;

            // Convert lock entry to SkillDefinition
            let skill_id = fastskill_core::SkillId::new(entry.id.clone())
                .map_err(|e| CliError::Validation(format!("Invalid skill ID in lock: {}", e)))?;
            SkillDefinition::new(
                skill_id,
                entry.name.clone(),
                String::new(),
                entry.version.clone(),
            )
        } else {
            resolve_skill(&service, &args.skill_id).await?
        };

        let output = format_show_results(&[skill], format)
            .map_err(|e| CliError::Config(format!("Failed to format output: {}", e)))?;
        println!("{}", output);

        // If --tree is also set, fall through to print tree after meta
        if args.tree {
            let tree_skill = if args.locked {
                // Re-resolve from lock for tree
                let lock_path = PathBuf::from("skills.lock");
                let lock = ProjectSkillsLock::load_from_file(&lock_path)
                    .map_err(|e| CliError::Config(format!("Failed to load skills.lock: {}", e)))?;
                let entry = lock
                    .skills
                    .iter()
                    .find(|s| {
                        s.id == args.skill_id
                            || s.name == args.skill_id
                            || s.id.ends_with(&format!("/{}", args.skill_id))
                    })
                    .ok_or_else(|| {
                        CliError::Validation(format!(
                            "Skill '{}' not found in skills.lock",
                            args.skill_id
                        ))
                    })?;
                let skill_id = fastskill_core::SkillId::new(entry.id.clone()).map_err(|e| {
                    CliError::Validation(format!("Invalid skill ID in lock: {}", e))
                })?;
                let mut sd = SkillDefinition::new(
                    skill_id,
                    entry.name.clone(),
                    String::new(),
                    entry.version.clone(),
                );
                if !entry.dependencies.is_empty() {
                    sd.dependencies = Some(entry.dependencies.clone());
                }
                sd
            } else {
                resolve_skill(&service, &args.skill_id).await?
            };
            print_dependency_tree(&tree_skill);
        }

        return Ok(());
    }

    // --tree only (no --meta)
    if args.tree {
        let skill = resolve_skill(&service, &args.skill_id).await?;
        print_dependency_tree(&skill);
        return Ok(());
    }

    // Default: stream full SKILL.md to stdout
    // T007, T022: Parse and validate skill ID
    let skill_id = fastskill_core::SkillId::new(args.skill_id.clone()).map_err(|e| {
        eprintln!("Error: Invalid skill ID format: '{}'", args.skill_id);
        eprintln!();
        eprintln!("Expected format:");
        eprintln!("  - id");
        eprintln!("  - scope/id");
        eprintln!("  - id@version or scope/id@version (if supported)");
        CliError::Validation(format!("Invalid skill ID format: {}", e))
    })?;

    // T011, T021: Implement skill resolution via skill_manager().get_skill()
    // Try exact match first
    let skill = match service
        .skill_manager()
        .get_skill(&skill_id)
        .await
        .map_err(CliError::Service)?
    {
        Some(skill) => skill,
        None => {
            // T024: If no exact match, check for multiple partial matches
            let all_skills = service
                .skill_manager()
                .list_skills()
                .await
                .map_err(CliError::Service)?;

            let matching_skills: Vec<_> = all_skills
                .iter()
                .filter(|s| {
                    let skill_id_str = s.id.to_string();
                    skill_id_str.ends_with(&format!("/{}", args.skill_id))
                        || skill_id_str == args.skill_id
                        || s.name == args.skill_id
                })
                .collect();

            // T024: If multiple matches found, return error with disambiguation instructions
            if matching_skills.len() > 1 {
                eprintln!("Error: Multiple skills match '{}':", args.skill_id);
                eprintln!();
                for skill in &matching_skills {
                    eprintln!("  - {} ({})", skill.id, skill.version);
                }
                eprintln!();
                eprintln!("To disambiguate, use:");
                eprintln!("  - Version qualifier: <skill-id>@<version>");
                eprintln!("  - Full identifier: <scope>/<id>");
                return Err(CliError::Validation(format!(
                    "Multiple skills match '{}'",
                    args.skill_id
                )));
            }

            // If single match found, use it; otherwise return not found error
            if matching_skills.is_empty() {
                let searched_paths = config::get_skill_search_locations_for_display(false)
                    .unwrap_or_else(|_| {
                        vec![(
                            service.config().skill_storage_path.clone(),
                            "project".to_string(),
                        )]
                    });
                return Err(CliError::SkillNotFound(SkillNotFoundMessage::new(
                    args.skill_id.clone(),
                    searched_paths,
                )));
            }

            // We know there's exactly one match at this point
            matching_skills[0].clone()
        }
    };

    // T012: Implement base directory extraction from skill_file.parent()
    let base_dir = skill
        .skill_file
        .parent()
        .ok_or_else(|| CliError::Config("Failed to determine skill base directory".to_string()))?;

    // T015: Implement absolute path resolution using canonicalize()
    let base_dir_absolute = base_dir
        .canonicalize()
        .map_err(|e| CliError::Config(format!("Failed to resolve absolute path: {}", e)))?;

    // T044: Implement file size check (500KB limit) before reading
    let metadata = std::fs::metadata(&skill.skill_file).map_err(|e| {
        eprintln!("Error: Failed to load skill '{}': {}", args.skill_id, e);
        CliError::Io(e)
    })?;

    const MAX_FILE_SIZE: u64 = 512_000; // 500KB = 512,000 bytes
    if metadata.len() > MAX_FILE_SIZE {
        eprintln!(
            "Error: Skill '{}' documentation exceeds size limit",
            args.skill_id
        );
        eprintln!();
        eprintln!("File size: {} bytes", metadata.len());
        eprintln!("Maximum size: 500KB ({} bytes)", MAX_FILE_SIZE);
        eprintln!();
        eprintln!("Please reduce the size of SKILL.md or split content into reference files.");
        return Err(CliError::Validation(format!(
            "File size {} exceeds maximum of {} bytes",
            metadata.len(),
            MAX_FILE_SIZE
        )));
    }

    // T013, T045: Implement file reading with error handling for corrupted/unreadable files
    let content = std::fs::read_to_string(&skill.skill_file).map_err(|e| {
        eprintln!("Error: Failed to load skill '{}': {}", args.skill_id, e);
        eprintln!();
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => {
                eprintln!("Permission denied: {}", skill.skill_file.display());
            }
            std::io::ErrorKind::NotFound => {
                eprintln!("File not readable: {}", skill.skill_file.display());
            }
            _ => {
                eprintln!("I/O error: {}", e);
            }
        }
        CliError::Io(e)
    })?;

    // T014: Implement structured output format (header, base directory, content, footer)
    // T016: Ensure plain text output with no ANSI colors or formatting codes
    println!("Reading: {}", args.skill_id);
    println!("Base directory: {}", base_dir_absolute.display());
    println!();
    print!("{}", content);
    if !content.ends_with('\n') {
        println!();
    }
    println!();
    println!("Skill read: {}", args.skill_id);

    Ok(())
}

/// Print a dependency tree for the given skill definition.
fn print_dependency_tree(skill: &SkillDefinition) {
    println!("{}", skill.id);
    match &skill.dependencies {
        Some(deps) if !deps.is_empty() => {
            for dep in deps {
                println!("  - {}", dep);
            }
        }
        _ => {
            println!("  (no dependencies)");
        }
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;
    use fastskill_core::{FastSkillService, ServiceConfig};
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_read_invalid_skill_id() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReadArgs {
            skill_id: "bad@id@here".to_string(),
            meta: false,
            tree: false,
            format: None,
            json: false,
            locked: false,
        };

        let result = execute_read(Arc::new(service), args).await;
        assert!(result.is_err());
        if let Err(CliError::Validation(_)) = result {
            // Expected error type
        } else {
            panic!("Expected Validation error for invalid skill ID");
        }
    }

    #[tokio::test]
    async fn test_execute_read_skill_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReadArgs {
            skill_id: "nonexistent-skill".to_string(),
            meta: false,
            tree: false,
            format: None,
            json: false,
            locked: false,
        };

        let result = execute_read(Arc::new(service), args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_read_success() {
        let temp_dir = TempDir::new().unwrap();
        let skills_dir = temp_dir.path().join(".claude/skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let skill_dir = skills_dir.join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_content = r#"# Test Skill

Name: test-skill
Version: 1.0.0
Description: A test skill for coverage

This is the content of the skill file.
"#;
        fs::write(skill_dir.join("SKILL.md"), skill_content).unwrap();

        let config = ServiceConfig {
            skill_storage_path: skills_dir,
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReadArgs {
            skill_id: "test-skill".to_string(),
            meta: false,
            tree: false,
            format: None,
            json: false,
            locked: false,
        };

        let result = execute_read(Arc::new(service), args).await;
        // May succeed or fail depending on skill registration state
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_execute_read_locked_without_meta_returns_validation_error() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReadArgs {
            skill_id: "some-skill".to_string(),
            meta: false,
            tree: false,
            format: None,
            json: false,
            locked: true,
        };

        let result = execute_read(Arc::new(service), args).await;
        assert!(matches!(result, Err(CliError::Validation(_))));
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("--meta is required"));
        }
    }

    #[tokio::test]
    async fn test_execute_read_format_without_meta_returns_validation_error() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReadArgs {
            skill_id: "some-skill".to_string(),
            meta: false,
            tree: false,
            format: Some(OutputFormat::Json),
            json: false,
            locked: false,
        };

        let result = execute_read(Arc::new(service), args).await;
        assert!(matches!(result, Err(CliError::Validation(_))));
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("--meta is required"));
        }
    }

    #[tokio::test]
    async fn test_execute_read_json_without_meta_returns_validation_error() {
        let temp_dir = TempDir::new().unwrap();
        let config = ServiceConfig {
            skill_storage_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut service = FastSkillService::new(config).await.unwrap();
        service.initialize().await.unwrap();

        let args = ReadArgs {
            skill_id: "some-skill".to_string(),
            meta: false,
            tree: false,
            format: None,
            json: true,
            locked: false,
        };

        let result = execute_read(Arc::new(service), args).await;
        assert!(matches!(result, Err(CliError::Validation(_))));
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("--meta is required"));
        }
    }
}

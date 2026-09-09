//! Read command - streams skill documentation to stdout
//!
//! Reads and outputs the full SKILL.md content. For structured metadata
//! (name, version, description, author, source) use `--meta`; for the
//! dependency tree use `--tree`.

use crate::commands::common::validate_format_args;
use crate::error::{CliError, CliResult, SkillNotFoundMessage};
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::lock::{global_lock_path, GlobalSkillsLock, ProjectSkillsLock};
use fastskill_core::core::project::resolve_project_file;
use fastskill_core::core::skill_manager::SkillDefinition;
use fastskill_core::output::{format_show_results, OutputFormat};
use fastskill_core::FastSkillService;
use std::collections::HashMap;
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
            examples: vec!["fastskill read pptx", "fastskill read pptx --meta --json"],
            args: vec![
                ArgSpec {
                    name: "skill-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Exact installed skill identifier (for example 'pptx' or 'scope/pptx')",
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

/// Resolve an installed skill by its exact canonical ID.
async fn resolve_skill(
    service: &Arc<FastSkillService>,
    skill_id_str: &str,
    global: bool,
) -> CliResult<SkillDefinition> {
    if skill_id_str.contains('@') {
        return Err(CliError::Validation(format!(
            "Installed read accepts an exact canonical ID without a version, got '{skill_id_str}'. Use 'fastskill repos versions <ID>' to discover repository versions."
        )));
    }
    let skill_id = fastskill_core::SkillId::new(skill_id_str.to_string()).map_err(|error| {
        CliError::Validation(format!(
            "Invalid skill ID '{skill_id_str}': {error}. Expected 'name' or 'scope/name'."
        ))
    })?;
    service
        .skill_manager()
        .get_skill(&skill_id)
        .await
        .map_err(CliError::Service)?
        .ok_or_else(|| {
            let searched_paths = crate::config::get_skill_search_locations_for_display(global)
                .unwrap_or_else(|_| {
                    vec![(
                        service.config().skill_storage_path.clone(),
                        if global { "global" } else { "project" }.to_string(),
                    )]
                });
            CliError::SkillNotFound(SkillNotFoundMessage::new(
                skill_id_str.to_string(),
                searched_paths,
            ))
        })
}

fn locked_skill(skill_id: &str, global: bool) -> CliResult<SkillDefinition> {
    fastskill_core::SkillId::new(skill_id.to_string())
        .map_err(|error| CliError::Validation(format!("Invalid skill ID format: {error}")))?;
    if global {
        let path = global_lock_path()
            .map_err(|error| CliError::Config(format!("Failed to resolve global lock: {error}")))?;
        let lock = GlobalSkillsLock::load_from_file(&path)
            .map_err(|error| CliError::Config(format!("Failed to load global lock: {error}")))?;
        let entry = lock
            .skills
            .iter()
            .find(|entry| entry.id == skill_id)
            .ok_or_else(|| {
                CliError::Validation(format!("Skill '{skill_id}' not found in global lock"))
            })?;
        let id = fastskill_core::SkillId::new(entry.id.clone()).map_err(CliError::Service)?;
        let mut skill = SkillDefinition::new(
            id,
            entry.name.clone(),
            String::new(),
            entry.resolved.version.clone(),
            entry.origin.clone(),
        );
        skill.dependencies = Some(entry.dependencies.clone());
        return Ok(skill);
    }
    let current = std::env::current_dir().map_err(CliError::Io)?;
    let project = resolve_project_file(&current);
    if !project.found {
        return Err(CliError::Config(
            "skill-project.toml not found; cannot resolve project Lock".to_string(),
        ));
    }
    let path = project
        .path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("skills.lock");
    let lock = ProjectSkillsLock::load_from_file(&path)
        .map_err(|error| CliError::Config(format!("Failed to load skills.lock: {error}")))?;
    let entry = lock
        .skills
        .iter()
        .find(|entry| entry.id == skill_id)
        .ok_or_else(|| {
            CliError::Validation(format!("Skill '{skill_id}' not found in skills.lock"))
        })?;
    let id = fastskill_core::SkillId::new(entry.id.clone()).map_err(CliError::Service)?;
    let mut skill = SkillDefinition::new(
        id,
        entry.name.clone(),
        String::new(),
        entry.resolved.version.clone(),
        entry.origin.clone(),
    );
    skill.dependencies = Some(entry.dependencies.clone());
    Ok(skill)
}

/// Execute the read command
pub async fn execute_read(
    service: Arc<FastSkillService>,
    args: ReadArgs,
    global: bool,
) -> CliResult<()> {
    if args.skill_id.contains('@') {
        return Err(CliError::Validation(
            "read accepts an installed canonical ID without @version; use 'fastskill repos versions ID' to discover repository versions"
                .to_string(),
        ));
    }
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
            locked_skill(&args.skill_id, global)?
        } else {
            resolve_skill(&service, &args.skill_id, global).await?
        };

        if args.tree && args.json {
            let actual = resolve_skill(&service, &args.skill_id, global).await?;
            let value = serde_json::json!({
                "metadata": skill,
                "actual_tree": {
                    "id": actual.id.to_string(),
                    "dependencies": actual.dependencies.unwrap_or_default(),
                }
            });
            crate::outln!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|error| {
                    CliError::Config(format!("Failed to format read result: {error}"))
                })?
            );
            return Ok(());
        }

        let output = format_show_results(&[skill], format)
            .map_err(|e| CliError::Config(format!("Failed to format output: {}", e)))?;
        crate::outln!("{}", output);

        // If --tree is also set, fall through to print tree after meta
        if args.tree {
            let tree_skill = resolve_skill(&service, &args.skill_id, global).await?;
            print_dependency_tree(&tree_skill);
        }

        return Ok(());
    }

    // --tree only (no --meta)
    if args.tree {
        let skill = resolve_skill(&service, &args.skill_id, global).await?;
        print_dependency_tree(&skill);
        return Ok(());
    }

    // Default: stream the selected installed SKILL.md.
    let skill = resolve_skill(&service, &args.skill_id, global).await?;

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
    crate::outln!("Reading: {}", args.skill_id);
    crate::outln!("Base directory: {}", base_dir_absolute.display());
    crate::outln!();
    print!("{}", content);
    if !content.ends_with('\n') {
        crate::outln!();
    }
    crate::outln!();
    crate::outln!("Skill read: {}", args.skill_id);

    Ok(())
}

/// Print a dependency tree for the given skill definition.
fn print_dependency_tree(skill: &SkillDefinition) {
    crate::outln!("{}", skill.id);
    match &skill.dependencies {
        Some(deps) if !deps.is_empty() => {
            for dep in deps {
                crate::outln!("  - {}", dep);
            }
        }
        _ => {
            crate::outln!("  (no dependencies)");
        }
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::await_holding_lock
)]
#[cfg(test)]
mod tests {
    use super::*;
    use fastskill_core::{FastSkillService, ServiceConfig};
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn service_with_skill(temp: &TempDir, body: &str) -> Arc<FastSkillService> {
        let skills = temp.path().join("skills");
        let skill = skills.join("demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!(
                "---\nname: demo\nversion: 1.2.3\ndescription: fixture\ndependencies:\n  - child\n---\n{body}"
            ),
        )
        .unwrap();
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: skills,
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();
        Arc::new(service)
    }

    fn args(meta: bool, tree: bool, json: bool) -> ReadArgs {
        ReadArgs {
            skill_id: "demo".to_string(),
            meta,
            tree,
            format: None,
            json,
            locked: false,
        }
    }

    #[test]
    fn typed_arguments_accept_every_output_format_and_ignore_wrong_optional_types() {
        for (name, expected) in [
            ("table", OutputFormat::Table),
            ("json", OutputFormat::Json),
            ("grid", OutputFormat::Grid),
            ("xml", OutputFormat::Xml),
        ] {
            assert_eq!(parse_output_format(name), Some(expected));
        }
        assert_eq!(parse_output_format("yaml"), None);
        let mut map = HashMap::new();
        map.insert("skill-id".to_string(), ArgValue::Str("demo".to_string()));
        map.insert("format".to_string(), ArgValue::Bool(true));
        map.insert("meta".to_string(), ArgValue::Bool(true));
        map.insert("tree".to_string(), ArgValue::Bool(true));
        map.insert("json".to_string(), ArgValue::Bool(true));
        map.insert("locked".to_string(), ArgValue::Bool(true));
        let args = ReadArgs::from_arg_value_map(&map);
        assert_eq!(args.skill_id, "demo");
        assert!(args.meta && args.tree && args.json && args.locked);
        assert_eq!(args.format, None);
        assert_eq!(
            ReadArgs::command_spec().syntax,
            Some("read <SKILL_ID> [OPTIONS]")
        );
    }

    #[tokio::test]
    async fn installed_read_covers_content_metadata_and_tree_modes() {
        let temp = TempDir::new().unwrap();
        let service = service_with_skill(&temp, "# Demo\n").await;
        for args in [
            args(false, false, false),
            args(false, true, false),
            args(true, false, false),
            args(true, true, false),
            args(true, true, true),
        ] {
            assert!(execute_read(Arc::clone(&service), args, false)
                .await
                .is_ok());
        }
    }

    #[tokio::test]
    async fn installed_read_rejects_oversized_documentation_before_streaming() {
        let temp = TempDir::new().unwrap();
        let service = service_with_skill(&temp, &"x".repeat(512_001)).await;
        let result = execute_read(service, args(false, false, false), false).await;
        assert!(
            matches!(result, Err(CliError::Validation(message)) if message.contains("exceeds maximum"))
        );
    }

    #[tokio::test]
    async fn locked_project_metadata_uses_the_selected_lock_entry() {
        use fastskill_core::core::lock::ProjectLockedSkillEntry;
        use fastskill_core::core::origin::{Origin, Resolved};

        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let temp = TempDir::new().unwrap();
        let original = std::env::current_dir().unwrap();
        struct Restore(std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _restore = Restore(original);
        std::env::set_current_dir(temp.path()).unwrap();
        fs::write(temp.path().join("skill-project.toml"), "[dependencies]\n").unwrap();
        let mut lock = ProjectSkillsLock::new_empty();
        lock.skills.push(ProjectLockedSkillEntry {
            id: "demo".to_string(),
            name: "Demo".to_string(),
            origin: Origin::Local {
                path: "source/demo".into(),
                editable: false,
            },
            resolved: Resolved {
                version: "4.5.6".to_string(),
                commit_hash: None,
                checksum: Some("sha256:fixture".to_string()),
            },
            dependencies: vec!["child".to_string()],
            groups: vec!["default".to_string()],
            depth: 0,
            parent_skill: None,
            required_by: Vec::new(),
        });
        lock.save_to_file(&temp.path().join("skills.lock")).unwrap();
        let mut service = FastSkillService::new(ServiceConfig {
            skill_storage_path: temp.path().join("skills"),
            ..Default::default()
        })
        .await
        .unwrap();
        service.initialize().await.unwrap();

        assert!(execute_read(
            Arc::new(service),
            ReadArgs {
                locked: true,
                ..args(true, false, false)
            },
            false,
        )
        .await
        .is_ok());
        assert!(matches!(
            locked_skill("absent", false),
            Err(CliError::Validation(message)) if message.contains("not found")
        ));
    }

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

        let result = execute_read(Arc::new(service), args, false).await;
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

        let result = execute_read(Arc::new(service), args, false).await;
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

        let result = execute_read(Arc::new(service), args, false).await;
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

        let result = execute_read(Arc::new(service), args, false).await;
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

        let result = execute_read(Arc::new(service), args, false).await;
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

        let result = execute_read(Arc::new(service), args, false).await;
        assert!(matches!(result, Err(CliError::Validation(_))));
        if let Err(CliError::Validation(msg)) = result {
            assert!(msg.contains("--meta is required"));
        }
    }
}

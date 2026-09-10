//! Init command implementation
//!
//! Creates skill-project.toml for skill authors and project-level configuration:
//! - **Skill context**: In skill directory - creates [metadata] section for skill authoring
//! - **Project context**: At project root - creates [dependencies] section for managing skills
//! - **SKILL.md only**: When skill follows standard without extra config - skill-project.toml is optional

use crate::error::{CliError, CliResult};
use crate::utils::messages;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use fastskill_core::core::manifest::{
    DependenciesSection, FastSkillToolConfig, MetadataSection, SkillProjectToml, ToolSection,
    MANIFEST_SCHEMA_VERSION,
};
use fastskill_core::core::metadata::parse_yaml_frontmatter;
use fastskill_core::core::validation::{
    validate_identifier, validate_project_structure, validate_semver,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Initialize skill-project.toml for the current directory
#[derive(Debug)]
pub struct InitArgs {
    /// Skip interactive prompts and use defaults
    yes: bool,

    /// Force reinitialization even if skill-project.toml exists
    force: bool,

    /// Set version directly
    version: Option<String>,

    /// Set skill description
    description: Option<String>,

    /// Set skill author
    author: Option<String>,

    /// Set download URL
    download_url: Option<String>,

    /// Skills directory path (required for project-level, optional for skill-level)
    skills_dir: Option<String>,
}

impl IntoCommandSpec for InitArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Initialize skill-project.toml in current skill directory",
            syntax: Some("project init [OPTIONS]"),
            category: Some("project"),
            help_order: Some(10),
            examples: vec![
                "fastskill project init",
                "fastskill project init --yes --description \"My skill\"",
            ],
            args: vec![
                ArgSpec {
                    name: "yes",
                    kind: ArgKind::Flag,
                    long: Some("yes"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Skip interactive prompts and use defaults",
                    ..Default::default()
                },
                ArgSpec {
                    name: "force",
                    kind: ArgKind::Flag,
                    long: Some("force"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Force reinitialization even if skill-project.toml exists",
                    ..Default::default()
                },
                ArgSpec {
                    name: "set-version",
                    kind: ArgKind::Option,
                    long: Some("set-version"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Set version directly",
                    ..Default::default()
                },
                ArgSpec {
                    name: "description",
                    kind: ArgKind::Option,
                    long: Some("description"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Set skill description",
                    ..Default::default()
                },
                ArgSpec {
                    name: "author",
                    kind: ArgKind::Option,
                    long: Some("author"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Set skill author",
                    ..Default::default()
                },
                ArgSpec {
                    name: "download-url",
                    kind: ArgKind::Option,
                    long: Some("download-url"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Set download URL",
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

fn bool_flag(v: &ArgValue) -> Option<bool> {
    if let ArgValue::Bool(b) = v {
        Some(*b)
    } else {
        None
    }
}

#[allow(clippy::panic)]
impl FromArgValueMap for InitArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            yes: map.get("yes").and_then(bool_flag).unwrap_or(false),
            force: map.get("force").and_then(bool_flag).unwrap_or(false),
            version: map.get("set-version").and_then(opt_str),
            description: map.get("description").and_then(opt_str),
            author: map.get("author").and_then(opt_str),
            download_url: map.get("download-url").and_then(opt_str),
            // `--skills-dir` is a global (root-level) flag, so it never reaches this
            // subcommand's arg map. main.rs injects it via `with_skills_dir`.
            skills_dir: None,
        }
    }
}

impl InitArgs {
    /// Inject the value of the global `--skills-dir` flag.
    ///
    /// `--skills-dir` is declared on the root command, not on `init`, so it is
    /// absent from this subcommand's arg map; the caller reads it off the app
    /// context and hands it in here.
    pub fn with_skills_dir(mut self, skills_dir: Option<String>) -> Self {
        if skills_dir.is_some() {
            self.skills_dir = skills_dir;
        }
        self
    }
}

pub async fn execute_init(args: InitArgs) -> CliResult<()> {
    crate::outln!("FastSkill Skill Initialization");
    crate::outln!();

    let skill_project_path = Path::new("skill-project.toml");
    ensure_can_init(skill_project_path, args.force)?;

    let is_skill_level = Path::new("SKILL.md").exists();
    let (skill_md_content, frontmatter) = load_skill_md_and_frontmatter(is_skill_level)?;

    let version = resolve_version(&args, &frontmatter, skill_md_content.as_deref())?;
    let description = resolve_description(&args, &frontmatter)?;
    let author = resolve_author(&args, &frontmatter)?;
    let download_url = resolve_download_url(&args)?;
    let skills_directory = resolve_skills_directory(is_skill_level, &args)?;

    let skill_id = resolve_skill_id()?;

    let meta = InitMetadata {
        skill_id: &skill_id,
        version: version.clone(),
        description,
        author,
        download_url,
        skills_directory: &skills_directory,
    };
    let skill_project = build_skill_project(meta)?;

    skill_project
        .save_to_file(skill_project_path)
        .map_err(|e| CliError::Config(format!("Failed to write skill-project.toml: {}", e)))?;

    append_tool_comment(skill_project_path, is_skill_level)?;

    let context = fastskill_core::core::project::detect_context(skill_project_path);
    skill_project.validate_for_context(context).map_err(|e| {
        CliError::Config(format!(
            "skill-project.toml validation failed after creation: {}",
            e
        ))
    })?;

    print_success(is_skill_level, &version, skills_directory.as_deref());
    Ok(())
}

fn ensure_can_init(path: &Path, force: bool) -> CliResult<()> {
    if path.exists() && !force {
        return Err(CliError::Config(
            "skill-project.toml already exists. Use --force to overwrite.".to_string(),
        ));
    }
    Ok(())
}

fn load_skill_md_and_frontmatter(
    skill_md_exists: bool,
) -> CliResult<(
    Option<String>,
    fastskill_core::core::metadata::SkillFrontmatter,
)> {
    if !skill_md_exists {
        return Ok((
            None,
            fastskill_core::core::metadata::SkillFrontmatter {
                name: String::new(),
                description: String::new(),
                version: None,
                author: None,
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: None,
                extra: HashMap::new(),
            },
        ));
    }
    let content = fs::read_to_string("SKILL.md")
        .map_err(|e| CliError::Config(format!("Failed to read SKILL.md: {}", e)))?;
    let frontmatter = parse_yaml_frontmatter(&content)
        .map_err(|e| CliError::Config(format!("Failed to parse SKILL.md frontmatter: {}", e)))?;
    Ok((Some(content), frontmatter))
}

fn resolve_version(
    args: &InitArgs,
    frontmatter: &fastskill_core::core::metadata::SkillFrontmatter,
    skill_md_content: Option<&str>,
) -> CliResult<String> {
    if let Some(ref version_arg) = args.version {
        validate_semver(version_arg)
            .map_err(|e| CliError::InvalidSemver(format!("{}: {}", version_arg, e)))?;
        return Ok(version_arg.clone());
    }
    if let Some(ref v) = frontmatter.version {
        if !v.is_empty() {
            validate_semver(v).map_err(|e| CliError::InvalidSemver(format!("{}: {}", v, e)))?;
            return Ok(v.clone());
        }
    }
    let content = skill_md_content.unwrap_or("");
    extract_version_from_skill_md(content, args.yes)
}

fn resolve_description(
    args: &InitArgs,
    frontmatter: &fastskill_core::core::metadata::SkillFrontmatter,
) -> CliResult<Option<String>> {
    if let Some(ref d) = args.description {
        return Ok(Some(d.clone()));
    }
    if !frontmatter.description.is_empty() {
        return Ok(Some(frontmatter.description.clone()));
    }
    if !args.yes {
        return prompt_for_field("Description", None);
    }
    Ok(None)
}

fn resolve_author(
    args: &InitArgs,
    frontmatter: &fastskill_core::core::metadata::SkillFrontmatter,
) -> CliResult<Option<String>> {
    if let Some(ref a) = args.author {
        return Ok(Some(a.clone()));
    }
    if let Some(ref a) = frontmatter.author {
        return Ok(Some(a.clone()));
    }
    if !args.yes {
        return Ok(prompt_for_field("Author", None).ok().flatten());
    }
    Ok(None)
}

fn resolve_download_url(args: &InitArgs) -> CliResult<Option<String>> {
    if let Some(ref u) = args.download_url {
        return Ok(Some(u.clone()));
    }
    if !args.yes {
        return Ok(prompt_for_field("Download URL", None).ok().flatten());
    }
    Ok(None)
}

/// Default project-level skills directory, used both when the interactive
/// prompt gets an empty answer and when `--yes` skips the prompt entirely.
const DEFAULT_PROJECT_SKILLS_DIRECTORY: &str = ".claude/skills";

fn resolve_skills_directory(is_skill_level: bool, args: &InitArgs) -> CliResult<Option<String>> {
    if is_skill_level {
        return Ok(args.skills_dir.clone());
    }
    if let Some(ref dir) = args.skills_dir {
        return Ok(Some(dir.clone()));
    }
    if !args.yes {
        let dir = prompt_for_field("Skills directory", Some(DEFAULT_PROJECT_SKILLS_DIRECTORY))?
            .or(Some(DEFAULT_PROJECT_SKILLS_DIRECTORY.to_string()));
        return Ok(dir);
    }
    // `--yes` means "use defaults", matching this command's own `--help`
    // examples (`fastskill project init --yes --description "My skill"`) and the
    // README quick start -- neither passes `--skills-dir`. The interactive
    // path above already defaults an empty answer to ".claude/skills"; `--yes`
    // must take that same default rather than erroring.
    Ok(Some(DEFAULT_PROJECT_SKILLS_DIRECTORY.to_string()))
}

struct InitMetadata<'a> {
    skill_id: &'a str,
    version: String,
    description: Option<String>,
    author: Option<String>,
    download_url: Option<String>,
    skills_directory: &'a Option<String>,
}

fn resolve_skill_id() -> CliResult<String> {
    let current_dir = std::env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let skill_id = current_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            CliError::Config("Cannot determine skill ID from current directory name".to_string())
        })?;
    validate_identifier(skill_id)
        .map_err(|e| CliError::InvalidIdentifier(format!("Skill ID '{}': {}", skill_id, e)))?;
    Ok(skill_id.to_string())
}

fn build_skill_project(meta: InitMetadata<'_>) -> CliResult<SkillProjectToml> {
    let metadata = Some(MetadataSection {
        id: Some(meta.skill_id.to_string()),
        version: Some(meta.version),
        description: meta.description,
        author: meta.author,
        download_url: meta.download_url,
        name: None,
    });
    let dependencies = Some(DependenciesSection {
        dependencies: HashMap::new(),
    });
    let tool = meta.skills_directory.as_ref().map(|dir| ToolSection {
        fastskill: Some(FastSkillToolConfig {
            skills_directory: Some(std::path::PathBuf::from(dir)),
            embedding: None,
            repositories: None,
            server: None,
            install_depth: 5,
            skip_transitive: false,
            eval: None,
            auto_reindex: true,
        }),
    });
    validate_project_structure(true, dependencies.is_some())
        .map_err(|e| CliError::ProjectTomlValidation(e.to_string()))?;
    Ok(SkillProjectToml {
        schema_version: Some(MANIFEST_SCHEMA_VERSION.to_string()),
        metadata,
        dependencies,
        tool,
    })
}

fn append_tool_comment(path: &Path, is_skill_level: bool) -> CliResult<()> {
    let comment = if is_skill_level {
        r#"
# Optional: FastSkill configuration for skill authors
# Uncomment and configure as needed

# [tool.fastskill]
# skills_directory = ".claude/skills"
#
# [tool.fastskill.embedding]
# openai_base_url = "https://api.openai.com/v1"
# embedding_model = "text-embedding-3-small"
#
# [[tool.fastskill.repositories]]
# name = "default"
# type = "http-registry"
# index_url = "https://registry.fastskill.dev"
# priority = 0
"#
    } else {
        r#"
# Additional configuration options for [tool.fastskill]:
#
# [tool.fastskill.embedding]
# openai_base_url = "https://api.openai.com/v1"
# embedding_model = "text-embedding-3-small"
#
# [[tool.fastskill.repositories]]
# name = "default"
# type = "http-registry"
# index_url = "https://registry.fastskill.dev"
# priority = 0
"#
    };
    let mut content = fs::read_to_string(path)
        .map_err(|e| CliError::Config(format!("Failed to read skill-project.toml: {}", e)))?;
    content.push_str(comment);
    fs::write(path, content)
        .map_err(|e| CliError::Config(format!("Failed to write skill-project.toml: {}", e)))?;
    Ok(())
}

fn print_success(is_skill_level: bool, version: &str, skills_directory: Option<&str>) {
    crate::outln!(
        "{}",
        messages::ok(&format!(
            "Created skill-project.toml with version: {}",
            version
        ))
    );
    if is_skill_level {
        crate::outln!();
        crate::outln!(
            "{}",
            messages::info("This file contains author-provided metadata for your skill.")
        );
        crate::outln!("   It will be used by fastskill for version management.");
        return;
    }
    if let Some(dir) = skills_directory {
        crate::outln!();
        crate::outln!("{}", messages::info(&format!("Skills directory: {}", dir)));
    }
    crate::outln!();
    crate::outln!(
        "{}",
        messages::info("This file configures your project's skill dependencies.")
    );
    crate::outln!("   Add skills with: fastskill skill add <skill-id>");
}

fn extract_version_from_skill_md(content: &str, skip_prompts: bool) -> CliResult<String> {
    if let Some(v) = try_version_from_content(content) {
        return Ok(v);
    }
    default_version_or_prompt(skip_prompts)
}

fn default_version_or_prompt(skip_prompts: bool) -> CliResult<String> {
    if skip_prompts {
        Ok("1.0.0".to_string())
    } else {
        prompt_for_version()
    }
}

fn try_version_from_content(content: &str) -> Option<String> {
    if content.is_empty() || !content.starts_with("---") {
        return None;
    }
    let opening_end = opening_delimiter_end(content)?;
    let after_opening = &content[opening_end..];
    let closing_start = find_closing_delimiter(content, opening_end, after_opening)?;
    let frontmatter = content[opening_end..closing_start].trim();
    version_from_frontmatter_text(frontmatter)
}

fn opening_delimiter_end(content: &str) -> Option<usize> {
    if content.starts_with("---\n") {
        Some(4)
    } else if content.starts_with("---\r\n") {
        Some(5)
    } else if content.len() > 3 && content.starts_with("---") {
        Some(3)
    } else {
        None
    }
}

fn find_closing_delimiter(content: &str, opening_end: usize, after_opening: &str) -> Option<usize> {
    if let Some(pos) = after_opening.find("\n---\n") {
        return Some(opening_end + pos + 1);
    }
    if let Some(pos) = after_opening.find("\n---\r\n") {
        return Some(opening_end + pos + 1);
    }
    if let Some(pos) = after_opening.find("\n---") {
        let start = opening_end + pos + 1;
        if is_valid_closing(content, start) {
            return Some(start);
        }
    }
    None
}

fn is_valid_closing(content: &str, start: usize) -> bool {
    if start + 3 > content.len() {
        return false;
    }
    let after_dash = start + 3;
    if after_dash >= content.len() {
        return true;
    }
    let c = content[after_dash..].chars().next();
    c.map(|c| c == '\n' || c == '\r' || c.is_whitespace())
        .unwrap_or(true)
}

fn version_from_frontmatter_text(frontmatter: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let line = line.trim();
        if !line.starts_with("version:") {
            continue;
        }
        let v = line.split(':').nth(1)?.trim();
        let v = v.trim_matches('"').trim_matches('\'').trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn prompt_for_version() -> CliResult<String> {
    crate::outln!("Version");
    crate::outln!("No version found in SKILL.md frontmatter.");
    print!("Enter version (or press Enter for 1.0.0): ");
    io::stdout().flush()?;

    let mut version = String::new();
    io::stdin().read_line(&mut version)?;
    let version = version.trim();

    if version.is_empty() {
        Ok("1.0.0".to_string())
    } else {
        // Validate version format
        validate_semver(version)
            .map_err(|e| CliError::InvalidSemver(format!("{}: {}", version, e)))?;
        Ok(version.to_string())
    }
}

fn prompt_for_field(field_name: &str, default: Option<&str>) -> CliResult<Option<String>> {
    let prompt = if let Some(default_val) = default {
        format!("{} (default: {}): ", field_name, default_val)
    } else {
        format!("{} (optional, press Enter to skip): ", field_name)
    };

    print!("{}", prompt);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        Ok(default.map(|s| s.to_string()))
    } else {
        Ok(Some(input.to_string()))
    }
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
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    struct CwdGuard(PathBuf);

    impl CwdGuard {
        fn enter(path: &Path) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self(original)
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }

    fn args() -> InitArgs {
        InitArgs {
            yes: true,
            force: false,
            version: None,
            description: None,
            author: None,
            download_url: None,
            skills_dir: None,
        }
    }

    fn valid_dir(temp: &TempDir, name: &str) -> PathBuf {
        let path = temp.path().join(name);
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn command_contract_and_arg_map_use_project_namespace() {
        let spec = InitArgs::command_spec();
        assert_eq!(spec.syntax, Some("project init [OPTIONS]"));
        assert_eq!(spec.category, Some("project"));
        assert_eq!(spec.help_order, Some(10));
        assert!(spec
            .examples
            .iter()
            .all(|e| e.starts_with("fastskill project init")));

        let map = HashMap::from([
            ("yes".to_string(), ArgValue::Bool(true)),
            ("force".to_string(), ArgValue::Bool(true)),
            (
                "set-version".to_string(),
                ArgValue::Str("2.3.4".to_string()),
            ),
            ("description".to_string(), ArgValue::Str("Demo".to_string())),
            ("author".to_string(), ArgValue::Str("Ada".to_string())),
            (
                "download-url".to_string(),
                ArgValue::Str("https://example.test/demo".to_string()),
            ),
        ]);
        let parsed = InitArgs::from_arg_value_map(&map).with_skills_dir(Some("skills".to_string()));
        assert!(parsed.yes && parsed.force);
        assert_eq!(parsed.version.as_deref(), Some("2.3.4"));
        assert_eq!(parsed.description.as_deref(), Some("Demo"));
        assert_eq!(parsed.author.as_deref(), Some("Ada"));
        assert_eq!(
            parsed.download_url.as_deref(),
            Some("https://example.test/demo")
        );
        assert_eq!(parsed.skills_dir.as_deref(), Some("skills"));

        let wrong_types = HashMap::from([
            ("yes".to_string(), ArgValue::Str("true".to_string())),
            ("force".to_string(), ArgValue::Str("true".to_string())),
            ("set-version".to_string(), ArgValue::Bool(true)),
            ("description".to_string(), ArgValue::Bool(true)),
            ("author".to_string(), ArgValue::Bool(true)),
            ("download-url".to_string(), ArgValue::Bool(true)),
        ]);
        let parsed = InitArgs::from_arg_value_map(&wrong_types).with_skills_dir(None);
        assert!(!parsed.yes && !parsed.force);
        assert!(parsed.version.is_none() && parsed.description.is_none());
        assert!(parsed.author.is_none() && parsed.download_url.is_none());
        assert!(parsed.skills_dir.is_none());
    }

    #[tokio::test]
    async fn project_init_writes_defaults_and_force_controls_overwrite() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = TempDir::new().unwrap();
        let project = valid_dir(&temp_dir, "demo-project");
        let _cwd = CwdGuard::enter(&project);

        execute_init(args()).await.unwrap();
        let content = fs::read_to_string("skill-project.toml").unwrap();
        assert!(content.contains("schema_version = \"1\""));
        assert!(content.contains("id = \"demo-project\""));
        assert!(content.contains("version = \"1.0.0\""));
        assert!(content.contains("skills_directory = \".claude/skills\""));
        assert!(content.contains("# Additional configuration options"));

        let error = execute_init(args()).await.unwrap_err();
        assert!(error.to_string().contains("Use --force to overwrite"));
        let mut force = args();
        force.force = true;
        force.skills_dir = Some(".agents/skills".to_string());
        execute_init(force).await.unwrap();
        let content = fs::read_to_string("skill-project.toml").unwrap();
        assert!(content.contains("skills_directory = \".agents/skills\""));
    }

    #[tokio::test]
    async fn skill_init_uses_frontmatter_and_keeps_tool_config_optional() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = TempDir::new().unwrap();
        let project = valid_dir(&temp_dir, "demo-skill");
        fs::write(
            project.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo skill\nversion: 2.3.4\nauthor: Ada\n---\n# Demo\n",
        )
        .unwrap();
        let _cwd = CwdGuard::enter(&project);

        execute_init(args()).await.unwrap();
        let content = fs::read_to_string("skill-project.toml").unwrap();
        assert!(content.contains("version = \"2.3.4\""));
        assert!(content.contains("description = \"Demo skill\""));
        assert!(content.contains("author = \"Ada\""));
        assert!(!content.contains("\n[tool.fastskill]\n"));
        assert!(content.contains("# [tool.fastskill]"));
    }

    #[test]
    fn resolvers_prefer_explicit_values_and_validate_versions() {
        let frontmatter = parse_yaml_frontmatter(
            "---\nname: demo\ndescription: Frontmatter\nversion: 1.2.3\nauthor: Ada\n---\n",
        )
        .unwrap();
        let mut explicit = args();
        explicit.version = Some("2.0.0".to_string());
        explicit.description = Some("Explicit".to_string());
        explicit.author = Some("Grace".to_string());
        explicit.download_url = Some("https://example.test".to_string());
        assert_eq!(
            resolve_version(&explicit, &frontmatter, None).unwrap(),
            "2.0.0"
        );
        assert_eq!(
            resolve_description(&explicit, &frontmatter)
                .unwrap()
                .as_deref(),
            Some("Explicit")
        );
        assert_eq!(
            resolve_author(&explicit, &frontmatter).unwrap().as_deref(),
            Some("Grace")
        );
        assert_eq!(
            resolve_download_url(&explicit).unwrap().as_deref(),
            Some("https://example.test")
        );

        assert_eq!(
            resolve_version(&args(), &frontmatter, None).unwrap(),
            "1.2.3"
        );
        assert_eq!(
            resolve_description(&args(), &frontmatter)
                .unwrap()
                .as_deref(),
            Some("Frontmatter")
        );
        assert_eq!(
            resolve_author(&args(), &frontmatter).unwrap().as_deref(),
            Some("Ada")
        );
        assert_eq!(resolve_skills_directory(true, &args()).unwrap(), None);
        assert_eq!(
            resolve_skills_directory(false, &args()).unwrap().as_deref(),
            Some(DEFAULT_PROJECT_SKILLS_DIRECTORY)
        );

        let mut invalid = args();
        invalid.version = Some("not-semver".to_string());
        assert!(matches!(
            resolve_version(&invalid, &frontmatter, None),
            Err(CliError::InvalidSemver(_))
        ));
        let invalid_frontmatter =
            parse_yaml_frontmatter("---\nname: demo\ndescription: Demo\nversion: bad\n---\n")
                .unwrap();
        assert!(matches!(
            resolve_version(&args(), &invalid_frontmatter, None),
            Err(CliError::InvalidSemver(_))
        ));
    }

    #[test]
    fn version_text_parser_handles_supported_delimiters_and_shapes() {
        for (content, expected) in [
            ("---\nversion: 1.2.3\n---\n# Demo", Some("1.2.3")),
            ("---\r\nversion: '2.0.0'\r\n---\r\n", Some("2.0.0")),
            ("---version: \"3.4.5\"\n---", Some("3.4.5")),
            ("---\nname: demo\n---", None),
            ("---\nversion:\n---", None),
            ("not frontmatter", None),
            ("", None),
            ("---\nversion: 1.0.0", None),
            ("---\nversion: 1.0.0\n---suffix", None),
        ] {
            assert_eq!(try_version_from_content(content).as_deref(), expected);
        }
        assert_eq!(extract_version_from_skill_md("", true).unwrap(), "1.0.0");
        assert!(is_valid_closing("---", 0));
        assert!(!is_valid_closing("--", 0));
        assert!(!is_valid_closing("---suffix", 0));
    }

    #[tokio::test]
    async fn invalid_directory_and_skill_file_return_typed_errors() {
        let _lock = fastskill_core::test_utils::DIR_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = TempDir::new().unwrap();
        let invalid = valid_dir(&temp_dir, "bad.id");
        let _cwd = CwdGuard::enter(&invalid);
        assert!(matches!(
            execute_init(args()).await,
            Err(CliError::InvalidIdentifier(_))
        ));
        drop(_cwd);

        let malformed = valid_dir(&temp_dir, "malformed-skill");
        fs::write(malformed.join("SKILL.md"), "---\nname: [\n---\n").unwrap();
        let _cwd = CwdGuard::enter(&malformed);
        let error = execute_init(args()).await.unwrap_err();
        assert!(error
            .to_string()
            .contains("Failed to parse SKILL.md frontmatter"));

        assert!(append_tool_comment(Path::new("missing.toml"), false)
            .unwrap_err()
            .to_string()
            .contains("Failed to read skill-project.toml"));
        print_success(true, "1.0.0", None);
        print_success(false, "1.0.0", Some("skills"));
    }
}

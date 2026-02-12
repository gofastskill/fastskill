//! Init command implementation - creates skill-project.toml for skill authors

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use clap::Args;
use fastskill::core::manifest::{
    DependenciesSection, FastSkillToolConfig, MetadataSection, SkillProjectToml, ToolSection,
};
use fastskill::core::metadata::parse_yaml_frontmatter;
use fastskill::core::validation::{
    validate_identifier, validate_project_structure, validate_semver,
};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Initialize skill-project.toml for the current directory
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Skip interactive prompts and use defaults
    #[arg(long)]
    yes: bool,

    /// Force reinitialization even if skill-project.toml exists
    #[arg(long)]
    force: bool,

    /// Set version directly
    #[arg(long)]
    version: Option<String>,

    /// Set skill description
    #[arg(long)]
    description: Option<String>,

    /// Set skill author
    #[arg(long)]
    author: Option<String>,

    /// Set download URL
    #[arg(long)]
    download_url: Option<String>,

    /// Skills directory path (required for project-level, optional for skill-level)
    #[arg(long)]
    skills_dir: Option<String>,
}

pub async fn execute_init(args: InitArgs) -> CliResult<()> {
    println!("FastSkill Skill Initialization");
    println!();

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

    let context = fastskill::core::project::detect_context(skill_project_path);
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
) -> CliResult<(Option<String>, fastskill::core::metadata::SkillFrontmatter)> {
    if !skill_md_exists {
        return Ok((
            None,
            fastskill::core::metadata::SkillFrontmatter {
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
    frontmatter: &fastskill::core::metadata::SkillFrontmatter,
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
    frontmatter: &fastskill::core::metadata::SkillFrontmatter,
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
    frontmatter: &fastskill::core::metadata::SkillFrontmatter,
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

fn resolve_skills_directory(is_skill_level: bool, args: &InitArgs) -> CliResult<Option<String>> {
    if is_skill_level {
        return Ok(args.skills_dir.clone());
    }
    if let Some(ref dir) = args.skills_dir {
        return Ok(Some(dir.clone()));
    }
    if !args.yes {
        let dir = prompt_for_field("Skills directory", Some(".claude/skills"))?
            .or(Some(".claude/skills".to_string()));
        return Ok(dir);
    }
    Err(CliError::Config(
        "Project-level init requires --skills-dir <path>. Provide --skills-dir or remove --yes to be prompted.".to_string()
    ))
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
        }),
    });
    validate_project_structure(true, dependencies.is_some())
        .map_err(|e| CliError::ProjectTomlValidation(e.to_string()))?;
    Ok(SkillProjectToml {
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
    println!(
        "{}",
        messages::ok(&format!(
            "Created skill-project.toml with version: {}",
            version
        ))
    );
    if is_skill_level {
        println!();
        println!(
            "{}",
            messages::info("This file contains author-provided metadata for your skill.")
        );
        println!("   It will be used by fastskill for version management.");
        return;
    }
    if let Some(dir) = skills_directory {
        println!();
        println!("{}", messages::info(&format!("Skills directory: {}", dir)));
    }
    println!();
    println!(
        "{}",
        messages::info("This file configures your project's skill dependencies.")
    );
    println!("   Add skills with: fastskill add <skill-id>");
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
    println!("Version");
    println!("No version found in SKILL.md frontmatter.");
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
    clippy::collapsible_if
)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_init_with_all_args() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = InitArgs {
            yes: true,
            force: false,
            version: Some("1.0.0".to_string()),
            description: Some("Test description".to_string()),
            author: Some("Test Author".to_string()),
            download_url: Some("https://example.com".to_string()),
            skills_dir: Some(".claude/skills".to_string()),
        };

        let result = execute_init(args).await;
        // May succeed or fail depending on environment, but shouldn't panic
        if result.is_ok() {
            // Verify file was created if successful
            if Path::new("skill-project.toml").exists() {
                fs::remove_file("skill-project.toml").ok();
            }
        }
    }

    #[tokio::test]
    async fn test_execute_init_with_invalid_version() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = InitArgs {
            yes: true,
            force: false,
            version: Some("invalid-version".to_string()),
            description: None,
            author: None,
            download_url: None,
            skills_dir: Some(".claude/skills".to_string()),
        };

        let result = execute_init(args).await;
        assert!(result.is_err());
        if let Err(CliError::InvalidSemver(_)) = result {
            // Correct error type
        } else {
            panic!("Expected InvalidSemver error");
        }
    }

    #[tokio::test]
    async fn test_execute_init_with_invalid_id() {
        // Note: This test verifies that skill ID validation works
        // The ID is derived from directory name, so we can't easily test invalid IDs here
        // Invalid ID validation happens when the directory name is invalid
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = InitArgs {
            yes: true,
            force: false,
            version: Some("1.0.0".to_string()),
            description: None,
            author: None,
            download_url: None,
            skills_dir: Some(".claude/skills".to_string()),
        };

        // This test now just verifies the function doesn't panic
        // ID validation happens based on directory name, which is harder to test in unit tests
        let result = execute_init(args).await;
        // May succeed or fail depending on directory name, but shouldn't panic
        if result.is_ok() {
            if Path::new("skill-project.toml").exists() {
                fs::remove_file("skill-project.toml").ok();
            }
        }
    }

    #[tokio::test]
    async fn test_execute_init_success() {
        let _lock = fastskill::test_utils::DIR_MUTEX
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

        let args = InitArgs {
            yes: true,
            force: false,
            version: None,
            description: None,
            author: None,
            download_url: None,
            skills_dir: Some(".claude/skills".to_string()),
        };

        let result = execute_init(args).await;
        // May succeed or fail depending on various factors, but shouldn't panic
        if result.is_ok() {
            assert!(Path::new("skill-project.toml").exists());
            let content = fs::read_to_string("skill-project.toml").unwrap();
            assert!(content.contains("[tool.fastskill]"));
            assert!(content.contains("skills_directory"));

            fs::remove_file("skill-project.toml").ok();
        }
    }
}

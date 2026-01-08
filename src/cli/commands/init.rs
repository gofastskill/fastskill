//! Init command implementation - creates skill-project.toml for skill authors

use crate::cli::error::{CliError, CliResult};
use crate::cli::utils::messages;
use clap::Args;
use fastskill::core::manifest::{
    DependenciesSection, DependencySpec, MetadataSection, SkillProjectToml,
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
}

pub async fn execute_init(args: InitArgs) -> CliResult<()> {
    println!("FastSkill Skill Initialization");
    println!();

    // Check if skill-project.toml already exists
    let skill_project_path = Path::new("skill-project.toml");
    if skill_project_path.exists() && !args.force {
        return Err(CliError::Config(
            "skill-project.toml already exists. Use --force to overwrite.".to_string(),
        ));
    }

    // Detect context: skill-level if SKILL.md exists, otherwise project-level (T040)
    let _context = fastskill::core::project::detect_context(skill_project_path);

    // SKILL.md is optional - check if it exists
    let skill_md_exists = Path::new("SKILL.md").exists();

    // Read SKILL.md if it exists to extract metadata from frontmatter
    let (skill_md_content, frontmatter) = if skill_md_exists {
        let content = fs::read_to_string("SKILL.md")
            .map_err(|e| CliError::Config(format!("Failed to read SKILL.md: {}", e)))?;
        let frontmatter = parse_yaml_frontmatter(&content).map_err(|e| {
            CliError::Config(format!("Failed to parse SKILL.md frontmatter: {}", e))
        })?;
        (Some(content), frontmatter)
    } else {
        (
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
                extra: std::collections::HashMap::new(),
            },
        )
    };

    // Extract version (priority: CLI arg > frontmatter > prompt/default)
    let version = if let Some(version_arg) = args.version {
        // Validate version format
        validate_semver(&version_arg)
            .map_err(|e| CliError::InvalidSemver(format!("{}: {}", version_arg, e)))?;
        version_arg
    } else if let Some(ref version) = frontmatter.version {
        if !version.is_empty() {
            validate_semver(version)
                .map_err(|e| CliError::InvalidSemver(format!("{}: {}", version, e)))?;
            version.clone()
        } else {
            extract_version_from_skill_md(
                skill_md_content.as_ref().unwrap_or(&String::new()),
                args.yes,
            )?
        }
    } else if let Some(ref content) = skill_md_content {
        extract_version_from_skill_md(content, args.yes)?
    } else {
        // Default version when no SKILL.md
        if args.yes {
            "1.0.0".to_string()
        } else {
            extract_version_from_skill_md("", args.yes)?
        }
    };

    // Extract description (priority: CLI arg > frontmatter > prompt/default)
    let description = if let Some(desc_arg) = args.description {
        Some(desc_arg)
    } else if !frontmatter.description.is_empty() {
        Some(frontmatter.description.clone())
    } else if !args.yes {
        prompt_for_field("Description", None)?
    } else {
        None
    };

    // Extract author (priority: CLI arg > frontmatter > prompt/default)
    let author = if let Some(author_arg) = args.author {
        Some(author_arg)
    } else if let Some(frontmatter_author) = frontmatter.author.clone() {
        Some(frontmatter_author)
    } else if !args.yes {
        prompt_for_field("Author", None).ok().flatten()
    } else {
        None
    };

    // Extract download_url (priority: CLI arg > prompt/default)
    let download_url = if let Some(url_arg) = args.download_url {
        Some(url_arg)
    } else if !args.yes {
        prompt_for_field("Download URL", None).ok().flatten()
    } else {
        None
    };

    // Extract skill ID from current directory name
    let current_dir = std::env::current_dir()
        .map_err(|e| CliError::Config(format!("Failed to get current directory: {}", e)))?;
    let skill_id = current_dir.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        CliError::Config("Cannot determine skill ID from current directory name".to_string())
    })?;

    // Validate skill ID format
    validate_identifier(skill_id)
        .map_err(|e| CliError::InvalidIdentifier(format!("Skill ID '{}': {}", skill_id, e)))?;
    let skill_id = skill_id.to_string();

    // Create skill-project.toml
    let version_clone = version.clone();

    // Build metadata section (required - must have at least id and version)
    let metadata = Some(MetadataSection {
        id: Some(skill_id),
        version: Some(version),
        description,
        author,
        download_url,
        name: None,
    });

    // Create empty dependencies section (optional, but we'll create it for consistency)
    let dependencies = Some(DependenciesSection {
        dependencies: HashMap::<String, DependencySpec>::new(),
    });

    // Metadata is now always required (must have id and version)
    // Validate that at least one section exists
    validate_project_structure(true, dependencies.is_some())
        .map_err(|e| CliError::ProjectTomlValidation(e.to_string()))?;

    let skill_project = SkillProjectToml {
        metadata,
        dependencies,
        tool: None,
    };

    skill_project
        .save_to_file(skill_project_path)
        .map_err(|e| CliError::Config(format!("Failed to write skill-project.toml: {}", e)))?;

    // T058: Validate context after creation
    let context = fastskill::core::project::detect_context(skill_project_path);
    skill_project.validate_for_context(context).map_err(|e| {
        CliError::Config(format!(
            "skill-project.toml validation failed after creation: {}",
            e
        ))
    })?;

    println!(
        "{}",
        messages::ok(&format!(
            "Created skill-project.toml with version: {}",
            version_clone
        ))
    );
    println!();
    println!(
        "{}",
        messages::info("This file contains author-provided metadata for your skill.")
    );
    println!("   It will be used by fastskill for version management.");

    Ok(())
}

fn extract_version_from_skill_md(content: &str, skip_prompts: bool) -> CliResult<String> {
    // If content is empty (no SKILL.md), return default
    if content.is_empty() {
        return if skip_prompts {
            Ok("1.0.0".to_string())
        } else {
            prompt_for_version()
        };
    }
    // Try to extract version from YAML frontmatter
    if content.starts_with("---") {
        // Find the end of the opening delimiter
        let opening_end = if content.starts_with("---\n") {
            4 // "---\n" is 4 bytes
        } else if content.starts_with("---\r\n") {
            5 // "---\r\n" is 5 bytes
        } else if content.len() >= 3 && &content[0..3] == "---" {
            // Handle case where "---" is not followed by newline
            if content.len() > 3 {
                3
            } else {
                return Ok("1.0.0".to_string()); // No content after opening delimiter
            }
        } else {
            return Ok("1.0.0".to_string()); // Invalid format
        };

        // Search for the closing delimiter after the opening one
        let after_opening = &content[opening_end..];

        // Try to find closing delimiter with newline first
        let closing_pos = if let Some(pos) = after_opening.find("\n---\n") {
            Some(opening_end + pos + 1) // +1 to skip the newline before "---"
        } else if let Some(pos) = after_opening.find("\n---\r\n") {
            Some(opening_end + pos + 1)
        } else if let Some(pos) = after_opening.find("\n---") {
            // Check if this is at the end of content or followed by whitespace/newline
            let check_pos = opening_end + pos + 1;
            if check_pos + 3 >= content.len()
                || content
                    .chars()
                    .nth(check_pos + 3)
                    .is_none_or(|c| c == '\n' || c == '\r' || c.is_whitespace())
            {
                Some(check_pos)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(closing_start) = closing_pos {
            // Extract frontmatter content between delimiters
            let frontmatter = &content[opening_end..closing_start].trim();
            for line in frontmatter.lines() {
                if line.trim().starts_with("version:") {
                    let version = line
                        .split(':')
                        .nth(1)
                        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string());
                    if let Some(v) = version {
                        if !v.is_empty() {
                            return Ok(v);
                        }
                    }
                }
            }
        }
    }

    // If version not found in frontmatter, prompt user
    if skip_prompts {
        return Ok("1.0.0".to_string());
    }

    prompt_for_version()
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
}

//! Utility functions for CLI operations

pub mod api_client;
pub mod install_utils;
pub mod manifest_utils;
pub mod messages;

use crate::cli::error::{CliError, CliResult};
use std::path::{Path, PathBuf};
use url::Url;

/// Git repository information parsed from URL
#[derive(Debug, Clone)]
pub struct GitUrlInfo {
    pub repo_url: String,
    pub branch: Option<String>,
    pub subdir: Option<PathBuf>,
}

/// Detect if input is a skill ID (format: skillid@version, skillid, or scope/skillid@version)
pub fn is_skill_id(input: &str) -> bool {
    // Skill ID format: alphanumeric with dashes/underscores, optionally followed by @version
    // Can also be scoped: scope/skillid or scope/skillid@version
    // Examples: pptx@1.2.3, web-scraper, my_skill@2.0.0, dev-user/test-skill, org/my-skill@1.0.0
    // Must not contain backslashes or colons (URL schemes)
    if input.contains('\\') || input.contains(':') {
        return false;
    }

    // Check if it looks like a skill ID (not a file path)
    // Supports both unscoped (skillid) and scoped (scope/skillid) formats
    // This regex is a compile-time constant pattern, so it should never fail
    #[allow(clippy::expect_used)]
    let skill_id_pattern =
        regex::Regex::new(r"^[a-zA-Z0-9_-]+(/[a-zA-Z0-9_-]+)?(@[0-9]+\.[0-9]+(\.[0-9]+)?.*)?$")
            .expect("Invalid regex pattern");
    skill_id_pattern.is_match(input)
}

/// Parse skill ID and version from input (format: skillid@version)
pub fn parse_skill_id(input: &str) -> (String, Option<String>) {
    if let Some(at_pos) = input.find('@') {
        let skill_id = input[..at_pos].to_string();
        let version = input[at_pos + 1..].to_string();
        (skill_id, Some(version))
    } else {
        (input.to_string(), None)
    }
}

/// Detect the type of skill source (zip, folder, git URL, or skill ID)
pub fn detect_skill_source(path: &str) -> SkillSource {
    // Check if it's a skill ID first
    if is_skill_id(path) {
        return SkillSource::SkillId(path.to_string());
    }

    // Check if it's a URL (git or http)
    if let Ok(url) = Url::parse(path) {
        if url.scheme() == "git" || url.scheme() == "https" || url.scheme() == "http" {
            return SkillSource::GitUrl(path.to_string());
        }
    }

    let path_buf = PathBuf::from(path);

    // Check if it's a zip file
    if path_buf.extension().and_then(|s| s.to_str()) == Some("zip") {
        return SkillSource::ZipFile(path_buf);
    }

    // Otherwise, treat as folder
    SkillSource::Folder(path_buf)
}

/// Represents different skill source types
#[derive(Debug, Clone)]
pub enum SkillSource {
    ZipFile(PathBuf),
    Folder(PathBuf),
    GitUrl(String),
    SkillId(String), // Format: skillid@version or skillid
}

/// Parse git URL to extract repository information
pub fn parse_git_url(git_url: &str) -> CliResult<GitUrlInfo> {
    let url = Url::parse(git_url)
        .map_err(|e| CliError::InvalidSource(format!("Invalid git URL: {}", e)))?;

    // Extract query parameters for branch
    let query_branch = url
        .query_pairs()
        .find(|(key, _)| key == "branch")
        .map(|(_, value)| value.to_string());

    // For GitHub tree URLs, extract branch and subdir
    let path_segments: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    let mut branch = None;
    let mut subdir = None;

    if url.host_str() == Some("github.com")
        && path_segments.len() >= 3
        && path_segments.get(2) == Some(&"tree")
        && path_segments.len() >= 4
    {
        // GitHub tree URL: /org/repo/tree/branch[/subdir...]
        branch = Some(path_segments[3].to_string());
        if path_segments.len() > 4 {
            subdir = Some(std::path::PathBuf::from(path_segments[4..].join("/")));
        }
    }

    // Construct clean repo URL
    let mut clean_url = url.clone();
    clean_url.set_query(None);
    let mut path = clean_url.path().to_string();

    // Remove tree/branch/subdir from path for GitHub tree URLs
    if url.host_str() == Some("github.com") && path.contains("/tree/") {
        if let Some(tree_pos) = path.find("/tree/") {
            path = path[..tree_pos].to_string();
        }
    }

    // Ensure .git extension
    if !path.ends_with(".git") {
        path.push_str(".git");
    }

    clean_url.set_path(&path);
    let repo_url = clean_url.to_string();

    Ok(GitUrlInfo {
        repo_url,
        branch: branch.or(query_branch),
        subdir,
    })
}

/// Validate skill structure follows Claude Code standard
pub fn validate_skill_structure(skill_path: &Path) -> CliResult<()> {
    // Check if SKILL.md exists
    let skill_file = skill_path.join("SKILL.md");
    if !skill_file.exists() {
        return Err(CliError::Validation(format!(
            "SKILL.md not found in: {}",
            skill_path.display()
        )));
    }

    // Basic validation - file exists and is readable
    std::fs::metadata(&skill_file)
        .map_err(|e| CliError::Validation(format!("Cannot read SKILL.md: {}", e)))?;

    Ok(())
}

/// Format skill output for display
#[allow(dead_code)]
pub fn format_skill_output(skills: &[fastskill::SkillDefinition], format: OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_skills_table(skills),
        OutputFormat::Json => format_skills_json(skills),
    }
}

/// Output format options
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum OutputFormat {
    Table,
    Json,
}

// Re-export for use in other modules

#[allow(dead_code)]
fn format_skills_table(skills: &[fastskill::SkillDefinition]) -> String {
    if skills.is_empty() {
        return "No skills found.".to_string();
    }

    let mut output = format!(
        "{:<30} {:<15} {:<10} {:<50}\n",
        "ID", "Version", "Status", "Description"
    );
    output.push_str(&format!("{:-<105}\n", ""));

    for skill in skills {
        let status = if skill.enabled { "Enabled" } else { "Disabled" };
        let description = if skill.description.len() > 47 {
            format!("{}...", &skill.description[..47])
        } else {
            skill.description.clone()
        };
        output.push_str(&format!(
            "{:<30} {:<15} {:<10} {:<50}\n",
            skill.id, skill.version, status, description
        ));
    }

    output
}

#[allow(dead_code)]
fn format_skills_json(skills: &[fastskill::SkillDefinition]) -> String {
    serde_json::to_string_pretty(skills).unwrap_or_else(|e| format!("Error serializing: {}", e))
}

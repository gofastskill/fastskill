//! Utility functions for CLI operations

pub mod api_client;
pub mod install_utils;
pub mod manifest_utils;
pub mod messages;

use crate::cli::config::get_skill_search_locations_for_display;
use crate::cli::error::{CliError, CliResult, SkillNotFoundMessage};
use std::path::{Path, PathBuf};

/// Convert ServiceError to CliError, mapping SkillNotFound to the rich message with searched paths and Try suggestions.
pub fn service_error_to_cli(e: fastskill::ServiceError, skill_storage_path: &Path) -> CliError {
    if let fastskill::ServiceError::SkillNotFound(id) = e {
        let searched_paths = get_skill_search_locations_for_display()
            .unwrap_or_else(|_| vec![(skill_storage_path.to_path_buf(), "project".to_string())]);
        return CliError::SkillNotFound(SkillNotFoundMessage::new(id, searched_paths));
    }
    CliError::Service(e)
}
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
    // Skill ID format: alphanumeric with dashes/underscores, optionally followed by @version or @tag
    // Can also be scoped: scope/skillid or scope/skillid@version
    // Examples: pptx@1.2.3, web-scraper, my_skill@2.0.0, aroff@teste, dev-user/test-skill, org/my-skill@1.0.0
    // Must not contain backslashes or colons (URL schemes)
    if input.contains('\\') || input.contains(':') {
        return false;
    }

    // Check if it looks like a skill ID (not a file path)
    // Supports both unscoped (skillid) and scoped (scope/skillid) formats
    // The @suffix can be a version (1.2.3) or any identifier (teste, label, etc.)
    // This regex is a compile-time constant pattern, so it should never fail
    #[allow(clippy::expect_used)]
    let skill_id_pattern =
        regex::Regex::new(r"^[a-zA-Z0-9_-]+(/[a-zA-Z0-9_-]+)?(@[a-zA-Z0-9_.-]+)?$")
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

/// Compatibility warning text; only shown when `verbose` is true.
const COMPATIBILITY_WARNING: &str = "No compatibility field specified";

/// Filter warnings for display: when not verbose, hide the compatibility warning.
fn filter_warnings(warnings: &[String], verbose: bool) -> Vec<String> {
    if verbose {
        warnings.to_vec()
    } else {
        warnings
            .iter()
            .filter(|w| *w != COMPATIBILITY_WARNING)
            .cloned()
            .collect()
    }
}

/// Validate skill structure follows Claude Code standard.
/// When `verbose` is false, the "No compatibility field specified" warning is not shown.
pub fn validate_skill_structure(skill_path: &Path, verbose: bool) -> CliResult<()> {
    use fastskill::validation::standard_validator::{StandardValidator, ValidationError};

    // Use StandardValidator for comprehensive AI Skill standard validation
    let result = StandardValidator::validate_skill_directory(skill_path);

    match result {
        Ok(validation_result) => {
            let warnings = filter_warnings(&validation_result.warnings, verbose);

            if !validation_result.is_valid {
                // Format validation errors for CLI display
                let error_messages: Vec<String> = validation_result
                    .errors
                    .iter()
                    .map(|e| match e {
                        ValidationError::InvalidNameFormat(msg) => {
                            format!("✗ Name format invalid: {}", msg)
                        }
                        ValidationError::NameMismatch { expected, actual } => format!(
                            "✗ Name mismatch: Directory '{}' doesn't match skill name '{}'",
                            actual, expected
                        ),
                        ValidationError::InvalidDescriptionLength(len) => format!(
                            "✗ Description length invalid: {} characters (must be 1-1024)",
                            len
                        ),
                        ValidationError::InvalidCompatibilityLength(len) => format!(
                            "✗ Compatibility field too long: {} characters (max 500)",
                            len
                        ),
                        ValidationError::MissingRequiredField(field) => {
                            format!("✗ Missing required field: {}", field)
                        }
                        ValidationError::InvalidFileReference(msg) => {
                            format!("✗ Invalid file reference: {}", msg)
                        }
                        ValidationError::InvalidDirectoryStructure(msg) => {
                            format!("✗ Invalid directory structure: {}", msg)
                        }
                        ValidationError::YamlParseError(msg) => {
                            format!("✗ YAML parsing error: {}", msg)
                        }
                    })
                    .collect();

                let warning_messages: Vec<String> =
                    warnings.iter().map(|w| format!("⚠ {}", w)).collect();

                let mut all_messages = error_messages;
                all_messages.extend(warning_messages);

                return Err(CliError::Validation(all_messages.join("\n")));
            }

            // Log warnings even if validation passes
            for warning in &warnings {
                eprintln!("⚠ {}", warning);
            }

            Ok(())
        }
        Err(e) => Err(CliError::Validation(format!("Validation failed: {:?}", e))),
    }
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

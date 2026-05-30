//! YAML frontmatter parser for skill files.
//!
//! Replaces the `split(':').nth(1)` pattern that silently truncated values
//! containing colons (e.g. `description: Tool: does X` → `" does X"`).

use serde::Deserialize;

/// Parsed skill frontmatter metadata
#[derive(Debug, Default, Deserialize)]
pub struct SkillMetadata {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
}

/// Errors that can occur when parsing frontmatter
#[derive(Debug, thiserror::Error)]
pub enum FrontmatterError {
    #[error("no YAML frontmatter block found")]
    Missing,
    #[error("YAML parse error: {0}")]
    Parse(#[from] serde_yaml::Error),
}

/// Parse YAML frontmatter from skill file content.
///
/// Extracts the `---`-delimited block at the top of the file and
/// deserializes it with `serde_yaml`. Values containing colons are
/// handled correctly (unlike the old `split(':').nth(1)` approach).
pub fn parse_skill_frontmatter(content: &str) -> Result<SkillMetadata, FrontmatterError> {
    let mut lines = content.lines();

    // First non-empty line must be `---`
    let first = lines.find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.trim() != "---" {
        return Err(FrontmatterError::Missing);
    }

    // Collect lines until the closing `---`
    let mut yaml_lines = Vec::new();
    let mut found_close = false;
    for line in lines {
        if line.trim() == "---" {
            found_close = true;
            break;
        }
        yaml_lines.push(line);
    }

    if !found_close {
        return Err(FrontmatterError::Missing);
    }

    let yaml = yaml_lines.join("\n");
    let metadata: SkillMetadata = serde_yaml::from_str(&yaml)?;
    Ok(metadata)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_basic() {
        let content = "---\nid: my-skill\nname: My Skill\nversion: 1.0.0\n---\n\n# Body";
        let meta = parse_skill_frontmatter(content).unwrap();
        assert_eq!(meta.id.as_deref(), Some("my-skill"));
        assert_eq!(meta.name.as_deref(), Some("My Skill"));
        assert_eq!(meta.version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn test_parse_frontmatter_colon_in_value() {
        // G9 acceptance criterion: description with colon must NOT be truncated
        let content = "---\ndescription: \"Tool: does X\"\n---\n";
        let meta = parse_skill_frontmatter(content).unwrap();
        assert_eq!(
            meta.description.as_deref(),
            Some("Tool: does X"),
            "description must preserve value after the first colon"
        );
    }

    #[test]
    fn test_parse_frontmatter_missing_returns_error() {
        let content = "# No frontmatter here\n\nJust a regular file.";
        let result = parse_skill_frontmatter(content);
        assert!(
            matches!(result, Err(FrontmatterError::Missing)),
            "missing frontmatter must return FrontmatterError::Missing"
        );
    }

    #[test]
    fn test_parse_frontmatter_unclosed_returns_error() {
        let content = "---\nid: my-skill\nname: My Skill\n";
        let result = parse_skill_frontmatter(content);
        assert!(
            matches!(result, Err(FrontmatterError::Missing)),
            "unclosed frontmatter block must return FrontmatterError::Missing"
        );
    }
}

//! YAML frontmatter validation for SKILL.md.

use crate::validation::result::ValidationResult;

pub(crate) fn bounds(content: &str) -> (bool, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let has_start = !lines.is_empty() && lines[0].trim().starts_with("---");
    let has_end = has_start
        && lines
            .iter()
            .enumerate()
            .skip(1)
            .any(|(_, line)| line.trim() == "---");
    (has_start, has_end)
}

pub(crate) fn validate_content(content: &str, mut result: ValidationResult) -> ValidationResult {
    let (has_start, has_end) = bounds(content);
    if !has_start {
        result = result.with_warning(
            "yaml_frontmatter",
            "SKILL.md should start with YAML frontmatter (---)",
        );
    } else if !has_end {
        result = result.with_warning(
            "yaml_frontmatter",
            "YAML frontmatter should be properly closed with ---",
        );
    }
    result
}

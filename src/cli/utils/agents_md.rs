//! AGENTS.md parsing and generation utilities

use tracing::debug;

/// Summary of a skill for sync purposes
#[derive(Debug, Clone)]
pub struct SkillSummary {
    pub id: String,
    #[allow(dead_code)]
    pub name: String,
    pub description: String,
    pub location: SkillLocation,
}

/// Location of a skill: project or global
#[derive(Debug, Clone, PartialEq)]
pub enum SkillLocation {
    Project,
    Global,
}

/// Parse current skill IDs from an existing AGENTS.md file
pub fn parse_current_skills(content: &str) -> Vec<String> {
    let mut skill_ids = Vec::new();
    let mut in_available_skills = false;
    let mut in_skill = false;

    for line in content.lines() {
        let line = line.trim();

        if line.contains("<available_skills>") {
            in_available_skills = true;
            continue;
        }

        if line.contains("</available_skills>") {
            in_available_skills = false;
            continue;
        }

        if in_available_skills && line.contains("<skill>") {
            in_skill = true;
            continue;
        }

        if line.contains("</skill>") {
            in_skill = false;
            continue;
        }

        if in_skill && line.contains("<name>") {
            if let Some(name_start) = line.find("<name>") {
                let name_part = &line[name_start + 6..];
                if let Some(name_end) = name_part.find("</name>") {
                    skill_ids.push(name_part[..name_end].trim().to_string());
                }
            }
        }
    }

    debug!("Parsed {} skills from AGENTS.md", skill_ids.len());
    skill_ids
}

/// Generate the XML skills section for AGENTS.md
pub fn generate_skills_xml(skills: &[SkillSummary]) -> String {
    let mut xml = String::new();

    xml.push_str("<skills_system priority=\"1\">\n\n");
    xml.push_str("## Available Skills\n\n");
    xml.push_str("<!-- SKILLS_TABLE_START -->\n");
    xml.push_str(
        r#"<usage>
When users ask you to perform tasks, check if any of the available skills below can help complete the task more effectively. Skills provide specialized capabilities and domain knowledge.

How to use skills:
- Invoke: Bash("fastskill read &lt;skill-id&gt;")
- The skill content will load with detailed instructions on how to complete the task
- Base directory provided in output for resolving bundled resources (references/, scripts/, assets/)

Usage notes:
- Only use skills listed in &lt;available_skills&gt; below
- Do not invoke a skill that is already loaded in your context
- Each skill invocation is stateless
</usage>

<available_skills>

"#,
    );

    for skill in skills {
        xml.push_str("<skill>\n");
        xml.push_str(&format!("  <name>{}</name>\n", xml_escape(&skill.id)));
        xml.push_str(&format!(
            "  <description>{}</description>\n",
            xml_escape(&skill.description)
        ));
        let location_str = match skill.location {
            SkillLocation::Project => "project",
            SkillLocation::Global => "global",
        };
        xml.push_str(&format!("  <location>{}</location>\n", location_str));
        xml.push_str("</skill>\n\n");
    }

    xml.push_str("</available_skills>\n");
    xml.push_str("<!-- SKILLS_TABLE_END -->\n\n");
    xml.push_str("</skills_system>");

    xml
}

/// Replace or append the skills section in AGENTS.md content
pub fn replace_skills_section(content: &str, new_section: &str) -> String {
    if let Some(start) = content.find("<skills_system") {
        if let Some(end) = content.find("</skills_system>") {
            let end = end + "</skills_system>".len();
            let mut new_content = String::with_capacity(content.len());
            new_content.push_str(&content[..start]);
            new_content.push_str(new_section);
            new_content.push_str(&content[end..]);
            return new_content;
        }
    }

    if let Some(start) = content.find("<!-- SKILLS_TABLE_START -->") {
        if let Some(end) = content.find("<!-- SKILLS_TABLE_END -->") {
            let end = end + "<!-- SKILLS_TABLE_END -->".len();
            let mut new_content = String::with_capacity(content.len());
            new_content.push_str(&content[..start]);
            new_content.push_str(new_section);
            new_content.push_str(&content[end..]);
            return new_content;
        }
    }

    let mut new_content = String::with_capacity(content.len() + new_section.len() + 2);
    if !content.is_empty() && !content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(content);
    if !content.is_empty() {
        new_content.push_str("\n\n");
    }
    new_content.push_str(new_section);
    new_content
}

/// Remove the skills section from AGENTS.md content
pub fn remove_skills_section(content: &str) -> String {
    if let Some(start) = content.find("<skills_system") {
        if let Some(end) = content.find("</skills_system>") {
            let end = end + "</skills_system>".len();
            let mut new_content = String::with_capacity(content.len());
            new_content.push_str(&content[..start]);
            new_content.push_str(&content[end..]);
            return new_content;
        }
    }

    if let Some(start) = content.find("<!-- SKILLS_TABLE_START -->") {
        if let Some(end) = content.find("<!-- SKILLS_TABLE_END -->") {
            let end = end + "<!-- SKILLS_TABLE_END -->".len();
            let mut new_content = String::with_capacity(content.len());
            new_content.push_str(&content[..start]);
            new_content.push_str(&content[end..]);
            return new_content;
        }
    }

    content.to_string()
}

/// Escape special XML characters
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_current_skills_empty() {
        let content = "# AGENTS.md\n\nSome content\n";
        let skills = parse_current_skills(content);
        assert!(skills.is_empty());
    }

    #[test]
    fn test_parse_current_skills_with_skills() {
        let content = r#"<skills_system priority="1">
<available_skills>
<skill>
  <name>skill1</name>
  <description>Test skill 1</description>
  <location>project</location>
</skill>
<skill>
  <name>skill2</name>
  <description>Test skill 2</description>
  <location>global</location>
</skill>
</available_skills>
</skills_system>"#;
        let skills = parse_current_skills(content);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0], "skill1");
        assert_eq!(skills[1], "skill2");
    }

    #[test]
    fn test_generate_skills_xml() {
        let skills = vec![
            SkillSummary {
                id: "skill1".to_string(),
                name: "Skill 1".to_string(),
                description: "Test skill 1".to_string(),
                location: SkillLocation::Project,
            },
            SkillSummary {
                id: "skill2".to_string(),
                name: "Skill 2".to_string(),
                description: "Test skill 2".to_string(),
                location: SkillLocation::Global,
            },
        ];

        let xml = generate_skills_xml(&skills);

        assert!(xml.contains("<skills_system priority=\"1\">"));
        assert!(xml.contains("<name>skill1</name>"));
        assert!(xml.contains("<name>skill2</name>"));
        assert!(xml.contains("<description>Test skill 1</description>"));
        assert!(xml.contains("<description>Test skill 2</description>"));
        assert!(xml.contains("<location>project</location>"));
        assert!(xml.contains("<location>global</location>"));
    }

    #[test]
    fn test_replace_skills_section_existing() {
        let content = r#"# AGENTS.md

Some content

<skills_system priority="1">
<available_skills>
<skill>
  <name>old-skill</name>
</skill>
</available_skills>
</skills_system>

More content"#;

        let new_section = r#"<skills_system priority="1">
<available_skills>
<skill>
  <name>new-skill</name>
</skill>
</available_skills>
</skills_system>"#;

        let result = replace_skills_section(content, new_section);
        assert!(!result.contains("old-skill"));
        assert!(result.contains("new-skill"));
        assert!(result.contains("More content"));
    }

    #[test]
    fn test_replace_skills_section_new() {
        let content = "# AGENTS.md\n\nSome content\n";
        let new_section = r#"<skills_system priority="1">
<available_skills>
</available_skills>
</skills_system>"#;

        let result = replace_skills_section(content, new_section);
        assert!(result.contains("<skills_system"));
        assert!(result.contains("Some content"));
    }

    #[test]
    fn test_remove_skills_section() {
        let content = r#"# AGENTS.md

Some content

<skills_system priority="1">
<available_skills>
<skill>
  <name>skill1</name>
</skill>
</available_skills>
</skills_system>

More content"#;

        let result = remove_skills_section(content);
        assert!(!result.contains("<skills_system"));
        assert!(!result.contains("skill1"));
        assert!(result.contains("Some content"));
        assert!(result.contains("More content"));
    }

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("test"), "test");
        assert_eq!(xml_escape("test <tag>"), "test &lt;tag&gt;");
        assert_eq!(xml_escape("test &amp;"), "test &amp;amp;");
        assert_eq!(xml_escape("test \"quoted\""), "test &quot;quoted&quot;");
    }
}

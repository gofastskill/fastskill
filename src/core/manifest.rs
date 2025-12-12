//! Skills manifest management for declarative skill control

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Main skills manifest structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsManifest {
    pub metadata: ManifestMetadata,
    #[serde(default)]
    pub skills: Vec<SkillEntry>,
}

/// Manifest metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMetadata {
    pub version: String,
}

/// Skill entry in the manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    pub id: String,
    pub source: SkillSource,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub editable: bool,
}

/// Source specification for skills
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SkillSource {
    #[serde(rename = "git")]
    Git {
        url: String,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        tag: Option<String>,
        #[serde(default)]
        subdir: Option<PathBuf>,
    },
    #[serde(rename = "source")]
    Source {
        name: String,
        skill: String,
        #[serde(default)]
        version: Option<String>,
    },
    #[serde(rename = "local")]
    Local {
        path: PathBuf,
        #[serde(default)]
        editable: bool,
    },
    #[serde(rename = "zip-url")]
    ZipUrl {
        base_url: String,
        #[serde(default)]
        version: Option<String>,
    },
}

impl SkillsManifest {
    /// Load manifest from TOML file
    pub fn load_from_file(path: &Path) -> Result<Self, ManifestError> {
        if !path.exists() {
            return Err(ManifestError::NotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path).map_err(ManifestError::Io)?;

        let manifest: SkillsManifest =
            toml::from_str(&content).map_err(|e| ManifestError::Parse(e.to_string()))?;

        Ok(manifest)
    }

    /// Save manifest to TOML file
    pub fn save_to_file(&self, path: &Path) -> Result<(), ManifestError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| ManifestError::Serialize(e.to_string()))?;

        std::fs::write(path, content).map_err(ManifestError::Io)?;

        Ok(())
    }

    /// Get skills filtered by groups (like Poetry groups)
    pub fn get_skills_for_groups(
        &self,
        exclude_groups: Option<&[String]>,
        only_groups: Option<&[String]>,
    ) -> Vec<&SkillEntry> {
        self.skills
            .iter()
            .filter(|skill| {
                // If only_groups specified, skill must be in one of those groups
                if let Some(only) = only_groups {
                    if skill.groups.is_empty() && !only.is_empty() {
                        return false;
                    }
                    if !skill.groups.is_empty() {
                        return skill.groups.iter().any(|g| only.contains(g));
                    }
                }

                // If exclude_groups specified, skill must not be in any excluded group
                if let Some(exclude) = exclude_groups {
                    return !skill.groups.iter().any(|g| exclude.contains(g));
                }

                true
            })
            .collect()
    }

    /// Get all skills (no filtering)
    pub fn get_all_skills(&self) -> Vec<&SkillEntry> {
        self.skills.iter().collect()
    }

    /// Add a skill to the manifest
    pub fn add_skill(&mut self, skill: SkillEntry) {
        self.skills.push(skill);
    }

    /// Remove a skill from the manifest
    pub fn remove_skill(&mut self, skill_id: &str) -> bool {
        if let Some(pos) = self.skills.iter().position(|s| s.id == skill_id) {
            self.skills.remove(pos);
            return true;
        }
        false
    }
}

// ============================================================================
// Skill Project TOML structures (skill-project.toml format)
// ============================================================================

/// Root structure for skill-project.toml file
/// Contains both project metadata and dependencies
/// Works in both project-level (skill consumer) and skill-level (skill author) contexts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProjectToml {
    /// Optional metadata section (required for skill-level, optional for project-level)
    #[serde(default)]
    pub metadata: Option<MetadataSection>,
    /// Optional dependencies section (required for project-level, optional for skill-level)
    #[serde(default)]
    pub dependencies: Option<DependenciesSection>,
    /// Optional tool configuration (project-level only)
    #[serde(default)]
    #[serde(rename = "tool")]
    pub tool: Option<ToolSection>,
}

/// Metadata section for skill or project metadata
/// Contains skill author information for skill-level, project documentation for project-level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSection {
    /// Required for skill-level, optional for project-level
    pub id: Option<String>,
    /// Required for skill-level, optional for project-level
    pub version: Option<String>,
    /// Optional: Description
    #[serde(default)]
    pub description: Option<String>,
    /// Optional: Author name
    #[serde(default)]
    pub author: Option<String>,
    /// Optional: Tags array
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Optional: Capabilities array
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    /// Optional: Download URL
    #[serde(default)]
    pub download_url: Option<String>,
    /// Optional: Project name (project-level only)
    #[serde(default)]
    pub name: Option<String>,
}

/// Dependencies section containing skill dependencies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependenciesSection {
    /// Map of skill ID to dependency specification
    #[serde(flatten)]
    pub dependencies: HashMap<String, DependencySpec>,
}

/// Dependency specification - can be a simple version string or inline table with source details
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Simple version string: "1.0.0"
    Version(String),
    /// Inline table with source details
    Inline {
        source: DependencySource,
        #[serde(flatten)]
        source_specific: SourceSpecificFields,
        #[serde(default)]
        groups: Option<Vec<String>>,
        #[serde(default)]
        editable: Option<bool>,
    },
}

/// Dependency source type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencySource {
    #[serde(rename = "git")]
    Git,
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "zip-url")]
    ZipUrl,
    #[serde(rename = "source")]
    Source,
}

/// Source-specific fields for dependency specifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSpecificFields {
    /// For git source
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// For local source
    #[serde(default)]
    pub path: Option<String>,
    /// For source source
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub skill: Option<String>,
    /// For zip-url source
    #[serde(default)]
    pub zip_url: Option<String>,
    /// Version (for source source)
    #[serde(default)]
    pub version: Option<String>,
}

/// Tool section containing tool-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSection {
    #[serde(default)]
    pub fastskill: Option<FastSkillToolConfig>,
}

/// FastSkill tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastSkillToolConfig {
    /// Optional skills storage directory override
    #[serde(default)]
    pub skills_directory: Option<PathBuf>,
    /// Optional repository configuration
    #[serde(default)]
    pub repositories: Option<Vec<RepositoryDefinition>>,
}

/// Repository definition with name, type, priority, authentication, and connection details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryDefinition {
    /// Repository name (unique identifier)
    pub name: String,
    /// Repository type
    pub r#type: RepositoryType,
    /// Priority (lower number = higher priority)
    pub priority: u32,
    /// Connection details (type-specific)
    #[serde(flatten)]
    pub connection: RepositoryConnection,
    /// Authentication configuration
    #[serde(default)]
    pub auth: Option<AuthConfig>,
}

/// Repository type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepositoryType {
    #[serde(rename = "http-registry")]
    HttpRegistry,
    #[serde(rename = "git-marketplace")]
    GitMarketplace,
    #[serde(rename = "zip-url")]
    ZipUrl,
    #[serde(rename = "local")]
    Local,
}

/// Repository connection details (type-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RepositoryConnection {
    HttpRegistry {
        index_url: String,
    },
    GitMarketplace {
        url: String,
        #[serde(default)]
        branch: Option<String>,
    },
    ZipUrl {
        zip_url: String,
    },
    Local {
        path: String,
    },
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub r#type: AuthType,
    #[serde(default)]
    pub env_var: Option<String>,
}

/// Authentication type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthType {
    #[serde(rename = "pat")]
    Pat,
}

/// Project context enum for context detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectContext {
    /// Project-level context (skill consumer)
    Project,
    /// Skill-level context (skill author)
    Skill,
    /// Ambiguous context (requires content-based detection)
    Ambiguous,
}

/// File resolution result
#[derive(Debug, Clone)]
pub struct FileResolutionResult {
    /// Resolved file path
    pub path: PathBuf,
    /// Context detected for the file
    pub context: ProjectContext,
    /// Whether file was found or created
    pub found: bool,
}

impl SkillProjectToml {
    /// Load skill-project.toml from file
    pub fn load_from_file(path: &Path) -> Result<Self, ManifestError> {
        if !path.exists() {
            return Err(ManifestError::NotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path).map_err(ManifestError::Io)?;

        let project: SkillProjectToml = toml::from_str(&content).map_err(|e| {
            // T066: Enhanced TOML error message with line numbers
            let error_msg = e.to_string();
            // Extract line number if available
            let line_info = if let Some(line_start) = error_msg.find("line ") {
                let after_line = &error_msg[line_start + 5..];
                let line_end = after_line
                    .find(|c: char| !c.is_ascii_digit() && c != ',')
                    .unwrap_or(after_line.len());
                if let Ok(line) = after_line[..line_end].parse::<usize>() {
                    format!("line {}", line)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            if !line_info.is_empty() {
                ManifestError::Parse(format!("TOML syntax error at {}: {}", line_info, error_msg))
            } else {
                ManifestError::Parse(format!("TOML syntax error: {}", error_msg))
            }
        })?;

        Ok(project)
    }

    /// Save skill-project.toml to file
    pub fn save_to_file(&self, path: &Path) -> Result<(), ManifestError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| ManifestError::Serialize(e.to_string()))?;

        std::fs::write(path, content).map_err(ManifestError::Io)?;

        Ok(())
    }

    /// Validate required sections based on context
    /// T060: Enhanced error messages with context information
    pub fn validate_for_context(&self, context: ProjectContext) -> Result<(), String> {
        match context {
            ProjectContext::Skill => {
                // Skill-level: metadata with id and version required
                if let Some(ref metadata) = self.metadata {
                    if metadata.id.as_ref().is_none_or(|id| id.is_empty()) {
                        return Err(
                            "Skill-level skill-project.toml (in directory with SKILL.md) requires [metadata].id field. \
                            Add 'id = \"your-skill-id\"' to the [metadata] section.".to_string()
                        );
                    }
                    if metadata.version.as_ref().is_none_or(|v| v.is_empty()) {
                        return Err(
                            "Skill-level skill-project.toml (in directory with SKILL.md) requires [metadata].version field. \
                            Add 'version = \"1.0.0\"' to the [metadata] section.".to_string()
                        );
                    }
                } else {
                    return Err(
                        "Skill-level skill-project.toml (in directory with SKILL.md) requires [metadata] section with 'id' and 'version' fields. \
                        This file is used for skill author metadata.".to_string()
                    );
                }
            }
            ProjectContext::Project => {
                // Project-level: dependencies required
                if self.dependencies.is_none() {
                    return Err(
                        "Project-level skill-project.toml (at project root) requires [dependencies] section. \
                        Add '[dependencies]' section to manage skill dependencies. \
                        Use 'fastskill add <skill-id>' to add skills.".to_string()
                    );
                }
            }
            ProjectContext::Ambiguous => {
                // Ambiguous: cannot validate without clear context
                // T059: Provide helpful error message for ambiguous context
                return Err(
                    "Cannot determine context for skill-project.toml. \
                    The file location and content are ambiguous. \
                    For skill-level: ensure SKILL.md exists in the same directory and add [metadata] section with 'id' and 'version'. \
                    For project-level: ensure file is at project root and add [dependencies] section.".to_string()
                );
            }
        }
        Ok(())
    }

    /// Convert SkillProjectToml dependencies to SkillEntry format for installation
    /// T027: Helper to convert unified format to legacy format for compatibility
    pub fn to_skill_entries(&self) -> Result<Vec<SkillEntry>, String> {
        let mut entries = Vec::new();

        if let Some(ref deps_section) = self.dependencies {
            for (skill_id, dep_spec) in &deps_section.dependencies {
                let (source, version, groups, editable) = match dep_spec {
                    DependencySpec::Version(version_str) => {
                        // Version-only dependency - treat as source-based
                        (
                            SkillSource::Source {
                                name: "default".to_string(),
                                skill: skill_id.clone(),
                                version: Some(version_str.clone()),
                            },
                            Some(version_str.clone()),
                            Vec::new(),
                            false,
                        )
                    }
                    DependencySpec::Inline {
                        source,
                        source_specific,
                        groups,
                        editable,
                    } => {
                        let source = match source {
                            DependencySource::Git => {
                                let url = source_specific.url.clone().ok_or_else(|| {
                                    format!("Git source requires 'url' field for {}", skill_id)
                                })?;
                                SkillSource::Git {
                                    url,
                                    branch: source_specific.branch.clone(),
                                    tag: None,
                                    subdir: None,
                                }
                            }
                            DependencySource::Local => {
                                let path = source_specific.path.clone().ok_or_else(|| {
                                    format!("Local source requires 'path' field for {}", skill_id)
                                })?;
                                SkillSource::Local {
                                    path: PathBuf::from(path),
                                    editable: editable.unwrap_or(false),
                                }
                            }
                            DependencySource::ZipUrl => {
                                let zip_url = source_specific.zip_url.clone().ok_or_else(|| {
                                    format!(
                                        "ZipUrl source requires 'zip_url' field for {}",
                                        skill_id
                                    )
                                })?;
                                SkillSource::ZipUrl {
                                    base_url: zip_url,
                                    version: source_specific.version.clone(),
                                }
                            }
                            DependencySource::Source => {
                                let name = source_specific.name.clone().ok_or_else(|| {
                                    format!("Source source requires 'name' field for {}", skill_id)
                                })?;
                                let skill = source_specific.skill.clone().ok_or_else(|| {
                                    format!("Source source requires 'skill' field for {}", skill_id)
                                })?;
                                SkillSource::Source {
                                    name,
                                    skill,
                                    version: source_specific.version.clone(),
                                }
                            }
                        };
                        (
                            source,
                            source_specific.version.clone(),
                            groups.clone().unwrap_or_default(),
                            editable.unwrap_or(false),
                        )
                    }
                };

                entries.push(SkillEntry {
                    id: skill_id.clone(),
                    source,
                    version,
                    groups,
                    editable,
                });
            }
        }

        Ok(entries)
    }
}

/// Manifest-related errors
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("Manifest file not found: {0}")]
    NotFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Serialize error: {0}")]
    Serialize(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_manifest_parsing() {
        let toml_content = r#"
            [metadata]
            version = "1.0.0"

            [[skills]]
            id = "web-scraper"
            source = { type = "git", url = "https://github.com/org/repo.git", branch = "main" }

            [[skills]]
            id = "dev-tools"
            source = { type = "git", url = "https://github.com/org/dev-tools.git" }
            groups = ["dev"]

            [[skills]]
            id = "monitoring"
            source = { type = "source", name = "team-tools", skill = "monitoring", version = "2.1.0" }
            groups = ["prod"]
        "#;

        let manifest: SkillsManifest = toml::from_str(toml_content).unwrap();

        assert_eq!(manifest.metadata.version, "1.0.0");
        assert_eq!(manifest.skills.len(), 3);

        // Check all skills
        let all_skills = manifest.get_all_skills();
        assert_eq!(all_skills.len(), 3);

        // Check skills without dev group
        let without_dev = manifest.get_skills_for_groups(Some(&["dev".to_string()]), None);
        assert_eq!(without_dev.len(), 2); // web-scraper and monitoring

        // Check only prod group
        let only_prod = manifest.get_skills_for_groups(None, Some(&["prod".to_string()]));
        assert_eq!(only_prod.len(), 1); // monitoring
    }

    #[test]
    fn test_skill_source_variants() {
        // Test Git source
        let git_source = SkillSource::Git {
            url: "https://github.com/org/repo.git".to_string(),
            branch: Some("main".to_string()),
            tag: None,
            subdir: None,
        };

        // Test Source reference
        let source_ref = SkillSource::Source {
            name: "team-tools".to_string(),
            skill: "monitoring".to_string(),
            version: Some("2.1.0".to_string()),
        };

        // Test Local source
        let _local_source = SkillSource::Local {
            path: PathBuf::from("./local-skills"),
            editable: false,
        };

        // Test ZipUrl source
        let _zip_source = SkillSource::ZipUrl {
            base_url: "https://skills.example.com/".to_string(),
            version: None,
        };

        // Verify they serialize correctly
        let git_toml = toml::to_string(&git_source).unwrap();
        assert!(git_toml.contains("type = \"git\""));

        let source_toml = toml::to_string(&source_ref).unwrap();
        assert!(source_toml.contains("type = \"source\""));
    }
}

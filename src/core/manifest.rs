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
    #[serde(default)]
    pub proxy: Option<ProxyManifestConfig>,
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

/// Proxy configuration in skills manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyManifestConfig {
    #[serde(default = "default_proxy_port")]
    pub port: u16,
    #[serde(default = "default_target_url")]
    pub target_url: String,
    #[serde(default)]
    pub enable_dynamic: bool,
    #[serde(default)]
    pub hardcoded_skills: Option<HardcodedSkillsConfig>,
    #[serde(default)]
    pub dynamic_skills: Option<DynamicSkillsConfig>,
    #[serde(default)]
    pub openai: Option<OpenAIManifestConfig>,
    #[serde(default)]
    pub anthropic: Option<AnthropicManifestConfig>,
}

/// Hardcoded skills configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardcodedSkillsConfig {
    pub skills: Vec<String>,
}

/// Dynamic skills configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSkillsConfig {
    #[serde(default = "default_min_relevance")]
    pub min_relevance: f64,
    #[serde(default = "default_max_skills")]
    pub max_skills: usize,
}

/// OpenAI proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIManifestConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_openai_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub model_mapping: HashMap<String, String>,
}

/// Anthropic proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicManifestConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_anthropic_endpoint")]
    pub endpoint: String,
}

// Default value functions for serde
fn default_proxy_port() -> u16 {
    8081
}

fn default_target_url() -> String {
    "https://api.anthropic.com".to_string()
}

fn default_min_relevance() -> f64 {
    0.7
}

fn default_max_skills() -> usize {
    5
}

fn default_true() -> bool {
    true
}

fn default_openai_endpoint() -> String {
    "/v1/chat/completions".to_string()
}

fn default_anthropic_endpoint() -> String {
    "/v1/messages".to_string()
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProjectToml {
    /// Optional metadata section for skill author information
    #[serde(default)]
    pub metadata: Option<MetadataSection>,
    /// Optional dependencies section for skill dependencies
    #[serde(default)]
    pub dependencies: Option<DependenciesSection>,
}

/// Metadata section for skill author information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSection {
    /// Required skill ID (must not contain slashes or scope, e.g., "my-skill")
    pub id: String,
    /// Required semantic version (e.g., "1.0.0")
    pub version: String,
    /// Optional skill description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional author name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Optional tags for categorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Optional capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    /// Optional download URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
}

/// Dependencies section - HashMap of skill ID to dependency specification
pub type DependenciesSection = HashMap<String, DependencySpec>;

/// Dependency specification - can be a simple version string or inline table
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Simple version string: "1.0.0"
    Version(String),
    /// Inline table with source and version: { source = "registry", version = "1.0.0" }
    InlineTable {
        /// Optional source/registry name
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        /// Required version
        version: String,
    },
}

impl SkillProjectToml {
    /// Load skill-project.toml from file
    pub fn load_from_file(path: &Path) -> Result<Self, ManifestError> {
        if !path.exists() {
            return Err(ManifestError::NotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path).map_err(ManifestError::Io)?;

        let project: SkillProjectToml =
            toml::from_str(&content).map_err(|e| ManifestError::Parse(e.to_string()))?;

        Ok(project)
    }

    /// Save skill-project.toml to file
    pub fn save_to_file(&self, path: &Path) -> Result<(), ManifestError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| ManifestError::Serialize(e.to_string()))?;

        std::fs::write(path, content).map_err(ManifestError::Io)?;

        Ok(())
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

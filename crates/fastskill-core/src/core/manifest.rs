//! Skills manifest management for declarative skill control

use crate::core::origin::Origin;
use crate::core::version::VersionConstraint;
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
    pub origin: Origin,
    #[serde(default)]
    pub groups: Vec<String>,
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

        crate::utils::atomic_write(path, content.as_bytes()).map_err(ManifestError::Io)?;

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
                // Exclusion always takes precedence: if skill is in exclude_groups, exclude it.
                if let Some(exclude) = exclude_groups {
                    if skill.groups.iter().any(|g| exclude.contains(g)) {
                        return false;
                    }
                }

                // If only_groups specified, skill must be in at least one of those groups.
                if let Some(only) = only_groups {
                    if only.is_empty() {
                        return true;
                    }
                    if skill.groups.is_empty() {
                        return false;
                    }
                    return skill.groups.iter().any(|g| only.contains(g));
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

/// Dependency specification - can be a simple version string or inline table with origin details
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Simple version string: "1.0.0"
    Version(String),
    /// Inline table with an explicit `Origin`
    Inline {
        origin: Origin,
        #[serde(default)]
        groups: Option<Vec<String>>,
    },
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
    /// Optional embedding configuration
    #[serde(default)]
    pub embedding: Option<EmbeddingConfigToml>,
    /// Optional repository configuration
    #[serde(default)]
    pub repositories: Option<Vec<RepositoryDefinition>>,
    /// Optional HTTP server configuration
    #[serde(default)]
    pub server: Option<HttpServerConfigToml>,
    /// Maximum dependency depth for recursive install (default: 5)
    #[serde(default = "default_install_depth")]
    pub install_depth: u32,
    /// Skip transitive dependency resolution entirely (default: false)
    #[serde(default)]
    pub skip_transitive: bool,
    /// Optional evaluation configuration
    #[serde(default)]
    pub eval: Option<EvalConfigToml>,
    /// Auto-run reindex after mutating commands when embedding is configured (default: true)
    #[serde(default = "default_auto_reindex")]
    pub auto_reindex: bool,
}

/// Evaluation configuration in TOML format ([tool.fastskill.eval])
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalConfigToml {
    /// Path to prompts CSV file (relative to skill project root)
    pub prompts: PathBuf,
    /// Optional path to checks TOML file
    #[serde(default)]
    pub checks: Option<PathBuf>,
    /// Timeout in seconds for each eval case execution
    #[serde(default = "default_eval_timeout_seconds")]
    pub timeout_seconds: u64,
    /// Trials per case (default: 1)
    #[serde(default = "default_trials_per_case")]
    pub trials_per_case: u32,
    /// Optional maximum parallelism for trials within one case (default: CPU cores)
    #[serde(default)]
    pub parallel: Option<u32>,
    /// Pass threshold for trial aggregation (0.0-1.0, default: 1.0)
    #[serde(default = "default_pass_threshold")]
    pub pass_threshold: f64,
    /// When true, `eval run` / `eval validate --agent` fail fast if the agent CLI is not available
    #[serde(default = "default_fail_on_missing_agent")]
    pub fail_on_missing_agent: bool,
}

fn default_eval_timeout_seconds() -> u64 {
    900
}

fn default_trials_per_case() -> u32 {
    1
}

fn default_pass_threshold() -> f64 {
    1.0
}

fn default_fail_on_missing_agent() -> bool {
    true
}

fn default_install_depth() -> u32 {
    5
}

fn default_auto_reindex() -> bool {
    true
}

/// HTTP server configuration in TOML format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServerConfigToml {
    /// List of origins allowed for CORS (required when server is used)
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Optional: allow list of request headers (default: ["Content-Type", "Authorization"])
    #[serde(default = "default_allowed_headers_toml")]
    pub allowed_headers: Vec<String>,
}

fn default_allowed_headers_toml() -> Vec<String> {
    vec!["Content-Type".to_string(), "Authorization".to_string()]
}

/// Embedding configuration in TOML format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfigToml {
    pub openai_base_url: String,
    pub embedding_model: String,
    #[serde(default)]
    pub index_path: Option<PathBuf>,
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

        // Canonicalize path to prevent traversal attacks
        let safe_path = path.canonicalize().map_err(ManifestError::Io)?;

        let content = std::fs::read_to_string(&safe_path).map_err(ManifestError::Io)?;

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

            // Pre-Origin dependency entries used `source = "git|local|zip-url|source"`
            // with flat sibling fields; those keys no longer parse. Point the author
            // at the new `origin = { type = … }` shape instead of a raw serde error.
            let origin_hint = if content.contains("source = \"") {
                "\n\nhint: this manifest looks like the pre-Origin `[dependencies]` format \
                 (`source = \"git\"` + flat fields). Replace each dependency's `source`/flat \
                 fields with `origin = { type = \"git|local|zip-url|repository\", … }`."
            } else {
                ""
            };

            if !line_info.is_empty() {
                ManifestError::Parse(format!(
                    "TOML syntax error at {}: {}{}",
                    line_info, error_msg, origin_hint
                ))
            } else {
                ManifestError::Parse(format!("TOML syntax error: {}{}", error_msg, origin_hint))
            }
        })?;

        Ok(project)
    }

    /// Save skill-project.toml to file
    pub fn save_to_file(&self, path: &Path) -> Result<(), ManifestError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| ManifestError::Serialize(e.to_string()))?;

        crate::utils::atomic_write(path, content.as_bytes()).map_err(ManifestError::Io)?;

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

                // Project-level: skills_directory in [tool.fastskill] required
                let has_skills_directory = self
                    .tool
                    .as_ref()
                    .and_then(|t| t.fastskill.as_ref())
                    .and_then(|f| f.skills_directory.as_ref())
                    .is_some();

                if !has_skills_directory {
                    return Err(
                        "Project-level skill-project.toml requires [tool.fastskill] with skills_directory. \
                        Run 'fastskill init --skills-dir <path>' or add [tool.fastskill] with skills_directory = \"...\".".to_string()
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
                let (origin, groups) = match dep_spec {
                    DependencySpec::Version(version_str) => {
                        // Version-only dependency: resolved against the "default" repository,
                        // preserving today's implicit-source behavior.
                        let constraint = VersionConstraint::parse(version_str).map_err(|e| {
                            format!("Invalid version '{}' for {}: {}", version_str, skill_id, e)
                        })?;
                        (
                            Origin::Repository {
                                repo: "default".to_string(),
                                skill: skill_id.clone(),
                                version: Some(constraint),
                            },
                            Vec::new(),
                        )
                    }
                    DependencySpec::Inline { origin, groups } => {
                        (origin.clone(), groups.clone().unwrap_or_default())
                    }
                };

                entries.push(SkillEntry {
                    id: skill_id.clone(),
                    origin,
                    groups,
                });
            }
        }

        Ok(entries)
    }
}

/// Canonical conversion from manifest RepositoryDefinition to the runtime type.
/// This is the single authoritative definition; all call-sites MUST use this impl.
impl From<&RepositoryDefinition> for crate::core::repository::RepositoryDefinition {
    fn from(r: &RepositoryDefinition) -> Self {
        use crate::core::repository::{
            RepositoryAuth, RepositoryConfig, RepositoryDefinition as RepoDef, RepositoryType,
        };

        let repo_type = match r.r#type {
            crate::core::manifest::RepositoryType::HttpRegistry => RepositoryType::HttpRegistry,
            crate::core::manifest::RepositoryType::GitMarketplace => RepositoryType::GitMarketplace,
            crate::core::manifest::RepositoryType::ZipUrl => RepositoryType::ZipUrl,
            crate::core::manifest::RepositoryType::Local => RepositoryType::Local,
        };

        let config = match &r.connection {
            RepositoryConnection::HttpRegistry { index_url } => RepositoryConfig::HttpRegistry {
                index_url: index_url.clone(),
            },
            RepositoryConnection::GitMarketplace { url, branch } => {
                RepositoryConfig::GitMarketplace {
                    url: url.clone(),
                    branch: branch.clone(),
                    tag: None,
                }
            }
            RepositoryConnection::ZipUrl { zip_url } => RepositoryConfig::ZipUrl {
                base_url: zip_url.clone(),
            },
            RepositoryConnection::Local { path } => RepositoryConfig::Local {
                path: std::path::PathBuf::from(path),
            },
        };

        let auth = r.auth.as_ref().map(|a| match a.r#type {
            AuthType::Pat => RepositoryAuth::Pat {
                env_var: a.env_var.clone().unwrap_or_else(|| "PAT_TOKEN".to_string()),
            },
        });

        RepoDef {
            name: r.name.clone(),
            repo_type,
            priority: r.priority,
            config,
            auth,
            storage: None,
        }
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
    fn pre_origin_manifest_gives_actionable_hint() {
        // A pre-Origin `[dependencies]` entry (`source = "git"` + flat fields) no
        // longer parses; the error must point the author at the `origin = {…}` shape.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skill-project.toml");
        std::fs::write(
            &path,
            "[metadata]\nid = \"x\"\nversion = \"1.0.0\"\ndescription = \"d\"\n\n\
             [dependencies.old]\nsource = \"git\"\nurl = \"https://example.com/x.git\"\n",
        )
        .unwrap();
        let err = SkillProjectToml::load_from_file(&path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("origin = { type"),
            "expected migration hint, got: {msg}"
        );
    }

    #[test]
    fn test_manifest_parsing() {
        let toml_content = r#"
            [metadata]
            version = "1.0.0"

            [[skills]]
            id = "web-scraper"
            origin = { type = "git", url = "https://github.com/org/repo.git", ref = { branch = "main" } }

            [[skills]]
            id = "dev-tools"
            origin = { type = "git", url = "https://github.com/org/dev-tools.git" }
            groups = ["dev"]

            [[skills]]
            id = "monitoring"
            origin = { type = "repository", repo = "team-tools", skill = "monitoring", version = "=2.1.0" }
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
    fn test_origin_variants_serialize_as_expected() {
        // Test Git origin
        let git_origin = Origin::Git {
            url: "https://github.com/org/repo.git".to_string(),
            r#ref: crate::core::origin::GitRef::Branch("main".to_string()),
            subdir: None,
        };

        // Test Repository reference
        let repo_origin = Origin::Repository {
            repo: "team-tools".to_string(),
            skill: "monitoring".to_string(),
            version: Some(VersionConstraint::parse("2.1.0").unwrap()),
        };

        // Test Local origin
        let _local_origin = Origin::Local {
            path: PathBuf::from("./local-skills"),
            editable: false,
        };

        // Test ZipUrl origin
        let _zip_origin = Origin::ZipUrl {
            url: "https://skills.example.com/".to_string(),
        };

        // Verify they serialize correctly
        let git_toml = toml::to_string(&git_origin).unwrap();
        assert!(git_toml.contains("type = \"git\""));

        let repo_toml = toml::to_string(&repo_origin).unwrap();
        assert!(repo_toml.contains("type = \"repository\""));
    }

    #[test]
    fn test_get_skills_for_groups_exclude_wins_over_only() {
        // S15: A skill present in both only_groups and exclude_groups MUST be excluded.
        let toml_content = r#"
            [metadata]
            version = "1.0.0"

            [[skills]]
            id = "dual-group-skill"
            origin = { type = "git", url = "https://github.com/org/repo.git" }
            groups = ["prod", "dev"]

            [[skills]]
            id = "only-prod-skill"
            origin = { type = "git", url = "https://github.com/org/repo2.git" }
            groups = ["prod"]
        "#;

        let manifest: SkillsManifest = toml::from_str(toml_content).unwrap();

        // dual-group-skill is in both "prod" (only) and "dev" (exclude) → must be excluded
        let result =
            manifest.get_skills_for_groups(Some(&["dev".to_string()]), Some(&["prod".to_string()]));

        let ids: Vec<&str> = result.iter().map(|s| s.id.as_str()).collect();
        assert!(
            !ids.contains(&"dual-group-skill"),
            "skill in both only_groups and exclude_groups must be excluded"
        );
        assert!(
            ids.contains(&"only-prod-skill"),
            "skill only in only_groups and not in exclude_groups must be included"
        );
    }

    #[test]
    fn test_from_manifest_repo_to_repository_definition() {
        let manifest_repo = RepositoryDefinition {
            name: "test-repo".to_string(),
            r#type: RepositoryType::GitMarketplace,
            priority: 1,
            connection: RepositoryConnection::GitMarketplace {
                url: "https://github.com/org/marketplace.git".to_string(),
                branch: Some("main".to_string()),
            },
            auth: None,
        };

        let repo_def = crate::core::repository::RepositoryDefinition::from(&manifest_repo);
        assert_eq!(repo_def.name, "test-repo");
        assert_eq!(repo_def.priority, 1);
        assert!(matches!(
            repo_def.repo_type,
            crate::core::repository::RepositoryType::GitMarketplace
        ));
    }
}

//! Skills manifest management for declarative skill control

use crate::core::origin::{GitRef, Origin};
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

/// The current `skill-project.toml` schema version, written by [`SkillProjectToml::save_to_file`].
///
/// **A manifest with no `schema_version` key is the pre-`Origin` legacy format**, because that
/// format shipped before this field existed — every manifest written before this feature is
/// unversioned, so "absent" is the only correct interpretation of "legacy".
///
/// Bump this when the on-disk shape changes incompatibly, and teach
/// [`SkillProjectToml::load_from_file`] to upgrade from the previous value.
pub const MANIFEST_SCHEMA_VERSION: &str = "1";

/// Root structure for skill-project.toml file
/// Contains both project metadata and dependencies
/// Works in both project-level (skill consumer) and skill-level (skill author) contexts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProjectToml {
    /// On-disk schema version. `None` means the file predates versioning (the legacy
    /// pre-`Origin` format) — reading upgrades it in memory, and the next save stamps
    /// [`MANIFEST_SCHEMA_VERSION`].
    ///
    /// Declared first so `toml` serializes it above the tables; a scalar emitted after a
    /// table would land *inside* that table and change its meaning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,
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

// ── Legacy (pre-`Origin`) manifest support ───────────────────────────────────
//
// The pre-`Origin` format spelled a dependency as a `source` discriminator plus flat
// sibling fields:
//
//     [dependencies.codescene]
//     source = "git"
//     url = "https://github.com/org/repo"
//     branch = "main"
//
// These types exist ONLY to read that shape and upgrade it. They are deliberately
// private and never serialized — nothing writes the legacy format, so there is no
// round-trip to preserve. Deleting them once no legacy manifests remain in the wild
// is the intended end state.

/// Legacy `source` discriminator. Mirrors the pre-`Origin` `DependencySource`.
#[derive(Debug, Clone, Deserialize)]
enum LegacyDependencySource {
    #[serde(rename = "git")]
    Git,
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "zip-url")]
    ZipUrl,
    /// Named `"source"` in the legacy format; it meant "a skill from a configured
    /// repository", which is now [`Origin::Repository`].
    #[serde(rename = "source")]
    Source,
}

/// Flat sibling fields of a legacy dependency entry. Every field is optional because
/// which ones are meaningful depends on `source`.
#[derive(Debug, Clone, Deserialize)]
struct LegacySourceFields {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    zip_url: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

/// A dependency entry in the legacy format.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum LegacyDependencySpec {
    /// A bare version string. Still valid today, so this arm carries through unchanged.
    Version(String),
    Inline {
        source: LegacyDependencySource,
        #[serde(flatten)]
        fields: LegacySourceFields,
        #[serde(default)]
        groups: Option<Vec<String>>,
        /// Legacy put `editable` beside `source`; `Origin::Local` now owns it.
        #[serde(default)]
        editable: Option<bool>,
    },
}

/// A whole manifest in the legacy format. Only `dependencies` differs from the current
/// shape, but the sections are re-declared rather than reused so a future change to the
/// current types cannot silently alter how legacy files are read.
#[derive(Debug, Clone, Deserialize)]
struct LegacySkillProjectToml {
    #[serde(default)]
    metadata: Option<MetadataSection>,
    #[serde(default)]
    dependencies: Option<HashMap<String, LegacyDependencySpec>>,
    #[serde(default, rename = "tool")]
    tool: Option<ToolSection>,
}

impl LegacyDependencySpec {
    /// Upgrade one legacy entry to the current shape.
    ///
    /// Returns an error rather than guessing when the required field for a `source` is
    /// missing: a dependency silently resolving to the wrong place is far worse than a
    /// migration that stops and says which entry is malformed.
    fn upgrade(self, skill_id: &str) -> Result<DependencySpec, ManifestError> {
        let (source, fields, groups, editable) = match self {
            // A bare version string means the same thing in both formats.
            LegacyDependencySpec::Version(v) => return Ok(DependencySpec::Version(v)),
            LegacyDependencySpec::Inline {
                source,
                fields,
                groups,
                editable,
            } => (source, fields, groups, editable),
        };

        let missing = |field: &str| {
            ManifestError::Parse(format!(
                "legacy dependency '{skill_id}' declares source but is missing required \
                 field '{field}'; cannot migrate it automatically"
            ))
        };

        let origin = match source {
            LegacyDependencySource::Git => Origin::Git {
                url: fields.url.ok_or_else(|| missing("url"))?,
                // Legacy could only express a branch, never a tag or commit. An absent
                // branch meant the repository default, which is `GitRef::Default`.
                r#ref: match fields.branch {
                    Some(branch) => GitRef::Branch(branch),
                    None => GitRef::Default,
                },
                subdir: None,
            },
            LegacyDependencySource::Local => Origin::Local {
                path: PathBuf::from(fields.path.ok_or_else(|| missing("path"))?),
                editable: editable.unwrap_or(false),
            },
            LegacyDependencySource::ZipUrl => Origin::ZipUrl {
                // Legacy accepted either spelling for the archive location.
                url: fields
                    .zip_url
                    .or(fields.url)
                    .ok_or_else(|| missing("zip_url"))?,
            },
            LegacyDependencySource::Source => Origin::Repository {
                repo: fields.name.ok_or_else(|| missing("name"))?,
                // The skill defaulted to the dependency's own key when unstated.
                skill: fields.skill.unwrap_or_else(|| skill_id.to_string()),
                version: match fields.version {
                    Some(raw) => Some(VersionConstraint::parse(&raw).map_err(|e| {
                        ManifestError::Parse(format!(
                            "legacy dependency '{skill_id}' has an unparseable version \
                             constraint '{raw}': {e}"
                        ))
                    })?),
                    None => None,
                },
            },
        };

        Ok(DependencySpec::Inline { origin, groups })
    }
}

impl LegacySkillProjectToml {
    /// Upgrade a whole legacy manifest, stamping the current schema version.
    fn upgrade(self) -> Result<SkillProjectToml, ManifestError> {
        let dependencies = match self.dependencies {
            Some(legacy) => {
                let mut upgraded = HashMap::with_capacity(legacy.len());
                for (id, spec) in legacy {
                    let converted = spec.upgrade(&id)?;
                    upgraded.insert(id, converted);
                }
                Some(DependenciesSection {
                    dependencies: upgraded,
                })
            }
            None => None,
        };

        Ok(SkillProjectToml {
            schema_version: Some(MANIFEST_SCHEMA_VERSION.to_string()),
            metadata: self.metadata,
            dependencies,
            tool: self.tool,
        })
    }
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

        Self::from_toml_str(&content)
    }

    /// Parse manifest content, upgrading a legacy (pre-`Origin`) file in memory.
    ///
    /// Dispatch is driven by the `schema_version` key, read in a cheap first pass (the same
    /// approach `skills.lock` uses):
    ///
    /// * **Absent** — either a legacy manifest or a hand-written current one, since nothing
    ///   stamped the field until now. Try the current shape first, and only on failure fall
    ///   back to the legacy shape. Trying current-first matters: a hand-written modern file
    ///   must not be mistaken for legacy and rewritten.
    /// * **Equal to [`MANIFEST_SCHEMA_VERSION`]** — parse the current shape, and let a parse
    ///   failure be a real error. A file that declares its version is taken at its word.
    /// * **Anything else** — refuse. A newer version means a newer FastSkill wrote it, and
    ///   guessing at a shape we do not know would corrupt it on the next save.
    ///
    /// Nothing is written to disk here. The upgrade is persisted only when something saves
    /// the manifest for its own reasons — see [`SkillProjectToml::save_to_file`].
    pub fn from_toml_str(content: &str) -> Result<Self, ManifestError> {
        /// First pass: read only `schema_version`, ignoring everything else. A legacy file
        /// does not parse as the current shape, so the version cannot be read from a full parse.
        #[derive(Deserialize)]
        struct SchemaVersionOnly {
            #[serde(default)]
            schema_version: Option<String>,
        }

        // A malformed file fails the full parse below with a better message than this peek
        // could give, so a peek failure is deliberately ignored rather than reported.
        let declared = toml::from_str::<SchemaVersionOnly>(content)
            .ok()
            .and_then(|v| v.schema_version);

        match declared.as_deref() {
            Some(MANIFEST_SCHEMA_VERSION) => Self::parse_current(content),
            Some(unknown) => Err(ManifestError::Parse(format!(
                "skill-project.toml declares schema_version '{unknown}', which this FastSkill \
                 ({}) does not understand. It was probably written by a newer FastSkill — \
                 upgrade, or remove the schema_version line if you set it by hand.",
                env!("CARGO_PKG_VERSION")
            ))),
            None => match Self::parse_current(content) {
                Ok(project) => Ok(project),
                // Report the CURRENT-format error if the legacy attempt also fails: for a file
                // that was simply malformed, the current-format diagnostic (with line numbers
                // and the pre-Origin hint) is the more useful of the two.
                Err(current_err) => match toml::from_str::<LegacySkillProjectToml>(content) {
                    Ok(legacy) => legacy.upgrade(),
                    Err(_) => Err(current_err),
                },
            },
        }
    }

    fn parse_current(content: &str) -> Result<Self, ManifestError> {
        let project: SkillProjectToml = toml::from_str(content).map_err(|e| {
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

    /// Save skill-project.toml to file, stamping the current schema version.
    ///
    /// This is where a legacy manifest becomes a current one on disk. Reading never writes,
    /// so an old file keeps working untouched until something changes it for its own reasons
    /// (`add`, `remove`, `init`, a version bump) — at which point it is rewritten in the
    /// current format, once, as a side effect of that change.
    ///
    /// Stamping here rather than at every call site means no writer can forget.
    pub fn save_to_file(&self, path: &Path) -> Result<(), ManifestError> {
        let stamped;
        let to_write = if self.schema_version.as_deref() == Some(MANIFEST_SCHEMA_VERSION) {
            self
        } else {
            stamped = SkillProjectToml {
                schema_version: Some(MANIFEST_SCHEMA_VERSION.to_string()),
                ..self.clone()
            };
            &stamped
        };

        let content = toml::to_string_pretty(to_write)
            .map_err(|e| ManifestError::Serialize(e.to_string()))?;

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
    ///
    /// `manifest_dir` is the directory this Manifest was loaded from. A local
    /// origin recorded relative to the Manifest is resolved against it here —
    /// never against the process's current directory, which has nothing to do
    /// with where the Manifest lives. Taking the directory as an argument is
    /// what makes that impossible to forget at a call site.
    pub fn to_skill_entries(&self, manifest_dir: &Path) -> Result<Vec<SkillEntry>, String> {
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
                    origin: origin.resolved_against(manifest_dir),
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
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod schema_version_tests;
